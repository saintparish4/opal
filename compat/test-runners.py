#!/usr/bin/env python3
"""Run one corpus of test files under jest, vitest, bun test and opal test, and
diff what each says about every test, and what each writes.

    python3 compat/test-runners.py [--opal PATH] [--runners jest,vitest,bun,opal]
                                   [FIXTURE ...] [--repeat N] [--no-mutants]
                                   [--reclassify] [--keep]

This is the oracle opal test is written against, so it exists before the
runner does. Without --opal the opal column is "not run" and the three
reference runners are checked against each other.

The corpus is compat/test-runners/corpus/<class>/<dialect>/<fixture>/, one
test file per fixture plus its helpers and committed snapshots, copied from
Jest's e2e fixtures and Vitest's core suite by compat/test-runners/seed.py,
which records where every byte came from. The dialect says whose semantics a
fixture is written against, and so which runner is the reference for it:
jest for files that use Jest's globals, vitest for files that import from
`vitest`. Every other runner that can run the fixture is compared with the
reference:
  jest    runs the jest dialect only (it has no `vitest` module);
  vitest  runs the vitest dialect, and the jest dialect with globals on,
          except files that use the `jest` object or @jest/globals;
  bun     runs both, as `bun test --isolate`;
  opal    runs both.

What is compared, per test file:
  verdicts  pass, fail, skip or todo for every test, keyed by its describe
            path and title, plus "(file)", the file's own pass or fail, which
            is where errors outside any test land (a throwing afterAll, a
            file that fails to load);
  writes    every file the run created, changed or deleted in the fixture:
            snapshot files and inline snapshots rewritten into the source.

Two differences are systematic, so they are applied to every snapshot file
before comparing instead of being pinned per fixture: the header line, which
names the runner that wrote it, and Vitest's ` > ` between the titles of a
snapshot key where Jest and bun put a space. Each runner must write exactly
its own header and key format; anything else stays a difference. opal is
expected to write the dialect's format, Jest's for jest files and Vitest's for
vitest files, since that is what lets it round-trip a project's snapshots.

The two classes are held to different things, the way
crates/opal-pm/tests/npm-cross-check.rs holds its fixtures:
  agrees            every runner that ran gives the reference's verdicts and
                    writes the reference's files;
  known_difference  each runner differs from the reference exactly as the
                    fixture's PIN.json says, no more and no less. A pin
                    failing means the difference grew, shrank, or was fixed,
                    and the fix is a decision about that difference and an
                    edit to the pin, not a retry. Every pin carries a "why".

The mutation set: for every test file, mutate.cjs toggles `.not` on one
assertion inside a test, a few candidates spread over the file. The
reference runner decides which candidate is the file's planted failure: the
first that turns exactly one passing test into a failing one and changes
nothing else. Every other runner that passed that test before the mutation
must fail it after. A runner that doesn't has a difference like any other,
`"mutant"` in its pin, with one exception: opal can't pin one. A false pass
in opal is always a failure.

--reclassify moves a fixture whose runners disagree into known_difference
with a draft PIN.json ("why": "TODO"), and one that no longer differs back to
agrees. A TODO is a failure until a person replaces it with the reason.

opal is run as `opal test --json --outputFile=<file>` in a folder holding the
corpus, and must write Jest's --json report (the format vitest's json
reporter also writes). Needs node and bun on PATH, and the pinned reference
runners installed with `opal install` in compat/test-runners/runners/.
The run's record goes to compat/results/ as JSON: what ran, each fixture's
outcome and the planted failures. The pins already hold every difference;
--full adds every runner's verdict for every test, with failure messages,
which is what curating pins needs. The summary table is printed. Exits
non-zero on any agrees difference, pin mismatch, TODO, unstable fixture or
planted failure a runner let through that its pin doesn't name.
"""

