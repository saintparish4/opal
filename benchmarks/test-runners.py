#!/usr/bin/env python3
"""opal test against Jest, Vitest, bun test and node --test, at 1, 40 and 400
test files, on this machine, in one session.

Every runner runs the same ten tests per file: a describe block with a
beforeEach, then toBe, toEqual, toThrow, an async test, toMatch, toContain,
toBeFalsy, toBeCloseTo, toBeNull and a counter. node --test gets them written
for node:test, which has no Jest globals. Each runner runs with its defaults:
its own worker count, its own caches warm (every block starts with warmup
runs), and its own isolation model, which the table names. That model is the
main cost, so a row means nothing without it.

    python3 benchmarks/test-runners.py              # 1, 40 and 400 files
    python3 benchmarks/test-runners.py 40           # one size
    python3 benchmarks/test-runners.py --quick      # one round, to check the commands

The method is benchmarks/engine-floor/run.py's. It runs several rounds, and
each round runs a short hyperfine block for every command, in an order
shuffled per round (seeded, so a rerun repeats it). Every sample is pooled,
so a slow minute on the machine lands on all the runners, not on whichever
one was running. Min and median come from the pooled samples; the mean isn't
reported, because one page-cache or scheduler stall moves it. Ratios are
medians against opal test's median at the same size.

Before timing, each command runs once and has to pass every test, so a
runner that ran nothing can't win.

Needs hyperfine, bun and node on PATH, the oracle's pinned Jest and Vitest
(`opal install` in compat/test-runners/runners) and a release build of opal.
"""

import json
import os
import platform
import random
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
OPAL = ROOT / "target" / "release" / "opal"
RUNNERS = ROOT / "compat" / "test-runners" / "runners" / "node_modules"
BIN = RUNNERS / ".bin"
WORK = ROOT / "target" / "bench" / "test-runners"

JEST_FILE = """// file {n}
let counter = 0;
beforeEach(() => {{ counter = 0; }});
describe('arith', () => {{
  test('adds', () => {{ counter++; expect(1 + 1).toBe(2); }});
  test('objects', () => {{ expect({{ a: 1, b: [1, 2, {{ c: undefined }}] }}).toEqual({{ a: 1, b: [1, 2, {{}}] }}); }});
  test('throws', () => {{ expect(() => {{ throw new Error('x'); }}).toThrow('x'); }});
  test('async', async () => {{ await Promise.resolve(); expect(counter).toBe(0); }});
  test('strings', () => {{ expect('hello world').toMatch(/wor/); }});
  test('arrays', () => {{ expect([1, 2, 3]).toContain(2); expect([1, 2, 3]).toHaveLength(3); }});
  test('truthy', () => {{ expect('').toBeFalsy(); expect(0).not.toBeTruthy(); }});
  test('numbers', () => {{ expect(0.1 + 0.2).toBeCloseTo(0.3); expect(5).toBeGreaterThan(4); }});
  test('nulls', () => {{ expect(null).toBeNull(); expect(undefined).toBeUndefined(); }});
  test('counter', () => {{ counter += 2; expect(counter).toBe(2); }});
}});
"""

NODE_FILE = """// file {n}: the same ten tests for node:test.
import {{ test, describe, beforeEach }} from "node:test";
import assert from "node:assert/strict";
let counter = 0;
beforeEach(() => {{ counter = 0; }});
describe("arith", () => {{
  test("adds", () => {{ counter++; assert.equal(1 + 1, 2); }});
  test("objects", () => {{ assert.deepEqual({{ a: 1, b: [1, 2, {{}}] }}, {{ a: 1, b: [1, 2, {{}}] }}); }});
  test("throws", () => {{ assert.throws(() => {{ throw new Error("x"); }}, /x/); }});
  test("async", async () => {{ await Promise.resolve(); assert.equal(counter, 0); }});
  test("strings", () => {{ assert.match("hello world", /wor/); }});
  test("arrays", () => {{ assert.ok([1, 2, 3].includes(2)); assert.equal([1, 2, 3].length, 3); }});
  test("truthy", () => {{ assert.ok(!""); assert.ok(!0); }});
  test("numbers", () => {{ assert.ok(Math.abs(0.1 + 0.2 - 0.3) < 0.005); assert.ok(5 > 4); }});
  test("nulls", () => {{ assert.equal(null, null); assert.equal(undefined, undefined); }});
  test("counter", () => {{ counter += 2; assert.equal(counter, 2); }});
}});
"""

# size -> rounds, runs per block, warmup runs per block
SIZES = {1: (5, 6, 2), 40: (3, 4, 1), 400: (3, 2, 1)}

# (label, command, isolation: what one test file can see of another)
ROWS = [
    ("opal test", f"{OPAL} test", "vm context per file, warm Node workers"),
    ("jest 30.5.2", f"{BIN}/jest", "vm context per file, worker processes"),
    ("vitest 4.1.12", f"{BIN}/vitest run", "worker and module graph per file"),
    ("vitest 4.1.12 --no-isolate", f"{BIN}/vitest run --no-isolate", "modules shared within a worker"),
    ("bun test 1.4.2", "bun test ./tests", "none: one global scope"),
    ("node --test 24.19", "node --test node-test/*.test.mjs", "process per file"),
]
BASELINE = "opal test"


def sh(cmd):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True).stdout.strip()


