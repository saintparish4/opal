#!/usr/bin/env python3
"""Time opal build against esbuild, rolldown, and bun on esbuild's own
three.js benchmark.

    python3 benchmarks/compare-bundlers.py --opal target/release/opal

The fixture is the one esbuild's Makefile builds for its published
numbers (`bench-three`): three.js r108's `src` copied ten times, and an
entry that imports each copy as a namespace and re-exports it. Every tool
bundles it to one ES module with a source map, unminified, since opal does
not minify; esbuild's own published configuration (`--minify`) is timed too,
for scale.

The method is BENCHMARKS.md's. Before timing, each tool's output is
imported under Node and its export names and one computation compared with
the unbundled program's, so a number is only reported for a bundle that
works. Then 2 untimed rounds, and --runs timed ones (default 10). Rounds
interleave the tools, each round starting from a different one, so a swing
in machine load lands on all of them. Wall time and peak RSS come from
wait4 on the tool's process. esbuild runs as its native binary, not the
Node shim npm puts on PATH; rolldown's CLI is a Node program and runs as
one.

opal is timed twice: with its module graph cache emptied before every run
(untimed), and with it warm, which is what a second build of an unchanged
project costs. Its phase timers (`--timings`) are the median of 5 more
builds of each kind. --baseline adds a second opal binary, timed warm, to
compare two builds of opal on the same rounds.

Needs git, node, bun on PATH, and `npm ci` in crates/opal-bundler/tools for
esbuild and rolldown. Raw samples go to benchmarks/results/bundlers-<date>.json;
the markdown table is printed.
"""

import argparse
import hashlib
import re
import statistics
import time
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
NODE_MODULES = REPO / "crates" / "opal-bundler" / "tools" / "node_modules"
TOOLS = NODE_MODULES / ".bin"
THREE_URL = "https://github.com/mrdoob/three.js.git"
THREE_TAG = "r108"
COPIES = 10

CHECK = """
const original = await import(process.argv[2]);
const bundled = await import(process.argv[3]);
const names = (ns) => Object.keys(ns).map((k) => k + ":" + Object.keys(ns[k]).join(",")).join("|");
if (names(original) !== names(bundled)) { console.log("different exports"); process.exit(1); }
const V = bundled.copy1.Vector3;
const v = new V(1, 2, 3).cross(new V(0, 1, 0));
if (v.x !== -3 || v.z !== 1) { console.log("Vector3 is wrong"); process.exit(1); }
console.log("ok");
"""


def run(command, cwd, capture=True):
    return subprocess.run([str(c) for c in command], cwd=cwd, capture_output=capture, text=True)


def fixture(cache):
    """three.js r108 checked out once, and the ten-copy fixture made fresh."""
    three = cache / f"three-{THREE_TAG}"
    if not (three / "src").is_dir():
        run(["git", "clone", "--quiet", "--depth", "1", "--branch", THREE_TAG, THREE_URL, three], cache)
    work = Path(tempfile.mkdtemp(prefix="opal-bench-bundlers-"))
    src = work / "src"
    src.mkdir()
    lines = []
    for i in range(1, COPIES + 1):
        shutil.copytree(three / "src", src / f"copy{i}")
        lines.append(f"import * as copy{i} from './copy{i}/Three.js'; export {{copy{i}}}")
    (src / "entry.js").write_text("\n".join(lines) + "\n")
    return work


def version(command):
    done = subprocess.run(command, capture_output=True, text=True)
    return (done.stdout or done.stderr).strip().splitlines()[0] if (done.stdout or done.stderr) else "?"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--opal", required=True)
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--baseline", help="a second opal binary to time warm on the same rounds")
    parser.add_argument("--results", default=str(HERE / "results"))
    args = parser.parse_args()
    opal = Path(args.opal).resolve()
    cache = REPO / "target" / "bench-bundlers"
    cache.mkdir(parents=True, exist_ok=True)
    work = fixture(cache)
    try:
        return measure(opal, work, args)
    finally:
        shutil.rmtree(work, ignore_errors=True)


def esbuild_binary():
    """esbuild's native binary for this platform, as its own benchmark runs it."""
    found = sorted(NODE_MODULES.glob("@esbuild/*/bin/esbuild"))
    return found[0] if found else TOOLS / "esbuild"