import argparse
import difflib
import hashlib
import json
import os
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
ORACLE = HERE / "test-runners"
CORPUS = ORACLE / "corpus"
NODE_MODULES = ORACLE / "runners" / "node_modules"
CLASSES = ("agrees", "known_difference")
RUNNERS = ("jest", "vitest", "bun", "opal")
REFERENCE = {"jest": "jest", "vitest": "vitest"}
FILE = "(file)"
TEST_FILE = re.compile(r"\.(test|spec)\.[cm]?[jt]s$")
JEST_ONLY = re.compile(r"\bjest\s*\.|@jest/globals")
MUTANT_CANDIDATES = 5
TIMEOUT = 900

SNAPSHOT_HEADERS = {
    "jest": "// Jest Snapshot v1, https://jestjs.io/docs/snapshot-testing",
    "vitest": "// Vitest Snapshot v1, https://vitest.dev/guide/snapshot.html",
    "bun": "// Bun Snapshot v1, https://bun.sh/docs/test/snapshots",
}
SNAPSHOT_KEY = re.compile(r"^exports\[`(.*?)`\] = ", re.M | re.S)

JEST_CONFIG = """\
module.exports = {
  rootDir: __dirname,
  testEnvironment: 'node',
  testMatch: ['**/*.test.?([cm])[jt]s', '**/*.spec.?([cm])[jt]s'],
  watchman: false,
  cacheDirectory: __dirname + '/.jest-cache',
};
"""
VITEST_CONFIG = """\
export default {
  cacheDir: '.vite',
  test: {
    globals: true,
    include: ['**/*.{test,spec}.?(c|m)[jt]s'],
    exclude: ['**/node_modules/**'],
    testTimeout: 5000,
    hookTimeout: 5000,
    allowOnly: true,
    watch: false,
  },
};
"""
# CI detection changes what every runner does with a missing snapshot: in CI
# it is a failure, outside it the snapshot is written. The oracle compares
# what gets written, so it always runs outside CI.
CI_VARIABLES = ("CI", "CONTINUOUS_INTEGRATION", "BUILD_NUMBER", "RUN_ID", "GITHUB_ACTIONS",
                "GITLAB_CI", "BUILDKITE", "CIRCLECI", "TRAVIS", "JENKINS_URL", "TF_BUILD")


class Fixture:
    def __init__(self, cls, dialect, path):
        self.cls, self.dialect, self.path, self.name = cls, dialect, path, path.name
        self.tests = sorted(p.relative_to(path).as_posix() for p in path.rglob("*")
                            if p.is_file() and TEST_FILE.search(p.name)
                            and "__snapshots__" not in p.parts)
        pin = path / "PIN.json"
        self.pin = json.loads(pin.read_text()) if pin.exists() else None
        self.uses_jest_object = any(JEST_ONLY.search((path / test).read_text())
                                    for test in self.tests)

    @property
    def id(self):
        return f"{self.dialect}/{self.name}"

    @property
    def rel(self):
        return f"{self.cls}/{self.dialect}/{self.name}"

    @property
    def reference(self):
        return REFERENCE[self.dialect]

    def files(self):
        return {p.relative_to(self.path).as_posix(): p.read_bytes()
                for p in self.path.rglob("*") if p.is_file() and p.name != "PIN.json"}

    def runs_on(self, runner):
        if runner == "jest":
            return self.dialect == "jest"
        if runner == "vitest" and self.dialect == "jest":
            return not self.uses_jest_object
        return True


def discover(names):
    fixtures = []
    for cls in CLASSES:
        for dialect in REFERENCE:
            folder = CORPUS / cls / dialect
            if folder.exists():
                fixtures += [Fixture(cls, dialect, p) for p in sorted(folder.iterdir()) if p.is_dir()]
    if names:
        unknown = [n for n in names if not any(n in (f.id, f.name) for f in fixtures)]
        if unknown:
            sys.exit(f"test-runners: no fixture named {', '.join(unknown)}")
        fixtures = [f for f in fixtures if f.id in names or f.name in names]
    return fixtures


# --- running ---------------------------------------------------------------