def fixture(size):
    """A project with `size` test files for every runner, made once."""
    project = WORK / f"n{size}"
    if (project / ".complete").exists():
        return project
    shutil.rmtree(project, ignore_errors=True)
    (project / "tests").mkdir(parents=True)
    (project / "node-test").mkdir()
    for n in range(1, size + 1):
        (project / "tests" / f"case{n}.test.js").write_text(JEST_FILE.format(n=n))
        (project / "node-test" / f"case{n}.test.mjs").write_text(NODE_FILE.format(n=n))
    (project / "package.json").write_text('{"name": "bench", "private": true}\n')
    (project / "jest.config.cjs").write_text(
        "module.exports = {roots: ['<rootDir>/tests'], testEnvironment: 'node', transform: {}};\n")
    (project / "vitest.config.mjs").write_text(
        "export default {test: {globals: true, include: ['tests/**/*.test.js']}};\n")
    (project / "node_modules").symlink_to(RUNNERS)
    (project / ".complete").write_text("")
    return project


def check(project, size):
    """Every command passes all its tests once before it is timed."""
    expected = size * 10
    for label, cmd, _ in ROWS:
        proc = subprocess.run(cmd, shell=True, cwd=project, capture_output=True, text=True)
        out = proc.stdout + proc.stderr
        # "10 passed" (Jest, Vitest, opal), "10 pass" (bun), "pass 10" (node).
        counts = [int(m) for m in re.findall(r"(\d+) pass(?:ed)?\b|\bpass (\d+)", out) for m in m if m]
        if proc.returncode != 0 or expected not in counts:
            sys.exit(f"{label} at {size} files didn't pass {expected} tests (exit {proc.returncode}):\n{out[-1500:]}")


def busy():
    for line in open("/proc/stat"):
        if line.startswith("procs_running"):
            return int(line.split()[1]) - 1
    return -1


def time_block(project, cmd, runs, warmup):
    with tempfile.NamedTemporaryFile(suffix=".json") as out:
        subprocess.run(
            ["hyperfine", "-N", "--style", "none", "-w", str(warmup), "-r", str(runs), "--export-json", out.name, cmd],
            cwd=project, check=True, stdout=subprocess.DEVNULL,
        )
        return json.load(open(out.name))["results"][0]["times"]


def measure(size, quick):
    rounds, runs, warmup = SIZES[size]
    if quick:
        rounds, runs = 1, 1
    project = fixture(size)
    check(project, size)
    rng = random.Random(size)
    samples = {label: [] for label, _, _ in ROWS}
    contention = []
    for r in range(rounds):
        order = ROWS[:]
        rng.shuffle(order)
        for label, cmd, _ in order:
            contention.append(busy())
            print(f"  [{size} files, round {r + 1}/{rounds}] {label} (runnable: {contention[-1]})", file=sys.stderr, flush=True)
            samples[label] += time_block(project, cmd, runs, warmup)
    return samples, contention


def fmt(seconds):
    return f"{seconds * 1000:.0f} ms" if seconds < 1 else f"{seconds:.2f} s"


def table(size, samples):
    median = {label: statistics.median(times) for label, times in samples.items()}
    lines = [
        f"| runner | isolation | min | median | × opal test | samples |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for label, _, isolation in ROWS:
        times = samples[label]
        lines.append(
            f"| {label} | {isolation} | {fmt(min(times))} | {fmt(median[label])} | "
            f"{median[label] / median[BASELINE]:.2f} | {len(times)} |")
    return "\n".join(lines)


def environment():
    cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")),
               platform.processor())
    return {
        "date": datetime.now().isoformat(timespec="seconds"),
        "commit": sh(f"git -C {ROOT} rev-parse --short HEAD"),
        "cpu": cpu,
        "cpus": os.cpu_count(),
        "kernel": platform.release(),
        "node": sh("node --version"),
        "bun": sh("bun --version"),
        "hyperfine": sh("hyperfine --version"),
        "opal_sha256": sh(f"sha256sum {OPAL}").split()[0],
        "loadavg_start": open("/proc/loadavg").read().split()[:3],
    }


def main():
    sizes = [int(a) for a in sys.argv[1:] if not a.startswith("--")] or list(SIZES)
    quick = "--quick" in sys.argv
    for tool in ("hyperfine", "bun", "node"):
        if not shutil.which(tool):
            sys.exit(f"missing {tool} on PATH")
    if not OPAL.exists() or not (BIN / "jest").exists():
        sys.exit("needs `cargo build --release` and `opal install` in compat/test-runners/runners")
    env = environment()
    out = {"environment": env, "sizes": {}}
    for size in sizes:
        samples, contention = measure(size, quick)
        out["sizes"][str(size)] = {
            "rows": {label: {"command": cmd, "isolation": iso, "times": samples[label]} for label, cmd, iso in ROWS},
            "runnable_at_block_start": contention,
        }
        print(f"\n### {size} file{'s' if size > 1 else ''}, {size * 10} tests\n\n{table(size, samples)}\n", flush=True)
        print(f"other runnable processes at block starts: max {max(contention)}, "
              f"median {statistics.median(contention)}\n", flush=True)
    out["environment"]["loadavg_end"] = open("/proc/loadavg").read().split()[:3]
    if not quick:
        results = HERE / "results"
        results.mkdir(exist_ok=True)
        path = results / f"test-runners-{datetime.now():%Y%m%d-%H%M%S}.json"
        path.write_text(json.dumps(out, indent=1))
        print(f"wrote {path.relative_to(ROOT)}", file=sys.stderr)


if __name__ == "__main__":
    main()
