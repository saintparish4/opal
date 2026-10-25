#!/usr/bin/env python3
"""Run test262's module tests unbundled and bundled, and record both.

Every test with the `module` flag under test/language/module-code,
test/language/import, and test/language/export is run twice under Node: as
written (the baseline), and bundled by `opal build`. A test's outcome is
judged by test262's own rules: a negative test passes when it fails in the
phase and with the error its metadata names, an async test when it prints
`Test262:AsyncTestComplete`, any other test when it exits 0.

A negative test of the parse or resolution phase fails before any code runs.
Its bundled counterpart passes when `opal build` refuses it with E-PARSE or
E-LINK: refusing to build is the bundle's way of failing before any code
runs.

Each test then gets a verdict:

  pass               the bundled run passes
  refused-by-design  the bundler refused with a code SEMANTICS.md lists, and
                     the test is outside what that rule allows
  node-fails         Node fails the unbundled test too, so there is nothing
                     for the bundle to match
  bug                anything else; it needs an entry in annotations.json

The run fails if any `bug` has no annotation, and --write rewrites
results.json and RESULTS.md next to this file.

    python3 crates/opal-bundler/test262/run.py --opal target/debug/opal --write
    python3 crates/opal-bundler/test262/run.py --node-only        # baseline
    python3 crates/opal-bundler/test262/run.py --filter namespace  # a subset

The checkout is pinned (TEST262_COMMIT) and kept under target/test262, so a
re-run does not refetch it.
"""

import argparse
import concurrent.futures
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

TEST262_URL = "https://github.com/tc39/test262"
# main as of 2026-10-13. Bump deliberately: new tests change the counts.
TEST262_COMMIT = "d53ebc8b1931d9f1541e43d59650776e4b766248"
DIRECTORIES = ["test/language/module-code", "test/language/import", "test/language/export"]

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
TIMEOUT = 20

# A refusal code is a pass for a negative test of these phases: the original
# fails before running anything, and so does the build.
EARLY_REFUSALS = {"parse": {"E-PARSE"}, "resolution": {"E-LINK", "E-PARSE"}}