def kill_group(process):
    try:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def run(cmd, cwd, env, log):
    with open(log, "w") as out:
        started = time.perf_counter()
        child = subprocess.Popen([str(c) for c in cmd], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                 stdout=out, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            code = None
        finally:
            kill_group(child)
    return code, time.perf_counter() - started


def lay_out(root, entries):
    """Write each (relative folder, files) into a fresh root a runner runs in."""
    root.mkdir(parents=True)
    (root / "node_modules").symlink_to(NODE_MODULES)
    (root / "package.json").write_text('{\n  "private": true\n}\n')
    (root / "jest.config.cjs").write_text(JEST_CONFIG)
    (root / "vitest.config.mjs").write_text(VITEST_CONFIG)
    for folder, files in entries:
        for rel, data in files.items():
            target = root / folder / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)


def command(runner, opal, report):
    if runner == "jest":
        return ["node", NODE_MODULES / "jest" / "bin" / "jest.js", "--ci=false", "--json",
                f"--outputFile={report}"]
    if runner == "vitest":
        return ["node", NODE_MODULES / "vitest" / "vitest.mjs", "run", "--reporter=json",
                f"--outputFile={report}"]
    if runner == "bun":
        return ["bun", "test", "--isolate", "--timeout=5000", "--reporter=junit",
                f"--reporter-outfile={report}"]
    return [opal, "test", "--json", f"--outputFile={report}"]


def add(tests, titles, verdict):
    key = " › ".join(titles)
    if key in tests:
        n = 2
        while f"{key} [{n}]" in tests:
            n += 1
        key = f"{key} [{n}]"
    tests[key] = verdict


JSON_STATUS = {"passed": "pass", "failed": "fail", "pending": "skip", "skipped": "skip",
               "disabled": "skip", "todo": "todo", "focused": "pass"}


def clip(message):
    """Failure messages are kept for reading, never compared; long stacks are cut."""
    return re.sub(r"\x1b\[[0-9;]*m", "", message)[:1500]


def from_jest_json(report, root):
    """Jest's --json report, which vitest's json reporter also writes."""
    results = {}
    for entry in json.loads(report.read_text())["testResults"]:
        tests, failures = {}, {}
        for test in entry["assertionResults"]:
            add(tests, [*test["ancestorTitles"], test["title"]], JSON_STATUS[test["status"]])
            if test.get("failureMessages"):
                failures[list(tests)[-1]] = clip("\n".join(str(m) for m in test["failureMessages"]))
        results[Path(entry["name"]).relative_to(root).as_posix()] = {
            "file": "fail" if entry["status"] == "failed" else "pass",
            "tests": tests, "message": clip(entry.get("message", "")), "failures": failures}
    return results


def from_bun_junit(report, root):
    """bun's JUnit report: a testsuite per file, nested one per describe.

    A hook that throws shows up as a failed testcase named "(unnamed)"; it is
    the file's failure, not a test. A file that throws while loading is not
    in the report at all; read_results marks it failed.
    """
    results = {}
    for suite in ET.parse(report).getroot().findall("testsuite"):
        tests, errors, failures = {}, [], {}

        def visit(node, titles):
            for child in node:
                if child.tag == "testsuite":
                    visit(child, [*titles, child.get("name")])
                elif child.tag == "testcase":
                    failure = child.find("failure")
                    if failure is None:
                        failure = child.find("error")
                    skipped = child.find("skipped")
                    if failure is not None:
                        verdict = "fail"
                    elif skipped is not None:
                        verdict = "todo" if skipped.get("message") == "TODO" else "skip"
                    else:
                        verdict = "pass"
                    if child.get("name") == "(unnamed)":
                        if verdict == "fail":
                            errors.append(failure.get("message", ""))
                        continue
                    add(tests, [*titles, child.get("name")], verdict)
                    if failure is not None:
                        failures[list(tests)[-1]] = clip(failure.text or failure.get("message", ""))

        visit(suite, [])
        failed = bool(errors) or "fail" in tests.values()
        results[suite.get("file") or suite.get("name")] = {
            "file": "fail" if failed else "pass", "tests": tests,
            "message": clip("\n".join(errors)), "failures": failures}
    return results


def console_section(log, rel):
    """What a runner printed under a file's heading, for a file its report left out."""
    text = re.sub(r"\x1b\[[0-9;]*m", "", log.read_text(errors="replace"))
    start = text.find(f"\n{rel}:\n")
    if start < 0:
        return ""
    end = re.search(r"\n(?:corpus|mutants)/\S+:\n", text[start + 1:])
    return text[start + 1:start + 1 + end.start()] if end else text[start + 1:]


