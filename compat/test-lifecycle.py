#!/usr/bin/env python3
"""Random test files, run under Jest and under opal test, compared on what
ran, in what order, and with what verdict.

    python3 compat/test-lifecycle.py --opal PATH [--seed N] [--files N] [--name-pattern RE] [--keep]

The test-runner corpus (compat/test-runners/) holds the cases somebody
thought to write down. This generates the rest: nested describe blocks,
every kind of hook, `only`, `skip`, `todo` and `failing`, and test and hook
bodies that pass, throw, reject, time out, call `done` (once, twice, with an
error), return a value, run as a generator, or throw from a timer. Every hook
and test body appends its label to a trace file as it starts, so the two
runners are compared on the order things ran, not only on the verdicts.

Jest 30.5.2 is the reference: its installed copy in
compat/test-runners/runners/ runs the same files in one invocation, and opal
test runs them in another. Any file whose trace, file status or any test's
verdict differs is printed with its source, and the run exits non-zero.
The seed makes a run reproducible; a mismatch names the seed and file.
"""

import argparse
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
JEST = HERE / "test-runners" / "runners" / "node_modules" / "jest" / "bin" / "jest.js"

HOOKS = ["beforeAll", "afterAll", "beforeEach", "afterEach"]

# Each body is JavaScript for a function, given the label it traces. They
# avoid racing timers against each other: a timeout is a body that never
# finishes under a 40 ms limit, never a close call.
TEST_BODIES = {
    "pass": "() => {{ trace('{label}'); }}",
    "throw": "() => {{ trace('{label}'); throw new Error('{label} failed'); }}",
    "throw-string": "() => {{ trace('{label}'); throw '{label} string'; }}",
    "async-pass": "async () => {{ trace('{label}'); await Promise.resolve(); }}",
    "async-reject": "async () => {{ trace('{label}'); await Promise.resolve(); throw new Error('{label} rejected'); }}",
    "timer-pass": "() => new Promise(resolve => {{ trace('{label}'); setTimeout(resolve, 1); }})",
    "done": "done => {{ trace('{label}'); setImmediate(done); }}",
    "done-sync": "done => {{ trace('{label}'); done(); }}",
    "done-error": "done => {{ trace('{label}'); setImmediate(() => done(new Error('{label} done error'))); }}",
    "done-string": "done => {{ trace('{label}'); done('{label} reason'); }}",
    "done-twice": "done => {{ trace('{label}'); done(); done(); }}",
    "done-and-promise": "async done => {{ trace('{label}'); done(); }}",
    "done-returns": "done => {{ trace('{label}'); setImmediate(done); return 1; }}",
    "never-done": "done => {{ trace('{label}'); }}",
    "never-resolves": "() => {{ trace('{label}'); return new Promise(() => {{}}); }}",
    "returns-value": "() => {{ trace('{label}'); return 42; }}",
    "generator": "function* () {{ trace('{label}'); yield Promise.resolve(1); }}",
    "generator-throw": "function* () {{ trace('{label}'); yield Promise.resolve(1); throw new Error('{label} gen'); }}",
    "uncaught": "async () => {{ trace('{label}'); setTimeout(() => {{ throw new Error('{label} uncaught'); }}); await new Promise(r => setTimeout(r, 20)); }}",
    "nested-test": "() => {{ trace('{label}'); test('nested in {label}', () => {{}}); }}",
    "nested-hook": "() => {{ trace('{label}'); beforeEach(() => {{}}); }}",
    "nested-describe": "() => {{ trace('{label}'); describe('describe in {label}', () => {{}}); }}",
}
TEST_WEIGHTS = {
    "pass": 30, "throw": 6, "throw-string": 2, "async-pass": 6, "async-reject": 3,
    "timer-pass": 3, "done": 4, "done-sync": 2, "done-error": 2, "done-string": 1,
    "done-twice": 1, "done-and-promise": 1, "done-returns": 1, "never-done": 1,
    "never-resolves": 1, "returns-value": 1, "generator": 1, "generator-throw": 1,
    "uncaught": 1, "nested-test": 1, "nested-hook": 1, "nested-describe": 1,
}
# Hooks run the same bodies, minus the ones whose meaning is about tests.
HOOK_WEIGHTS = {
    "pass": 40, "throw": 4, "async-pass": 4, "async-reject": 2, "timer-pass": 2,
    "done": 3, "done-error": 2, "done-twice": 1, "never-done": 1, "returns-value": 2,
    "generator": 1,
}
TIMES_OUT = {"never-done", "never-resolves"}


