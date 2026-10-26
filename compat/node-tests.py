#!/usr/bin/env python3
"""Node's own tests, run by `opal run`, with a pass/fail table per file.

The tests are test/parallel/*.js from the Node release the runtime is
measured against (PINNED_NODE), fetched once into a cache with a shallow,
sparse clone of that tag. Each selected file runs unmodified: first under
node, as a control (a file that fails under node on this machine says
nothing about Opal and is reported as such), then under opal. A file passes
when opal exits 0 within the timeout, which is what Node's own runner
(tools/test.py) asks of a test/parallel file.

test/common, the library every test requires first, runs as Node ships it
except for two things it needs as it loads that Opal doesn't have yet. Its
requires of modules Opal lacks (net, worker_threads, child_process, url)
are pointed at STAND_INS, which gives test/common the few values it reads
from them while loading and makes anything else a test asks of them an
error naming the module. And the globals it lists by bare name in its
known-globals set (AbortController, structuredClone, fetch) are read through
globalThis when Opal lacks them, so they are undefined there rather than a
ReferenceError. That is the whole difference; it is printed with the
results, and the node control runs against an unedited copy.

  compat/node-tests.py run [--opal PATH] [--only GLOB ...] [--jobs N]

writes compat/results/node-tests-<date>-<time>.json and compat/NODE-TESTS.md.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import datetime
import fnmatch
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

PINNED_NODE = "v24.19.0"
REPO = "https://github.com/nodejs/node.git"
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
CACHE = Path(os.environ.get("OPAL_NODE_TESTS_CACHE", Path.home() / ".cache" / "opal-node-tests"))
TIMEOUT = 60

# The areas the runtime implements, by the test-file names Node gives them.
# A file outside these is about something Opal doesn't claim yet.
SELECTION = [
    "test-assert*.js",
    "test-buffer-*.js",
    "test-console-*.js",
    "test-event-emitter-*.js",
    "test-events-*.js",
    "test-fs-*.js",
    "test-module-*.js",
    "test-next-tick*.js",
    "test-path*.js",
    "test-process-*.js",
    "test-require-*.js",
    "test-timers*.js",
    "test-util-format*.js",
    "test-util-inspect*.js",
    "test-util-types*.js",
    "test-util.js",
]

# Modules test/common requires as it loads that Opal doesn't have, and what
# it reads from them while loading.
ABSENT = ["net", "worker_threads", "child_process", "url"]
STAND_INS = """\
'use strict';
// Written by Opal's compat/node-tests.py, not part of Node: what
// test/common reads, as it loads, from modules Opal doesn't have yet. Any
// other use of these modules is an error naming the module, so a test that
// needs one fails, saying so.

function absent(name, known) {
  return new Proxy(known, {
    get(target, key) {
      if (key in target) return target[key];
      return () => {
        throw new Error(`${name}.${String(key)}: node:${name} is not in Opal yet`);
      };
    },
  });
}

module.exports = {
  net: absent('net', {
    // Node's default (lib/net.js).
    getDefaultAutoSelectFamilyAttemptTimeout: () => 250,
    setDefaultAutoSelectFamilyAttemptTimeout() {},
  }),
  worker_threads: absent('worker_threads', {
    isMainThread: true,
    Worker: class Worker {
      constructor() {
        throw new Error('worker_threads.Worker: node:worker_threads is not in Opal yet');
      }
    },
  }),
  child_process: absent('child_process', {}),
  url: absent('url', {}),
};
"""


def node_tests() -> Path:
    """The pinned release's test/ tree, cloned into the cache if missing."""
    checkout = CACHE / f"node-{PINNED_NODE}"
    if not (checkout / "test" / "parallel").is_dir():
        CACHE.mkdir(parents=True, exist_ok=True)
        if checkout.exists():
            shutil.rmtree(checkout)
        subprocess.run(
            ["git", "clone", "--quiet", "--depth", "1", "--branch", PINNED_NODE,
             "--filter=blob:none", "--sparse", REPO, str(checkout)],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(checkout), "sparse-checkout", "set",
             "test/common", "test/parallel", "test/fixtures"],
            check=True,
        )
    return checkout / "test"


# Globals test/common names bare, as it loads.
COMMON_GLOBALS = ["AbortController", "structuredClone", "fetch"]