def read_results(runner, report, root, expected, log):
    if not report.exists():
        parsed = {}
    elif runner == "bun":
        parsed = from_bun_junit(report, root)
    else:
        parsed = from_jest_json(report, root)
    for rel in expected:
        if rel not in parsed:
            parsed[rel] = {"file": "fail", "tests": {}, "failures": {},
                           "message": clip(f"not in {runner}'s report\n"
                                           + console_section(log, rel))}
    return parsed


def run_runner(runner, opal, root, test_paths, env, label):
    report = root.parent / f"{root.name}.{'xml' if runner == 'bun' else 'json'}"
    log = root.parent / f"{root.name}.log"
    code, seconds = run(command(runner, opal, report), root, env, log)
    results = read_results(runner, report, root, test_paths, log)
    print(f"  {label:<24} exit {code if code is not None else 'timeout'}, "
          f"{len(test_paths)} files, {seconds:.1f} s", flush=True)
    return results, {"exit": code, "seconds": round(seconds, 2), "log": str(log)}


# --- comparing -------------------------------------------------------------

def snapshot_format(runner, dialect):
    """(header, key separator) a runner is expected to write."""
    if runner == "opal":
        runner = REFERENCE[dialect]
    return SNAPSHOT_HEADERS[runner], " > " if runner == "vitest" else " "


def normalize(rel, data, runner, dialect):
    """Strip the systematic differences from a written snapshot file."""
    if data is None or not rel.endswith(".snap"):
        return data
    text = data.decode("utf-8", errors="replace")
    header, separator = snapshot_format(runner, dialect)
    first, newline, rest = text.partition("\n")
    if first != header:
        return text
    if separator != " ":
        rest = SNAPSHOT_KEY.sub(lambda m: f"exports[`{m.group(1).replace(separator, ' ')}`] = ", rest)
    return "// Snapshot v1" + newline + rest


def writes(fixture, folder, runner):
    """{relative path: normalized content, None when deleted} for one fixture."""
    before = fixture.files()
    after = {p.relative_to(folder).as_posix(): p.read_bytes()
             for p in folder.rglob("*") if p.is_file()}
    changed = {}
    for rel in sorted(set(before) | set(after)):
        if before.get(rel) != after.get(rel):
            data = after.get(rel)
            changed[rel] = normalize(rel, data, runner, fixture.dialect)
            if isinstance(changed[rel], bytes):
                changed[rel] = changed[rel].decode("utf-8", errors="replace")
    return changed


def text_diff(rel, ours, theirs, reference, runner):
    a = [] if ours is None else ours.splitlines()
    b = [] if theirs is None else theirs.splitlines()
    head = [f"{reference}: {'not written' if ours is None else 'written'}, "
            f"{runner}: {'not written' if theirs is None else 'written'}"]
    return "\n".join(head + list(difflib.unified_diff(a, b, f"{reference}/{rel}",
                                                      f"{runner}/{rel}", lineterm="", n=1)))


def difference(fixture, observed, runner):
    """What `runner` did differently from the fixture's reference, or {}."""
    reference = fixture.reference
    ours, theirs = observed[reference], observed[runner]
    verdicts = {}
    for test in fixture.tests:
        a, b = ours["results"][test], theirs["results"][test]
        rows = {FILE: [a["file"], b["file"]]} if a["file"] != b["file"] else {}
        for key in sorted(set(a["tests"]) | set(b["tests"])):
            if a["tests"].get(key) != b["tests"].get(key):
                rows[key] = [a["tests"].get(key), b["tests"].get(key)]
        if rows:
            verdicts[test] = rows
    written = {}
    for rel in sorted(set(ours["writes"]) | set(theirs["writes"])):
        if ours["writes"].get(rel, False) != theirs["writes"].get(rel, False):
            written[rel] = text_diff(rel, ours["writes"].get(rel), theirs["writes"].get(rel),
                                     reference, runner)
    found = {}
    if verdicts:
        found["verdicts"] = verdicts
    if written:
        found["written"] = written
    return found