class Generator:
    def __init__(self, rng):
        self.rng = rng
        self.counter = 0

    def label(self, kind):
        self.counter += 1
        return f"{kind}{self.counter}"

    def pick(self, weights):
        names = list(weights)
        return self.rng.choices(names, weights=[weights[n] for n in names])[0]

    def body(self, weights, label):
        kind = self.pick(weights)
        timeout = ", 40" if kind in TIMES_OUT else ""
        return TEST_BODIES[kind].format(label=label), timeout

    def hooks(self, indent):
        lines = []
        for _ in range(self.rng.choice([0, 0, 1, 1, 2, 3])):
            hook = self.rng.choice(HOOKS)
            label = self.label(hook)
            body, timeout = self.body(HOOK_WEIGHTS, label)
            lines.append(f"{indent}{hook}({body}{timeout});")
        return lines

    def test(self, indent):
        label = self.label("test")
        modifier = self.rng.choices(
            ["", ".only", ".skip", ".todo", ".failing", ".each"],
            weights=[80, 3, 5, 3, 5, 4])[0]
        if modifier == ".todo":
            return [f"{indent}test.todo('{label}');"]
        body, timeout = self.body(TEST_WEIGHTS, label)
        if modifier == ".each":
            rows = self.rng.choice(["[[1], [2]]", "[['a', 'b']]", "[]"])
            return [f"{indent}test.each({rows})('{label} %s', (...args) => {{ trace('{label} ' + args.length); }});"]
        return [f"{indent}test{modifier}('{label}', {body}{timeout});"]

    def block(self, indent, depth):
        lines = self.hooks(indent)
        children = self.rng.randint(2, 6) if depth == 0 else self.rng.choice([0, 1, 2, 2, 3, 3, 4])
        for _ in range(children):
            if depth < 3 and self.rng.random() < 0.3:
                label = self.label("describe")
                modifier = self.rng.choices(["", ".only", ".skip"], weights=[85, 5, 10])[0]
                inner = self.block(indent + "  ", depth + 1)
                ending = "  return 1;" if self.rng.random() < 0.02 else ""
                lines.append(f"{indent}describe{modifier}('{label}', () => {{")
                lines.extend(inner)
                if ending:
                    lines.append(indent + ending)
                lines.append(f"{indent}}});")
            else:
                lines.extend(self.test(indent))
        lines.extend(self.hooks(indent) if self.rng.random() < 0.3 else [])
        return lines


PRELUDE = """\
const __trace = require('fs');
const __traceFile = __filename + '.trace';
const trace = label => __trace.appendFileSync(__traceFile, label + '\\n');
"""


def generate(seed, count):
    files = {}
    for index in range(count):
        rng = random.Random(f"{seed}-{index}")
        generator = Generator(rng)
        body = generator.block("", 0)
        if not any("test" in line for line in body):
            body.append("test('only test', () => { trace('only test'); });")
        files[f"case{index:04}.test.js"] = PRELUDE + "\n".join(body) + "\n"
    return files


def verdicts(report, root):
    result = {}
    for entry in json.loads(report.read_text())["testResults"]:
        name = Path(entry["name"]).name
        tests = {}
        for test in entry["assertionResults"]:
            key = " › ".join([*test["ancestorTitles"], test["title"]])
            n = 2
            base = key
            while key in tests:
                key = f"{base} [{n}]"
                n += 1
            tests[key] = test["status"]
        result[name] = {"file": "failed" if entry["status"] == "failed" else "passed", "tests": tests}
    return result


def run(cmd, cwd, env):
    return subprocess.run([str(c) for c in cmd], cwd=cwd, env=env, capture_output=True, text=True,
                          timeout=1800)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--opal", required=True)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--files", type=int, default=300)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--name-pattern", help="pass this to both runners as -t, which changes "
                                               "which tests run and so which beforeAll and "
                                               "afterAll hooks do")
    args = parser.parse_args()

    files = generate(args.seed, args.files)
    work = Path(tempfile.mkdtemp(prefix="opal-lifecycle-"))
    env = {k: v for k, v in os.environ.items() if k not in ("CI", "NODE_OPTIONS")}
    try:
        for runner in ("jest", "opal"):
            root = work / runner
            root.mkdir()
            (root / "package.json").write_text('{"private": true}\n')
            (root / "jest.config.cjs").write_text(
                "module.exports = {testEnvironment: 'node', testMatch: ['**/*.test.js']};\n")
            for name, source in files.items():
                (root / name).write_text(source)
        pattern = ["-t", args.name_pattern] if args.name_pattern else []
        jest = run(["node", JEST, "--ci=false", "--json", f"--outputFile={work / 'jest.json'}", *pattern],
                   work / "jest", env)
        opal = run([Path(args.opal).resolve(), "test", "--json", f"--outputFile={work / 'opal.json'}",
                    *pattern], work / "opal", env)
        (work / "opal.log").write_text(opal.stdout + opal.stderr)
        for name, outcome in (("jest", jest), ("opal", opal)):
            if not (work / f"{name}.json").exists():
                print(outcome.stdout[-3000:], outcome.stderr[-3000:])
                sys.exit(f"lifecycle: {name} wrote no report")
        expected = verdicts(work / "jest.json", work / "jest")
        actual = verdicts(work / "opal.json", work / "opal")
        mismatches = 0
        for name in sorted(files):
            a, b = expected.get(name), actual.get(name)
            trace_a = (work / "jest" / f"{name}.trace")
            trace_b = (work / "opal" / f"{name}.trace")
            trace_a = trace_a.read_text().splitlines() if trace_a.exists() else []
            trace_b = trace_b.read_text().splitlines() if trace_b.exists() else []
            problems = []
            if a is None or b is None:
                problems.append(f"missing from {'jest' if a is None else 'opal'}'s report")
            else:
                if a["file"] != b["file"]:
                    problems.append(f"file: jest {a['file']}, opal {b['file']}")
                for key in sorted(set(a["tests"]) | set(b["tests"])):
                    if a["tests"].get(key) != b["tests"].get(key):
                        problems.append(f"{key}: jest {a['tests'].get(key)}, opal {b['tests'].get(key)}")
            if trace_a != trace_b:
                problems.append(f"trace: jest {trace_a}\n           opal {trace_b}")
            if problems:
                mismatches += 1
                if mismatches <= 5:
                    print(f"\n=== {name} (seed {args.seed})")
                    for problem in problems[:12]:
                        print(f"  {problem}")
                    print("--- source")
                    print(files[name])
        total_tests = sum(len(v["tests"]) for v in expected.values())
        print(f"\n{len(files)} files, {total_tests} tests: "
              f"{len(files) - mismatches} identical, {mismatches} differ (seed {args.seed})")
        return 1 if mismatches else 0
    finally:
        if args.keep:
            print(f"kept: {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
