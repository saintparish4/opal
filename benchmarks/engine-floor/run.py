#!/usr/bin/env python3
"""Engine-floor timings: the two opal engine prototypes against bun, node,
jest and vitest, on this machine, in one session.

hyperfine times each command in a block, so a slow minute on the machine lands
on whichever command was running. This script interleaves instead: it runs
ROUNDS rounds, shuffles the command order each round (seeded, so a rerun
repeats the order), runs a short hyperfine block per command, and pools every
sample. Min and median come from the pooled samples. Mean is not reported,
because a single page-cache or scheduler stall moves it.

    python3 run.py                 # all scenarios
    python3 run.py hello one       # a subset
    python3 run.py --quick         # one round, for checking the commands work
    python3 run.py --resources     # peak RSS and page faults only

Needs: cargo build --release in this directory, `engine-deno snapshot` and
`node --build-snapshot` outputs (built below when missing), `opal install` in
runners/, plus bun, node and hyperfine on PATH.
"""

import json
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
T = HERE / "target"
DENO = T / "release" / "engine-deno"
NODE_ORCH = T / "release" / "engine-node"
DSNAP = T / "harness.snap"
NSNAP = T / "node-harness.blob"
EMPTY = T / "empty-static"
BIN = HERE / "runners" / "node_modules" / ".bin"


def files(d):
    return " ".join(sorted(str(p.relative_to(HERE)) for p in (HERE / d).iterdir()))


ONE = files("fixtures/one")
FORTY = files("fixtures/forty")

# (scenario, label, command, isolation model). The label is what the table
# shows; isolation is what one test file can see of another.
SCENARIOS = {
    "hello": {
        "rounds": 5, "runs": 20, "warmup": 3,
        "rows": [
            ("floor: empty static binary", str(T / "empty-static"), "-"),
            ("node hello.js", "node fixtures/hello.js", "-"),
            ("bun hello.js", "bun fixtures/hello.js", "-"),
            ("A deno_core, no snapshot", f"{DENO} run fixtures/hello.js", "-"),
            ("A deno_core, snapshot", f"{DENO} --snapshot {DSNAP} run fixtures/hello.js", "-"),
            ("B node orchestrator", f"{NODE_ORCH} run fixtures/hello.js", "-"),
        ],
    },
    "one": {
        "rounds": 5, "runs": 6, "warmup": 1,
        "rows": [
            ("jest 30.5.2", f"{BIN}/jest fixtures/one", "vm context per file"),
            ("vitest 4.1.12", f"{BIN}/vitest run --config runners/vitest.config.mjs --root . fixtures/one", "worker + module graph per file"),
            ("bun test", "bun test ./fixtures/one", "none"),
            ("node --test", f"node --test {files('fixtures/node-test/one')}", "process per file"),
            ("A deno_core, no snapshot", f"{DENO} test --isolate context {ONE}", "V8 context per file"),
            ("A deno_core, snapshot", f"{DENO} --snapshot {DSNAP} test --isolate context {ONE}", "V8 context per file"),
            ("B node orchestrator", f"{NODE_ORCH} test --isolate context {ONE}", "vm context per file"),
            ("B node orchestrator, snapshot", f"{NODE_ORCH} --snapshot {NSNAP} test --isolate context {ONE}", "vm context per file"),
        ],
    },
    "forty": {
        "rounds": 3, "runs": 4, "warmup": 1,
        "rows": [
            ("jest 30.5.2", f"{BIN}/jest fixtures/forty", "vm context per file, worker pool"),
            ("vitest 4.1.12", f"{BIN}/vitest run --config runners/vitest.config.mjs --root . fixtures/forty", "worker + module graph per file"),
            ("bun test", "bun test ./fixtures/forty", "none"),
            ("node --test", f"node --test {files('fixtures/node-test/forty')}", "process per file, parallel"),
            ("A deno_core, shared", f"{DENO} --snapshot {DSNAP} test --isolate shared {FORTY}", "none"),
            ("A deno_core, context", f"{DENO} --snapshot {DSNAP} test --isolate context {FORTY}", "V8 context per file, 1 thread"),
            ("A deno_core, runtime", f"{DENO} --snapshot {DSNAP} test --isolate runtime {FORTY}", "isolate per file, 1 thread"),
            # No snapshot for B here: Node's user-land snapshot made its start
            # slower, not faster (see the `one` scenario), and process mode
            # pays the start once per file.
            ("B node orch., shared", f"{NODE_ORCH} test --isolate shared {FORTY}", "none"),
            ("B node orch., context", f"{NODE_ORCH} test --isolate context {FORTY}", "vm context per file, 1 thread"),
            ("B node orch., process", f"{NODE_ORCH} test --isolate process {FORTY}", "process per file, parallel"),
        ],
    },
}

# Peak RSS and page faults, for the 1-file rows. Measured through
# /usr/bin/time, which forks from its own small process: a command forked
# straight from Python starts with Python's resident pages, and ru_maxrss keeps
# that high-water mark across exec.
RESOURCE_RUNS = 7
RESOURCES = ["hello", "one"]

# Each scenario's ratios are against these rows.
BASELINES = {"hello": ("bun hello.js", "node hello.js"), "one": ("bun test", "jest 30.5.2"), "forty": ("bun test", "jest 30.5.2")}


def sh(cmd):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True).stdout.strip()