def check(fixture, observed, ran, misses):
    """(problems, {runner: difference}) for one fixture.

    `misses` is {runner: {"key", "verdict"}} for the planted failure each
    runner let through.
    """
    problems, found = [], {}
    if fixture.reference not in ran:
        return problems, found
    for runner in ran:
        if runner == fixture.reference or not fixture.runs_on(runner):
            continue
        found[runner] = difference(fixture, observed, runner)
        if runner in misses:
            found[runner]["mutant"] = misses[runner]
    if "mutant" in found.get("opal", {}):
        miss = found["opal"]["mutant"]
        problems.append(f"opal let the planted failure in {miss['key']} through "
                        f"({miss['verdict'] or 'not reported'})")
    if "mutant" in (fixture.pin or {}).get("opal", {}):
        problems.append("opal's pin excuses a planted failure; opal may not have false passes")
    if fixture.cls == "agrees":
        for runner, diff in found.items():
            if diff:
                problems.append(f"{runner} differs from {fixture.reference}")
        return problems, found
    pin = fixture.pin or {}
    for runner, diff in found.items():
        pinned = {k: v for k, v in pin.get(runner, {}).items() if k != "why"}
        if diff == pinned:
            continue
        if not pinned:
            problems.append(f"{runner} differs from {fixture.reference}, and no pin says so")
        elif not diff:
            problems.append(f"{runner} no longer differs from {fixture.reference}; "
                            f"the pin is stale")
        else:
            problems.append(f"{runner} differs from {fixture.reference} other than as pinned")
    for runner, entry in pin.items():
        if runner in found and found[runner] and not entry.get("why", "").strip():
            problems.append(f"{runner}'s pin has no why")
        elif runner in found and entry.get("why", "").startswith("TODO"):
            problems.append(f"{runner}'s pin gives a TODO as its why")
    pinned_runners = [r for r in pin if r in RUNNERS]
    if pinned_runners and all(r in found for r in pinned_runners) and not any(found.values()):
        problems.append("every runner agrees now; it belongs in agrees")
    return problems, found


def reclassify(fixture, found):
    """Move a fixture to the class its observed differences put it in."""
    pin = dict(fixture.pin or {})
    for runner, diff in found.items():
        if not diff:
            pin.pop(runner, None)
            continue
        old = pin.get(runner, {})
        why = old.get("why") if {k: v for k, v in old.items() if k != "why"} == diff else None
        pin[runner] = {"why": why or "TODO: why this runner differs", **diff}
    cls = "known_difference" if any(pin.get(r) for r in RUNNERS) else "agrees"
    target = CORPUS / cls / fixture.dialect / fixture.name
    if target != fixture.path:
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(fixture.path, target)
    if cls == "known_difference":
        (target / "PIN.json").write_text(json.dumps(pin, indent=2, ensure_ascii=False) + "\n")
    elif (target / "PIN.json").exists():
        (target / "PIN.json").unlink()
    return cls


# --- mutants ---------------------------------------------------------------

def make_mutants(fixtures, scratch):
    """{fixture id: [{"line", "column", "matcher", "op", "files"}]}: the candidates
    mutate.cjs picked for each fixture, assertion flips first."""
    requests = [{"file": str(f.path / t), "max": MUTANT_CANDIDATES} for f in fixtures for t in f.tests]
    if not requests:
        return {}
    request = scratch / "mutate-request.json"
    request.write_text(json.dumps(requests))
    with open(request) as stdin:
        out = subprocess.run(["node", ORACLE / "mutate.cjs"], stdin=stdin, capture_output=True,
                             text=True, check=True).stdout
    generated = json.loads(out)
    mutants = {}
    for fixture in fixtures:
        files = fixture.files()
        for test in fixture.tests:
            for index, mutant in enumerate(generated[str(fixture.path / test)]["mutants"]):
                mutated = dict(files)
                mutated[test] = mutant["source"].encode()
                mutants.setdefault(fixture.id, []).append({
                    "test": test, "candidate": index, "line": mutant["line"],
                    "column": mutant["column"], "matcher": mutant["matcher"], "op": mutant["op"],
                    "files": mutated})
    return mutants