PRELUDE = r"""
'use strict';
const { readFileSync, writeFileSync } = require('node:fs');
const vm = require('node:vm');
const includes = (process.env.OPAL_T262_INCLUDES || '').split(',').filter(Boolean);
const harness = process.env.OPAL_T262_HARNESS;
const report = process.env.OPAL_T262_REPORT;
delete process.env.OPAL_T262_INCLUDES;
delete process.env.OPAL_T262_HARNESS;
delete process.env.OPAL_T262_REPORT;
process.on('uncaughtExceptionMonitor', (error) => {
  let name = null, message = null;
  try {
    if (error !== null && (typeof error === 'object' || typeof error === 'function')) {
      name = String(error.constructor?.name);
      message = 'message' in error ? String(error.message) : null;
    } else { name = typeof error; message = String(error); }
  } catch {}
  writeFileSync(report, JSON.stringify({ name, message }));
});
globalThis.print = (...args) => console.log(...args);
globalThis.$262 = {
  global: globalThis,
  evalScript: (source) => vm.runInThisContext(source),
  gc: () => {},
  detachArrayBuffer: (buffer) => { structuredClone(buffer, { transfer: [buffer] }); },
  agent: undefined,
};
for (const file of includes) {
  vm.runInThisContext(readFileSync(harness + '/' + file, 'utf8'), { filename: file });
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--opal", help="opal binary to bundle with")
    parser.add_argument("--node", default=os.environ.get("OPAL_NODE", "node"))
    parser.add_argument("--node-only", action="store_true", help="run the baseline only")
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    parser.add_argument("--filter", help="only tests whose path contains this")
    parser.add_argument("--write", action="store_true", help="rewrite results.json and RESULTS.md")
    parser.add_argument("--show", action="append", default=[], help="list the tests with this verdict")
    parser.add_argument("--checkout", default=str(REPO / "target" / "test262" / TEST262_COMMIT))
    args = parser.parse_args()
    if not args.node_only and not args.opal:
        parser.error("--opal is required unless --node-only")

    checkout = ensure_checkout(Path(args.checkout))
    work = Path(tempfile.mkdtemp(prefix="opal-test262-"))
    try:
        tests = prepare(checkout, work, args.filter)
        node_version = subprocess.run([args.node, "--version"], capture_output=True, text=True).stdout.strip()
        print(f"test262 {TEST262_COMMIT[:12]}, node {node_version}, {len(tests)} module tests", file=sys.stderr)

        def one(test):
            return evaluate(test, work, args)

        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            results = list(pool.map(one, tests))
    finally:
        shutil.rmtree(work, ignore_errors=True)

    annotations = json.loads((HERE / "annotations.json").read_text()) if (HERE / "annotations.json").exists() else {}
    for result in results:
        classify(result, annotations, args.node_only)

    summarize(results, node_version, args)
    for verdict in args.show:
        print(f"\n{verdict}:", file=sys.stderr)
        for result in results:
            if result["verdict"] == verdict:
                features = ",".join(result["features"])
                print(f"  {result['path']}  [{features}]", file=sys.stderr)
                print(f"      node: {result['node']['detail']}  bundle: {result['bundle'].get('refused') or result['bundle']['detail']}", file=sys.stderr)
    unannotated = [r for r in results if r["verdict"] == "bug" and not r.get("note")]
    if args.write:
        write_results(results, node_version)
    if unannotated:
        print(f"\n{len(unannotated)} failure(s) without an annotation in annotations.json:", file=sys.stderr)
        for result in unannotated:
            print(f"  {result['path']}: {result['bundle']['detail']}", file=sys.stderr)
        return 1
    return 0


def ensure_checkout(checkout):
    if (checkout / ".git").exists():
        return checkout
    checkout.parent.mkdir(parents=True, exist_ok=True)
    run_git(["clone", "--quiet", "--filter=blob:none", "--no-checkout", "--sparse", TEST262_URL, str(checkout)])
    run_git(["-C", str(checkout), "sparse-checkout", "set", *DIRECTORIES, "harness"])
    run_git(["-C", str(checkout), "checkout", "--quiet", TEST262_COMMIT])
    return checkout


def run_git(arguments):
    subprocess.run(["git", *arguments], check=True)


def prepare(checkout, work, path_filter):
    """Mirrors the test directories into `work`, under a package.json that
    makes every .js file an ES module, and returns the module tests."""
    (work / "package.json").write_text('{"type": "module"}\n')
    (work / "prelude").mkdir()
    (work / "prelude" / "prelude.cjs").write_text(PRELUDE)
    shutil.copytree(checkout / "harness", work / "harness")
    tests = []
    for directory in DIRECTORIES:
        shutil.copytree(checkout / directory, work / directory)
        for path in sorted((checkout / directory).rglob("*.js")):
            relative = path.relative_to(checkout).as_posix()
            if "_FIXTURE" in path.name or (path_filter and path_filter not in relative):
                continue
            meta = metadata(path.read_text(encoding="utf-8"))
            tests.append({"path": relative, "meta": meta})
    return tests


FRONTMATTER = re.compile(r"/\*---(.*?)---\*/", re.S)


def metadata(source):
    """The handful of frontmatter keys the runner needs, from test262's YAML
    subset: flags, includes, features, and negative.phase/type."""
    match = FRONTMATTER.search(source)
    text = match.group(1) if match else ""
    meta = {"flags": [], "includes": [], "features": [], "negative": None}
    for key in ("flags", "includes", "features"):
        found = re.search(rf"^{key}:\s*\[(.*?)\]", text, re.M | re.S)
        if found:
            meta[key] = [item.strip() for item in found.group(1).split(",") if item.strip()]
        else:
            block = re.search(rf"^{key}:\s*\n((?:\s+-\s*.+\n)+)", text, re.M)
            if block:
                meta[key] = [line.strip()[1:].strip() for line in block.group(1).splitlines() if line.strip()]
    negative = re.search(r"^negative:\s*\n\s+phase:\s*(\S+)\s*\n\s+type:\s*(\S+)", text, re.M)
    if negative:
        meta["negative"] = {"phase": negative.group(1), "type": negative.group(2)}
    return meta


def evaluate(test, work, args):
    result = evaluate_in(test, work, args)
    # The work directory is random; results name it <work> so they compare
    # across runs.
    for side in ("node", "bundle"):
        detail = result[side]["detail"]
        result[side]["detail"] = detail.replace(work.as_uri(), "<work>").replace(str(work), "<work>")
    return result


def evaluate_in(test, work, args):
    meta = test["meta"]
    result = {"path": test["path"], "features": meta["features"]}
    if "module" not in meta["flags"]:
        result["node"] = {"ok": None, "detail": "script test"}
        result["bundle"] = {"ok": None, "detail": "script test"}
        return result
    entry = work / test["path"]
    result["node"] = judge(meta, run_node(entry, meta, work, args.node))
    if args.node_only:
        result["bundle"] = {"ok": None, "detail": "not run"}
        return result

    out = work / ".opal-out" / test["path"].replace("/", "__")
    build = subprocess.run(
        [args.opal, "build", str(entry), "--root", str(work), "--outdir", str(out)],
        capture_output=True,
        text=True,
        timeout=TIMEOUT,
    )
    if build.returncode != 0:
        code = re.search(r"\b(E-[A-Z0-9-]+)\b", build.stderr)
        code = code.group(1) if code else "E-UNKNOWN"
        message = build.stderr.strip().splitlines()[0] if build.stderr.strip() else f"exit {build.returncode}"
        negative = meta["negative"]
        early = negative and code in EARLY_REFUSALS.get(negative["phase"], set())
        result["bundle"] = {"ok": bool(early), "refused": code, "detail": message[:300]}
        return result
    bundle = out / (entry.stem + ".mjs")
    result["bundle"] = judge(meta, run_node(bundle, meta, work, args.node))
    return result


def run_node(file, meta, work, node):
    includes = [] if "raw" in meta["flags"] else ["assert.js", "sta.js"]
    if "async" in meta["flags"]:
        includes.append("doneprintHandle.js")
    includes += [name for name in meta["includes"] if name not in includes]
    report = Path(tempfile.mkstemp(prefix="report-", suffix=".json")[1])
    report.unlink()
    env = {
        **os.environ,
        "OPAL_T262_INCLUDES": ",".join(includes),
        "OPAL_T262_HARNESS": str(work / "harness"),
        "OPAL_T262_REPORT": str(report),
    }
    env.pop("NODE_OPTIONS", None)
    try:
        done = subprocess.run(
            [node, "--disable-warning=ExperimentalWarning", "--disable-warning=MODULE_TYPELESS_PACKAGE_JSON", "--require", str(work / "prelude" / "prelude.cjs"), str(file)],
            cwd=file.parent,
            env=env,
            capture_output=True,
            text=True,
            timeout=TIMEOUT,
        )
        stdout, code = done.stdout, done.returncode
    except subprocess.TimeoutExpired:
        return {"timeout": True}
    uncaught = None
    if report.exists():
        uncaught = json.loads(report.read_text() or "null")
        report.unlink()
    return {"stdout": stdout, "exit": code, "uncaught": uncaught}


def judge(meta, run):
    if run.get("timeout"):
        return {"ok": False, "detail": "timed out"}
    uncaught = run["uncaught"]
    thrown = f"{uncaught['name']}: {uncaught['message']}" if uncaught else None
    negative = meta["negative"]
    if negative:
        ok = bool(uncaught) and uncaught["name"] == negative["type"]
        detail = f"threw {thrown}" if thrown else f"exited {run['exit']} without throwing"
        return {"ok": ok, "detail": detail}
    if "async" in meta["flags"]:
        complete = "Test262:AsyncTestComplete" in run["stdout"]
        failure = re.search(r"Test262:AsyncTestFailure:(.*)", run["stdout"])
        ok = complete and not failure and not uncaught
        if ok:
            return {"ok": True, "detail": "async complete"}
        detail = failure.group(1) if failure else (f"threw {thrown}" if thrown else "never called $DONE")
        return {"ok": False, "detail": detail}
    ok = run["exit"] == 0 and not uncaught
    return {"ok": ok, "detail": "exit 0" if ok else (f"threw {thrown}" if thrown else f"exit {run['exit']}")}


def classify(result, annotations, node_only):
    note = annotations.get(result["path"])
    if note:
        result["note"] = note
    node, bundle = result["node"], result["bundle"]
    if node["ok"] is None:
        result["verdict"] = "not-applicable"
    elif node_only:
        result["verdict"] = "baseline-pass" if node["ok"] else "node-fails"
    elif not node["ok"]:
        result["verdict"] = "node-fails"
    elif bundle["ok"]:
        result["verdict"] = "pass"
    elif bundle.get("refused") and bundle["refused"] not in ("E-UNKNOWN", "E-PARSE", "E-LINK"):
        result["verdict"] = "refused-by-design"
    else:
        result["verdict"] = "bug"


ORDER = ["pass", "refused-by-design", "node-fails", "bug", "baseline-pass", "not-applicable"]


def summarize(results, node_version, args):
    counts = {}
    for result in results:
        counts[result["verdict"]] = counts.get(result["verdict"], 0) + 1
    print(file=sys.stderr)
    for verdict in ORDER:
        if verdict in counts:
            print(f"  {verdict:18} {counts[verdict]}", file=sys.stderr)
    refusals = {}
    for result in results:
        if result["verdict"] == "refused-by-design":
            refusals[result["bundle"]["refused"]] = refusals.get(result["bundle"]["refused"], 0) + 1
    for code, count in sorted(refusals.items()):
        print(f"    {code:16} {count}", file=sys.stderr)


def write_results(results, node_version):
    records = []
    for result in sorted(results, key=lambda r: r["path"]):
        record = {
            "path": result["path"],
            "verdict": result["verdict"],
            "node": result["node"]["detail"],
            "bundle": result["bundle"].get("refused") or result["bundle"]["detail"],
        }
        if result.get("note"):
            record["note"] = result["note"]
        records.append(record)
    (HERE / "results.json").write_text(
        json.dumps({"test262": TEST262_COMMIT, "node": node_version, "tests": records}, indent=1) + "\n"
    )

    counts = {}
    for record in records:
        counts[record["verdict"]] = counts.get(record["verdict"], 0) + 1
    lines = [
        "# test262 results",
        "",
        f"test262 at `{TEST262_COMMIT}`, Node {node_version}. Every test with the `module` flag under "
        + ", ".join(f"`{d}`" for d in DIRECTORIES)
        + " was run as written and bundled by `opal build`. Generated by `run.py --write`; the per-test outcomes are in `results.json`.",
        "",
        "| Verdict | Tests |",
        "|---|---|",
    ]
    for verdict in ORDER:
        if verdict in counts:
            lines.append(f"| {verdict} | {counts[verdict]} |")
    lines.append(f"| total | {len(records)} |")

    refused = {}
    for record in records:
        if record["verdict"] == "refused-by-design":
            refused.setdefault(record["bundle"], []).append(record)
    if refused:
        lines += ["", "## Refused by design", "", "| Code | Tests | Rule |", "|---|---|---|"]
        for code, items in sorted(refused.items()):
            lines.append(f"| `{code}` | {len(items)} | {REFUSAL_RULES.get(code, 'see SEMANTICS.md')} |")

    for verdict, heading in (("bug", "Bugs"), ("node-fails", "Failing in Node itself")):
        items = [r for r in records if r["verdict"] == verdict]
        if not items:
            continue
        lines += ["", f"## {heading}", "", "| Test | Node | Bundle | Note |", "|---|---|---|---|"]
        for record in items:
            note = record.get("note", "")
            lines.append(
                f"| `{record['path']}` | {cell(record['node'])} | {cell(record['bundle'])} | {cell(note)} |"
            )
    (HERE / "RESULTS.md").write_text("\n".join(lines) + "\n")


REFUSAL_RULES = {
    "E-TLA": "Top-level `await` outside the entry module (rule 11).",
    "E-DYNAMIC": "`import()` the bundler cannot resolve statically (rule 10).",
    "E-IMPORT-META": "A use of `import.meta` rule 9 does not list.",
    "E-EVAL": "Direct `eval` in an ES module.",
    "E-UNSUPPORTED": "A file type the bundler does not load.",
}


def cell(text):
    return str(text).replace("|", "\\|").replace("\n", " ")[:160]


if __name__ == "__main__":
    sys.exit(main())