def prepare():
    if not DENO.exists() or not NODE_ORCH.exists():
        subprocess.run(["cargo", "build", "--release"], cwd=HERE, check=True)
    if not DSNAP.exists():
        subprocess.run([str(DENO), "snapshot", str(DSNAP)], check=True)
    if not EMPTY.exists():
        src = T / "empty.c"
        src.write_text("int main(void) { return 0; }\n")
        subprocess.run(["gcc", "-O2", "-static", "-o", str(EMPTY), str(src)], check=True)
    if not NSNAP.exists():
        subprocess.run(["node", "--snapshot-blob", str(NSNAP), "--build-snapshot", "node/driver.cjs"], cwd=HERE, check=True, stderr=subprocess.DEVNULL)
    for tool in ("hyperfine", "bun", "node"):
        if not shutil.which(tool):
            sys.exit(f"missing {tool} on PATH")
    if not (BIN / "jest").exists():
        sys.exit("run `opal install` in runners/ first")


def environment():
    cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), platform.processor())
    return {
        "date": datetime.now().isoformat(timespec="seconds"),
        "commit": sh("git rev-parse --short HEAD"),
        "cpu": cpu,
        "cpus": os.cpu_count(),
        "kernel": platform.release(),
        "node": sh("node --version"),
        "bun": sh("bun --version"),
        "hyperfine": sh("hyperfine --version"),
        "deno_core": "0.413.0",
        "engine_deno_bytes": DENO.stat().st_size,
        "loadavg_start": open("/proc/loadavg").read().split()[:3],
    }


def busy():
    """Runnable processes right now, from /proc/stat, minus this script's one.
    Load average lags a minute; this is the contention a block starts under."""
    for line in open("/proc/stat"):
        if line.startswith("procs_running"):
            return int(line.split()[1]) - 1
    return -1


def time_block(cmd, runs, warmup):
    with tempfile.NamedTemporaryFile(suffix=".json") as out:
        subprocess.run(
            ["hyperfine", "-N", "--style", "none", "-w", str(warmup), "-r", str(runs), "--export-json", out.name, cmd],
            cwd=HERE, check=True, stdout=subprocess.DEVNULL,
        )
        return json.load(open(out.name))["results"][0]["times"]


def run(name, spec, quick):
    rounds = 1 if quick else spec["rounds"]
    runs = 2 if quick else spec["runs"]
    rng = random.Random(name)
    samples = {label: [] for label, _, _ in spec["rows"]}
    contention = []
    for r in range(rounds):
        order = spec["rows"][:]
        rng.shuffle(order)
        for label, cmd, _ in order:
            contention.append(busy())
            print(f"  [{name} {r + 1}/{rounds}] {label} (runnable: {contention[-1]})", file=sys.stderr, flush=True)
            samples[label] += time_block(cmd, runs, spec["warmup"])
    return samples, contention


def resources(spec):
    rows = []
    for label, cmd, _ in spec["rows"]:
        rss, minor, major = [], [], []
        for _ in range(RESOURCE_RUNS):
            with tempfile.NamedTemporaryFile(mode="r", suffix=".time") as t:
                subprocess.run(
                    f"/usr/bin/time -o {t.name} -f '%M %R %F' {cmd}",
                    shell=True, cwd=HERE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    executable="/bin/bash",
                )
                m, rmin, rmaj = t.read().split()[-3:]
            rss.append(int(m)); minor.append(int(rmin)); major.append(int(rmaj))
        rows.append((label, statistics.median(rss), statistics.median(minor), max(major)))
    return rows


def resource_table(rows):
    lines = ["| runner | peak RSS | minor faults | major faults (max) |", "|---|---:|---:|---:|"]
    for label, rss, minor, major in rows:
        lines.append(f"| {label} | {rss / 1024:.1f} MB | {minor:.0f} | {major} |")
    return "\n".join(lines)


def table(name, spec, samples):
    a, b = BASELINES[name]
    med = {k: statistics.median(v) for k, v in samples.items()}
    lines = [
        f"| runner | isolation | min | median | × {a} | × {b} | samples |",
        "|---|---|---:|---:|---:|---:|---:|",
    ]
    for label, _, iso in spec["rows"]:
        v = samples[label]
        lines.append(
            f"| {label} | {iso} | {fmt(min(v))} | {fmt(med[label])} | {med[label] / med[a]:.2f} | {med[label] / med[b]:.3f} | {len(v)} |"
        )
    return "\n".join(lines)


def fmt(s):
    return f"{s * 1000:.1f} ms" if s < 1 else f"{s:.2f} s"


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    quick = "--quick" in sys.argv
    prepare()
    env = environment()
    out = {"environment": env, "scenarios": {}}
    only_resources = "--resources" in sys.argv and not args
    for name in [] if only_resources else (args or SCENARIOS):
        spec = SCENARIOS[name]
        samples, contention = run(name, spec, quick)
        out["scenarios"][name] = {
            "rows": {label: {"command": cmd, "isolation": iso, "times": samples[label]} for label, cmd, iso in spec["rows"]},
            "runnable_at_block_start": contention,
        }
        print(f"\n### {name}\n\n{table(name, spec, samples)}\n", flush=True)
        print(f"other runnable processes at block starts: max {max(contention)}, median {statistics.median(contention)}\n", flush=True)
        out["environment"]["loadavg_end"] = open("/proc/loadavg").read().split()[:3]
    if only_resources or not args:
        out["resources"] = {}
        for name in RESOURCES:
            rows = resources(SCENARIOS[name])
            out["resources"][name] = {label: {"peak_rss_kb": rss, "minor_faults": mn, "major_faults_max": mj} for label, rss, mn, mj in rows}
            print(f"\n### {name}: memory and page faults (median of {RESOURCE_RUNS})\n\n{resource_table(rows)}\n", flush=True)
    if not quick:
        results = HERE / "results"
        results.mkdir(exist_ok=True)
        path = results / f"engine-floor-{datetime.now():%Y%m%d-%H%M%S}.json"
        path.write_text(json.dumps(out, indent=1))
        print(f"wrote {path.relative_to(HERE)}", file=sys.stderr)


if __name__ == "__main__":
    main()