def mutant_folder(fixture, index):
    return f"mutants/{fixture.dialect}/{fixture.name}/m{index}"


def judge_mutants(fixtures, mutants, observed, mutant_runs, ran):
    """Pick each file's planted failure on the reference, then hold every runner to it.

    Returns (planted, unplanted, {runner: counts}, {fixture id: {runner: miss}}).
    A runner is held to a planted failure only where it passed that test
    before the mutation, as the reference did; where it didn't, the fixture's
    verdicts already differ and that is what its pin is about.
    """
    planted, unplanted, misses = [], [], {}
    report = {r: {"planted": 0, "caught": 0, "missed": 0} for r in ran}
    for fixture in fixtures:
        reference = fixture.reference
        if reference not in mutant_runs or fixture.id not in mutants:
            continue
        original = observed[fixture.id][reference]["results"]
        chosen = None
        for index, mutant in enumerate(mutants[fixture.id]):
            test = mutant["test"]
            after = mutant_runs[reference].get(f"{mutant_folder(fixture, index)}/{test}")
            before = original[test]["tests"]
            if after is None:
                continue
            changed = [k for k in set(before) | set(after["tests"])
                       if before.get(k) != after["tests"].get(k)]
            if len(changed) == 1 and before.get(changed[0]) == "pass" \
                    and after["tests"].get(changed[0]) == "fail":
                chosen = (index, mutant, changed[0])
                break
        if chosen is None:
            unplanted.append(fixture.id)
            continue
        index, mutant, key = chosen
        entry = {"fixture": fixture.id, "test": mutant["test"], "key": key,
                 "line": mutant["line"], "column": mutant["column"], "matcher": mutant["matcher"],
                 "op": mutant["op"], "verdicts": {}}
        for runner in ran:
            if not fixture.runs_on(runner) or runner not in mutant_runs:
                continue
            before = observed[fixture.id][runner]["results"][mutant["test"]]["tests"]
            if before.get(key) != "pass":
                continue
            result = mutant_runs[runner].get(f"{mutant_folder(fixture, index)}/{mutant['test']}")
            verdict = result["tests"].get(key) if result else None
            entry["verdicts"][runner] = verdict
            if runner == reference:
                continue
            report[runner]["planted"] += 1
            if verdict == "fail":
                report[runner]["caught"] += 1
            else:
                report[runner]["missed"] += 1
                misses.setdefault(fixture.id, {})[runner] = {
                    "key": f"{mutant['test']}: {key}", "verdict": verdict,
                    "mutation": f"{mutant['op']}"
                    + (f" on {mutant['matcher']}" if mutant["matcher"] else "")
                    + f" at line {mutant['line']}"}
        planted.append(entry)
    return planted, unplanted, report, misses


# --- the run ---------------------------------------------------------------