def timed(command, cwd):
    """Wall time, peak RSS in MB, and exit code of one run, from wait4."""
    started = time.perf_counter()
    child = subprocess.Popen([str(c) for c in command], cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _, status, usage = os.wait4(child.pid, 0)
    wall = time.perf_counter() - started
    # ru_maxrss is kilobytes on Linux and bytes on macOS.
    per_mb = 1024 * 1024 if sys.platform == "darwin" else 1024
    return wall, usage.ru_maxrss / per_mb, os.waitstatus_to_exitcode(status)


def phases(command, cwd, prepare=None, runs=5):
    """Median of each phase `opal build --timings` prints, in ms."""
    samples = {}
    for _ in range(runs):
        if prepare:
            prepare()
        out = run(command + ["--timings"], cwd).stdout.strip().splitlines()
        for name, value, unit in re.findall(r"(\w+) ([\d.]+)(ms|s|µs)", out[-1] if out else ""):
            ms = float(value) * {"s": 1000.0, "ms": 1.0, "µs": 0.001}[unit]
            samples.setdefault(name, []).append(ms)
    return {name: round(statistics.median(values), 1) for name, values in samples.items()}


def measure(opal, work, args):
    graph_cache = work / "opal-cache"
    esbuild = esbuild_binary()

    def empty_cache():
        shutil.rmtree(graph_cache, ignore_errors=True)

    opal_build = [opal, "build", "src/entry.js", "--root", ".", "--outdir", "out/opal", "--cache-dir", graph_cache]
    tools = {
        "opal (graph cache empty)": {"command": opal_build, "prepare": empty_cache, "output": "out/opal/entry.mjs"},
        "opal (graph cache warm)": {"command": opal_build, "output": "out/opal/entry.mjs"},
    }
    if args.baseline:
        baseline = Path(args.baseline).resolve()
        tools["opal baseline (graph cache warm)"] = {
            "command": [baseline, "build", "src/entry.js", "--root", ".", "--outdir", "out/opal-baseline", "--cache-dir", work / "opal-baseline-cache"],
            "output": "out/opal-baseline/entry.mjs",
        }
    tools.update({
        "esbuild": {
            "command": [esbuild, "src/entry.js", "--bundle", "--format=esm", "--sourcemap", "--outfile=out/esbuild/entry.js", "--log-level=warning"],
            "output": "out/esbuild/entry.js",
        },
        "esbuild --minify": {
            "command": [esbuild, "src/entry.js", "--bundle", "--format=esm", "--sourcemap", "--minify", "--outfile=out/esbuild-min/entry.js", "--log-level=warning"],
            "output": "out/esbuild-min/entry.js",
        },
        "rolldown": {
            "command": [TOOLS / "rolldown", "src/entry.js", "--format", "esm", "--sourcemap", "--dir", "out/rolldown", "--logLevel", "silent"],
            "output": "out/rolldown/entry.js",
        },
        "bun": {
            "command": ["bun", "build", "src/entry.js", "--format=esm", "--sourcemap=external", "--outdir=out/bun"],
            "output": "out/bun/entry.js",
        },
    })
    (work / "check.mjs").write_text(CHECK)
    (work / "package.json").write_text('{"type": "module"}\n')

    results = {}
    working = []
    for name, tool in tools.items():
        built = run(tool["command"], work)
        output = work / tool["output"]
        shown = " ".join(str(c) for c in tool["command"]).replace(str(work), "<work>").replace(str(opal), "<opal>")
        if args.baseline:
            shown = shown.replace(str(Path(args.baseline).resolve()), "<baseline>")
        shown = shown.replace(str(REPO) + "/", "<repo>/")
        entry = {"command": shown}
        results[name] = entry
        if built.returncode != 0 or not output.exists():
            entry["error"] = (built.stderr or built.stdout).strip()[:500]
            print(f"{name}: build failed", file=sys.stderr)
            continue
        checked = run(["node", "check.mjs", "./src/entry.js", "./" + tool["output"]], work)
        entry["works"] = checked.returncode == 0
        entry["check"] = checked.stdout.strip()
        entry["bytes"] = output.stat().st_size
        if not entry["works"]:
            print(f"{name}: output does not work: {checked.stdout.strip()}", file=sys.stderr)
            continue
        entry["times"], entry["peak_rss_mb"] = [], []
        working.append(name)

    if args.baseline and "opal baseline (graph cache warm)" in working:
        # Two builds of opal are only comparable if they write the same bundle.
        same = all(
            (work / "out/opal" / f).read_bytes() == (work / "out/opal-baseline" / f).read_bytes()
            for f in ("entry.mjs", "entry.mjs.map")
        )
        results["opal baseline (graph cache warm)"]["same_bundle_and_map_as_opal"] = same
        print(f"baseline writes the same bundle and map: {same}", file=sys.stderr)

    for round_index in range(2 + args.runs):
        timed_round = round_index >= 2
        shift = round_index % len(working)
        for name in working[shift:] + working[:shift]:
            tool = tools[name]
            if "prepare" in tool:
                tool["prepare"]()
            wall, rss, code = timed(tool["command"], work)
            if code != 0:
                sys.exit(f"{name} failed in round {round_index}")
            if timed_round:
                results[name]["times"].append(round(wall, 4))
                results[name]["peak_rss_mb"].append(round(rss, 1))
        if timed_round:
            print(f"round {round_index - 1}/{args.runs}", file=sys.stderr)

    for name in working:
        entry = results[name]
        times = entry["times"]
        entry.update({
            "median": statistics.median(times),
            "min": min(times),
            "max": max(times),
            "median_peak_rss_mb": statistics.median(entry["peak_rss_mb"]),
        })

    versions = {
        "opal": version([opal, "--revision"]),
        "esbuild": version([esbuild, "--version"]),
        "rolldown": version([TOOLS / "rolldown", "--version"]),
        "bun": version(["bun", "--version"]),
        "node": version(["node", "--version"]),
    }
    if args.baseline:
        versions["opal baseline"] = version([baseline, "--revision"])
    meta = {
        "date": datetime.now().astimezone().isoformat(timespec="seconds"),
        "host": f"{platform.system()} {platform.release()}, {os.cpu_count()} threads",
        "fixture": f"three.js {THREE_TAG} src x{COPIES}, " + run(["sh", "-c", "find src -name '*.js' | wc -l"], work).stdout.strip() + " files",
        "runs": args.runs,
        "versions": versions,
        "repository": run(["git", "describe", "--always", "--dirty", "--abbrev=12"], REPO).stdout.strip(),
        "opal_binary_sha256": hashlib.sha256(opal.read_bytes()).hexdigest(),
        "opal_phases_ms": {
            "graph cache empty": phases(opal_build, work, prepare=empty_cache),
            "graph cache warm": phases(opal_build, work),
        },
    }
    if args.baseline:
        meta["opal_baseline_sha256"] = hashlib.sha256(baseline.read_bytes()).hexdigest()
        meta["opal_baseline_phases_ms"] = phases(tools["opal baseline (graph cache warm)"]["command"], work)
    out_dir = Path(args.results)
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / f"bundlers-{datetime.now():%Y%m%d-%H%M%S}.json"
    out.write_text(json.dumps({"meta": meta, "results": results}, indent=2) + "\n")

    print(f"\n{meta['fixture']}; {meta['host']}; {meta['date']}; median of {args.runs} interleaved rounds")
    print("\n| Bundler | Median | Min | Max | Peak memory | Output |")
    print("|---|---|---|---|---|---|")
    for name, entry in results.items():
        if "median" not in entry:
            print(f"| {name} | - | - | - | - | {'does not work: ' + entry.get('check', entry.get('error', ''))[:60]} |")
            continue
        print(f"| {name} | {entry['median'] * 1000:.0f} ms | {entry['min'] * 1000:.0f} ms | {entry['max'] * 1000:.0f} ms "
              f"| {entry['median_peak_rss_mb']:.0f} MB | {entry['bytes'] / 1e6:.1f} MB |")
    for kind, values in meta["opal_phases_ms"].items():
        print(f"\nopal phases, {kind}: " + ", ".join(f"{k} {v:.0f} ms" for k, v in values.items()))
    if args.baseline:
        print("baseline phases, graph cache warm: " + ", ".join(f"{k} {v:.0f} ms" for k, v in meta["opal_baseline_phases_ms"].items()))
    print(f"raw results: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