def missing_globals(opal: Path) -> list[str]:
    """Which of COMMON_GLOBALS opal doesn't have."""
    with tempfile.TemporaryDirectory() as scratch:
        probe = Path(scratch) / "probe.js"
        probe.write_text(f"console.log(JSON.stringify({json.dumps(COMMON_GLOBALS)}.filter((n) => !(n in globalThis))))\n")
        out = subprocess.run([str(opal), "run", str(probe)], capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


def lay_out(source: Path, test: Path, files: list[str]) -> None:
    """Lays out `test` as Node's tests expect it."""
    shutil.copytree(source / "common", test / "common")
    (test / "fixtures").symlink_to(source / "fixtures")
    (test / "parallel").mkdir()
    for name in files:
        shutil.copy2(source / "parallel" / name, test / "parallel" / name)


def prepare(source: Path, test: Path, files: list[str], globals_missing: list[str]) -> list[str]:
    """Lays out `test` for opal. Returns the edits made to test/common, one
    line each."""
    lay_out(source, test, files)
    (test / "common" / "opal-stand-ins.js").write_text(STAND_INS)
    edits = []
    index = test / "common" / "index.js"
    text = index.read_text()
    start = text.index("const knownGlobals = new Set([")
    end = text.index("]);", start)
    block = text[start:end]
    for name in globals_missing:
        entry = f"\n  {name},\n"
        if entry not in block:
            raise SystemExit(f"node-tests: test/common/index.js no longer lists {name} as a known global")
        block = block.replace(entry, f"\n  globalThis.{name},\n")
        edits.append(f"common/index.js: known global {name} -> globalThis.{name}")
    index.write_text(text[:start] + block + text[end:])
    pattern = re.compile(r"require\('(?:node:)?(" + "|".join(ABSENT) + r")'\)")
    for path in sorted((test / "common").glob("*.js")):
        if path.name == "opal-stand-ins.js":
            continue
        text = path.read_text()
        changed, count = pattern.subn(lambda m: f"require('./opal-stand-ins').{m.group(1)}", text)
        if count:
            path.write_text(changed)
            for module in sorted(set(pattern.findall(text))):
                edits.append(f"common/{path.name}: require('{module}') -> the stand-in")
    return edits


def run_one(command: list[str], cwd: Path, env: dict) -> dict:
    try:
        done = subprocess.run(command, cwd=cwd, env=env, capture_output=True,
                              timeout=TIMEOUT, stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        return {"status": "timeout", "code": None, "error": f"no exit within {TIMEOUT} s"}
    status = "pass" if done.returncode == 0 else "fail"
    return {"status": status, "code": done.returncode, "error": first_error(done.stderr)}


def first_error(stderr: bytes) -> str:
    """The line of a failure's report that says what went wrong."""
    lines = stderr.decode("utf-8", "replace").splitlines()
    for line in lines:
        if re.match(r"^(\w*Error|Error)\b.*:", line) or line.startswith("AssertionError"):
            return line[:200]
    for line in lines:
        if line.strip():
            return line.strip()[:200]
    return ""


def run(args) -> int:
    opal = Path(args.opal).resolve()
    version = subprocess.run(["node", "--version"], capture_output=True, text=True).stdout.strip()
    if version != PINNED_NODE:
        print(f"node-tests: node is {version}, the tests are {PINNED_NODE}'s", file=sys.stderr)
        return 1
    source = node_tests()
    patterns = args.only or SELECTION
    files = sorted(
        name for name in os.listdir(source / "parallel")
        if name.endswith(".js") and any(fnmatch.fnmatch(name, p) for p in patterns)
    )
    with tempfile.TemporaryDirectory(prefix="opal-node-tests-") as scratch:
        work = Path(scratch)
        lay_out(source, work / "node" / "test", files)
        edits = prepare(source, work / "opal" / "test", files, missing_globals(opal))
        results = {}

        def one(name: str) -> tuple[str, dict]:
            # Each test gets its own tmpdir (test/common/tmpdir.js names it by
            # NODE_TEST_DIR and the thread id), so tests can run in parallel.
            def env(tree: str) -> dict:
                return dict(os.environ, NODE_TEST_DIR=str(work / tree / "test"), TEST_THREAD_ID=name)

            node = run_one(["node", name], work / "node" / "test" / "parallel", env("node"))
            if node["status"] != "pass":
                return name, {"status": "node-fails", "node": node}
            opal_run = run_one([str(opal), "run", name], work / "opal" / "test" / "parallel", env("opal"))
            return name, {**opal_run, "node": node}

        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            for name, result in pool.map(one, files):
                results[name] = result
                if args.verbose:
                    print(f"{result['status']:10} {name} {result.get('error', '')}")

    opal_version = subprocess.run([str(opal), "--version"], capture_output=True, text=True).stdout.strip()
    record = {
        "date": datetime.datetime.now().isoformat(timespec="seconds"),
        "node": PINNED_NODE,
        "opal": opal_version,
        "selection": patterns,
        "common_edits": edits,
        "results": results,
    }
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    out = HERE / "results" / f"node-tests-{stamp}.json"
    out.parent.mkdir(exist_ok=True)
    out.write_text(json.dumps(record, indent=1) + "\n")
    if not args.only:
        (HERE / "NODE-TESTS.md").write_text(render(record))
    summary(record)
    print(f"results: {out}")
    return 0


def counts(results: dict) -> dict:
    out = {"pass": 0, "fail": 0, "timeout": 0, "node-fails": 0}
    for result in results.values():
        out[result["status"]] += 1
    return out


def area(name: str) -> str:
    for pattern in SELECTION:
        if fnmatch.fnmatch(name, pattern):
            return pattern
    return "other"


def summary(record: dict) -> None:
    c = counts(record["results"])
    ran = c["pass"] + c["fail"] + c["timeout"]
    print(f"{c['pass']}/{ran} pass ({c['fail']} fail, {c['timeout']} time out); "
          f"{c['node-fails']} fail under node here and aren't counted")


def render(record: dict) -> str:
    results = record["results"]
    lines = [
        "# Node's tests",
        "",
        f"Node's own tests (`test/parallel` from Node {record['node']}), run unmodified by "
        f"`opal run`, by [`node-tests.py`](./node-tests.py) on {record['date'][:10]} ({record['opal']}). "
        "A file passes when opal exits 0 within "
        f"{TIMEOUT} s, as Node's own runner requires. Every file first runs under node as a control; "
        "one that fails there is listed but not counted.",
        "",
        "The files are those for the areas the runtime implements (the selection below). "
        "`test/common` runs as Node ships it except for what it needs as it loads that Opal "
        "doesn't have yet: globals it lists by bare name are read through `globalThis`, and its "
        "requires of modules Opal lacks go to a stand-in that gives it what it reads from them "
        "while loading and makes any other use an error naming the module. The edits, all of them:",
        "",
    ]
    lines += [f"- {edit}" for edit in record["common_edits"]]
    lines += ["", "## By area", "", "| Area | Pass | Fail | Time out | Pass rate |", "|---|---|---|---|---|"]
    by_area: dict[str, dict] = {}
    for name, result in results.items():
        by_area.setdefault(area(name), {})[name] = result
    total = counts(results)
    for name in SELECTION:
        if name not in by_area:
            continue
        c = counts(by_area[name])
        ran = c["pass"] + c["fail"] + c["timeout"]
        rate = f"{100 * c['pass'] / ran:.0f}%" if ran else "-"
        lines.append(f"| `{name}` | {c['pass']} | {c['fail']} | {c['timeout']} | {rate} |")
    ran = total["pass"] + total["fail"] + total["timeout"]
    rate = f"{100 * total['pass'] / ran:.0f}%" if ran else "-"
    lines.append(f"| **all** | **{total['pass']}** | **{total['fail']}** | **{total['timeout']}** | **{rate}** |")
    lines += ["", f"{total['node-fails']} more files fail under node on the machine that ran them, "
              "so they say nothing about Opal and aren't counted.", ""]
    lines += ["## By file", "", "| File | Opal | What failed |", "|---|---|---|"]
    for name in sorted(results):
        result = results[name]
        status = {"pass": "pass", "fail": "**fail**", "timeout": "**time out**",
                  "node-fails": "(fails under node)"}[result["status"]]
        error = result.get("error", "") if result["status"] != "pass" else ""
        error = error.replace("|", "\\|").replace("`", "'")
        lines.append(f"| `{name}` | {status} | {error} |")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    go = sub.add_parser("run")
    go.add_argument("--opal", default=str(ROOT / "target" / "release" / "opal"))
    go.add_argument("--only", action="append", help="test-file glob; repeatable (no NODE-TESTS.md then)")
    go.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    go.add_argument("--verbose", "-v", action="store_true")
    args = parser.parse_args()
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