def versions(env, opal):
    def out(cmd):
        try:
            return subprocess.run([str(c) for c in cmd], capture_output=True, text=True,
                                  env=env).stdout.strip()
        except OSError:
            return None

    def package(name):
        return json.loads((NODE_MODULES / name / "package.json").read_text())["version"]

    found = {"node": out(["node", "--version"]).removeprefix("v"), "bun": out(["bun", "--version"]),
             "jest": package("jest"), "vitest": package("vitest")}
    if opal:
        found["opal"] = (out([opal, "--version"]) or "?").removeprefix("opal ")
    return found


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("fixtures", nargs="*", metavar="FIXTURE",
                        help="dialect/name or name of fixtures to run (default: all)")
    parser.add_argument("--opal", help="an opal binary whose `opal test` to check")
    parser.add_argument("--runners", default=None,
                        help=f"comma-separated subset of {','.join(RUNNERS)}")
    parser.add_argument("--repeat", type=int, default=1,
                        help="run the corpus this many times per runner; any change between "
                             "runs marks the fixture unstable")
    parser.add_argument("--no-mutants", action="store_true", help="skip the mutation set")
    parser.add_argument("--reclassify", action="store_true",
                        help="move fixtures between agrees and known_difference, writing draft pins")
    parser.add_argument("--keep", action="store_true", help="keep the scratch folder")
    parser.add_argument("--full", action="store_true",
                        help="record every runner's verdict and failure message for every test")
    parser.add_argument("--results", default=str(HERE / "results"))
    args = parser.parse_args()

    if not NODE_MODULES.exists():
        sys.exit("test-runners: the reference runners aren't installed; "
                 "run `opal install` in compat/test-runners/runners")
    opal = str(Path(args.opal).resolve()) if args.opal else None
    ran = [r for r in (args.runners.split(",") if args.runners else RUNNERS)
           if r != "opal" or opal]
    unknown = [r for r in ran if r not in RUNNERS]
    if unknown:
        sys.exit(f"test-runners: no runner named {', '.join(unknown)}")
    fixtures = discover(args.fixtures)

    scratch = Path(tempfile.mkdtemp(prefix="opal-test-runners-")).resolve()
    env = {k: v for k, v in os.environ.items()
           if k not in CI_VARIABLES and k not in ("NODE_OPTIONS", "NODE_ENV")}
    (scratch / "tmp").mkdir()
    env.update({"TMPDIR": str(scratch / "tmp"), "TZ": "UTC"})
    meta = {
        "date": datetime.now().isoformat(timespec="seconds"),
        "host": f"{platform.system()} {platform.release()}, {os.cpu_count()} threads",
        "versions": versions(env, opal),
        "runners": ran,
        "fixtures": len(fixtures),
    }
    if opal:
        meta["opal_binary"] = {"path": opal,
                               "sha256": hashlib.sha256(Path(opal).read_bytes()).hexdigest()}
    print(json.dumps(meta, indent=2), flush=True)

    observed = {f.id: {} for f in fixtures}
    unstable = {}
    invocations = {}
    started = time.perf_counter()
    try:
        for runner in ran:
            mine = [f for f in fixtures if f.runs_on(runner)]
            for attempt in range(args.repeat):
                root = scratch / f"{runner}-{attempt}"
                lay_out(root, [(f"corpus/{f.rel}", f.files()) for f in mine])
                paths = [f"corpus/{f.rel}/{t}" for f in mine for t in f.tests]
                label = runner if args.repeat == 1 else f"{runner} (run {attempt + 1})"
                results, invocation = run_runner(runner, opal, root, paths, env, label)
                invocations.setdefault(runner, []).append(invocation)
                for fixture in mine:
                    folder = root / "corpus" / fixture.rel
                    seen = {"results": {t: results[f"corpus/{fixture.rel}/{t}"]
                                        for t in fixture.tests},
                            "writes": writes(fixture, folder, runner)}
                    comparable = {"results": {t: {k: v for k, v in r.items()
                                                  if k not in ("message", "failures")}
                                              for t, r in seen["results"].items()},
                                  "writes": seen["writes"]}
                    if attempt == 0:
                        observed[fixture.id][runner] = seen
                        observed[fixture.id][runner]["_comparable"] = comparable
                    elif comparable != observed[fixture.id][runner]["_comparable"]:
                        unstable.setdefault(fixture.id, set()).add(runner)

        planted, unplanted, mutant_report, misses = [], [], {}, {}
        if not args.no_mutants:
            mutants = make_mutants(fixtures, scratch)
            print(f"  mutants: {sum(len(m) for m in mutants.values())} candidates "
                  f"in {len(mutants)} of {len(fixtures)} fixtures", flush=True)
            mutant_runs = {}
            for runner in ran:
                mine = [f for f in fixtures if f.runs_on(runner) and f.id in mutants]
                if not mine:
                    continue
                root = scratch / f"{runner}-mutants"
                lay_out(root, [(mutant_folder(f, i), m["files"])
                               for f in mine for i, m in enumerate(mutants[f.id])])
                paths = [f"{mutant_folder(f, i)}/{m['test']}"
                         for f in mine for i, m in enumerate(mutants[f.id])]
                results, invocation = run_runner(runner, opal, root, paths, env,
                                                 f"{runner} mutants")
                invocations.setdefault(f"{runner} mutants", []).append(invocation)
                mutant_runs[runner] = results
            planted, unplanted, mutant_report, misses = judge_mutants(
                fixtures, mutants, observed, mutant_runs, ran)

        checked = {}
        for fixture in fixtures:
            problems, found = check(fixture, observed[fixture.id], ran,
                                    misses.get(fixture.id, {}))
            for runner in sorted(unstable.get(fixture.id, ())):
                problems.append(f"{runner} gave different results on repeated runs")
            checked[fixture.id] = {"fixture": fixture, "problems": problems, "found": found}
    finally:
        if args.keep:
            print(f"\nscratch kept: {scratch}")
        else:
            shutil.rmtree(scratch, ignore_errors=True)
    meta["seconds"] = round(time.perf_counter() - started, 1)
    meta["invocations"] = invocations

    moved = []
    if args.reclassify:
        for entry in checked.values():
            if entry["problems"]:
                fixture = entry["fixture"]
                cls = reclassify(fixture, entry["found"])
                moved.append(f"{fixture.id}: now {cls}")

    results_dir = Path(args.results)
    results_dir.mkdir(parents=True, exist_ok=True)
    out = results_dir / f"test-runners-{datetime.now():%Y%m%d-%H%M%S}.json"
    record = {
        "meta": meta,
        "fixtures": {fid: {"class": c["fixture"].cls,
                           "compared": sorted(r for r, d in c["found"].items()),
                           "differs": sorted(r for r, d in c["found"].items() if d),
                           "problems": c["problems"]}
                     for fid, c in checked.items()},
        "mutants": {"planted": planted, "unplanted": unplanted, "by_runner": mutant_report},
    }
    if args.full:
        for fid, c in checked.items():
            record["fixtures"][fid]["differences"] = c["found"]
            record["fixtures"][fid]["observed"] = {
                r: {"results": o["results"], "writes": o["writes"]}
                for r, o in observed[fid].items()}
    text = json.dumps(record, indent=2, ensure_ascii=False) + "\n"
    out.write_text(text.replace(str(scratch), "<scratch>").replace(str(ORACLE), "<oracle>"))

    print()
    failures = 0
    for fid, entry in checked.items():
        for problem in entry["problems"]:
            print(f"FAIL {entry['fixture'].rel}: {problem}")
            failures += 1
    for line in moved:
        print(f"moved {line}")

    print("\n" + table(checked, mutant_report, ran, meta) + "\n")
    if not args.no_mutants:
        print(f"mutants: {len(planted)} files with a planted failure; "
              f"{len(unplanted)} without a candidate the reference accepted")
    print(f"{failures} failures ({meta['seconds']} s)")
    print(f"raw results: {out}")
    return 1 if failures else 0


def table(checked, mutant_report, ran, meta):
    lines = [
        f"Test runners: jest {meta['versions']['jest']}, vitest {meta['versions']['vitest']}, "
        f"bun {meta['versions']['bun']}, node {meta['versions']['node']}"
        + (f", opal {meta['versions']['opal']}" if "opal" in meta["versions"] else "")
        + f", {meta['date']}",
        "",
        "| runner | agrees | known_difference, as pinned | planted failures failed |",
        "|---|---|---|---|",
    ]
    for runner in RUNNERS:
        if runner not in ran:
            lines.append(f"| {runner} | not run | not run | not run |")
            continue
        cells = []
        for cls in CLASSES:
            mine = [c for c in checked.values() if c["fixture"].cls == cls
                    and c["fixture"].runs_on(runner)]
            reference = [c for c in mine if c["fixture"].reference == runner]
            compared = [c for c in mine if c["fixture"].reference != runner]
            ok = [c for c in compared if not any(p.startswith(runner) for p in c["problems"])]
            cell = f"{len(ok)} of {len(compared)}"
            if reference:
                cell += f" (+{len(reference)} as reference)"
            cells.append(cell)
        mutants = mutant_report.get(runner)
        cells.append(f"{mutants['caught']} of {mutants['planted']}"
                     if mutants and mutants["planted"] else "-")
        lines.append(f"| {runner} | " + " | ".join(cells) + " |")
    return "\n".join(lines)


if __name__ == "__main__":
    sys.exit(main())
