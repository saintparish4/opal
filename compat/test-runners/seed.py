#!/usr/bin/env python3
"""Copy the test-runner oracle's fixtures out of Jest's and Vitest's own repositories.

    python3 compat/test-runners/seed.py [--src DIR]     # write corpus/, SOURCES.json
    python3 compat/test-runners/seed.py --check [--src DIR]

Every fixture under corpus/ comes from one of two tags, never from memory:
Jest's e2e fixture projects (e2e/<name>/) and Vitest's own core suite
(test/core/test/). The lists below are the selection, and the comment above
each says what was left out and why. The rule for Vitest is mechanical: a
file is taken only if it passes under the pinned Vitest with a plain config
when copied alone, imports nothing but `vitest`, its own packages and Node
built-ins, and stays inside what Jest also has. Jest's fixtures are taken
whatever their verdicts, since many of them fail on purpose.

A fixture is one test file plus whatever its directory holds besides other
test files: helper modules, `__mocks__`, committed snapshots. The changes made
on the way in are few, and SOURCES.json records each one per fixture:
  - a test file whose name no runner but Jest would pick up
    (`__tests__/index.js`) gets `.test` before its extension;
  - package.json, yarn.lock and babel.config.js are not copied, since the
    oracle runs every fixture under one config of its own;
  - each Vitest fixture gets `{"type": "module"}` as its package.json, which
    is what test/core/package.json says about the files.

A fixture lands in corpus/agrees/; compat/test-runners.py --reclassify moves
it to corpus/known_difference/ when the runners disagree on it. Re-seeding
writes each fixture back to wherever it already is, so the split survives.
--check verifies that every committed fixture still matches its upstream
bytes, renamed or not, and that nothing else was added to it.
"""

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = HERE / "corpus"
SOURCES = HERE / "SOURCES.json"
CLASSES = ("agrees", "known_difference")

UPSTREAM = {
    "jest": {"url": "https://github.com/jestjs/jest", "tag": "v30.5.2",
             "commit": "202dd8a14777c270607c2416bd5034c3c0abc984", "root": "e2e"},
    "vitest": {"url": "https://github.com/vitest-dev/vitest", "tag": "v4.1.12",
               "commit": "4766ce1e72ded629d3a37637f57eeae643789a8c", "root": "test/core/test"},
}

# Jest e2e fixture directories and the test files taken from each.
#
# Left out: anything that needs a config key beyond testEnvironment (custom
# resolvers, transforms, projects, reporters, setupFiles, globalSetup,
# moduleNameMapper, clearMocks and friends, haste), coverage, watch mode,
# jsdom, native ESM, fake timers, concurrent tests (out of scope for opal
# test), retries, randomize and seeds, console and stack-trace output (the
# oracle compares verdicts, not messages), the jasmine globals Jest 30 removed,
# and fixtures that need the Jest repository's own babel.config.js
# (expect-async-matcher, imported-globals, explicit-resource-management,
# failures/testMacro, which needs babel-plugin-macros, and the two
# failures/errorWithCause files, which are written with Flow annotations). The e2e tests that
# write their fixtures from strings at run time (to-match-snapshot and the
# like) are not imported yet. A file whose docblock asks for jsdom
# (@jest-environment jsdom) is out with the rest of jsdom.
JEST = {
    "before-all-skipped": ["__tests__/beforeAllFiltered.test.js"],
    "before-each-queue": ["__tests__/beforeEachQueue.test.js"],
    "bigint": ["__tests__/each.test.js", "__tests__/failing.test.js"],
    "call-done-twice": ["__tests__/index.test.js"],
    "circus-declaration-errors": ["__tests__/asyncDefinition.test.js"],
    "declaration-errors": ["__tests__/describeReturnPromise.test.js",
                           "__tests__/describeReturnSomething.test.js",
                           "__tests__/describeThrow.test.js"],
    "done-in-hooks": ["__tests__/index.test.js"],
    "each": ["__tests__/describeOnly.test.js", "__tests__/eachException.test.js",
             "__tests__/eachOnly.test.js", "__tests__/eachSkip.test.js",
             "__tests__/eachTemplate.test.js", "__tests__/failure.test.js",
             "__tests__/pretty.test.js", "__tests__/specialCharacterKeys.test.js",
             "__tests__/success.test.js"],
    "empty-describe-with-hooks": ["__tests__/hookInDescribeWithSkippedTest.test.js",
                                  "__tests__/hookInEmptyDescribe.test.js",
                                  "__tests__/hookInEmptyNestedDescribe.test.js",
                                  "__tests__/multipleHooksInEmptyDescribe.test.js"],
    "empty-suite-error": ["__tests__/emptySuite.test.js"],
    "failures": ["__tests__/aggregateError.test.js", "__tests__/assertionCount.test.js",
                 "__tests__/assertionError.test.js", "__tests__/assertionErrorExpand.test.js",
                 "__tests__/asyncFailures.test.js", "__tests__/duringTests.test.js",
                 "__tests__/errorAfterTestComplete.test.js", "__tests__/snapshot.test.js",
                 "__tests__/snapshotWithHint.test.js", "__tests__/throwNumber.test.js",
                 "__tests__/throwObject.test.js", "__tests__/throwObjectWithStackProp.test.js",
                 "__tests__/throwString.test.js"],
    "focused-tests": ["__tests__/tests.js"],
    "generator-mock": ["__tests__/generatorMock.test.js"],
    # asyncTestFails asks for jsdom in its docblock.
    "jasmine-async": ["__tests__/generator.test.js",
                      "__tests__/pendingInPromise.test.js", "__tests__/promiseAfterAll.test.js",
                      "__tests__/promiseAfterEach.test.js", "__tests__/promiseBeforeAll.test.js",
                      "__tests__/promiseBeforeEach.test.js", "__tests__/promiseFit.test.js",
                      "__tests__/promiseIt.test.js", "__tests__/promiseXit.test.js",
                      "__tests__/returningValues.test.js"],
    "lifecycles": ["__tests__/index.js"],
    "location-in-results": ["__tests__/test.js"],
    "mock-functions": ["__tests__/index.js"],
    "mock-json": ["__tests__/index.js"],
    **{f"mock-names/{name}": ["__tests__/index.js"] for name in (
        "with-empty-mock-name", "with-empty-mock-name-not-called", "with-mock-name",
        "with-mock-name-call-times-fail", "with-mock-name-call-times-pass",
        "with-mock-name-not-called", "with-mock-name-not-called-fail",
        "with-mock-name-not-called-pass", "without-mock-name", "without-mock-name-not-called")},
    # Configured for jsdom upstream; none of the four touches the DOM.
    "nested-test-definitions": ["__tests__/nestedDescribeInTest.js",
                                "__tests__/nestedHookInTest.js",
                                "__tests__/nestedTestOutsideDescribe.js",
                                "__tests__/nestedTestWithinDescribe.js"],
    "promise-and-callback": ["__tests__/promise-and-callback.test.js"],
    "snapshot": ["__tests__/secondSnapshot.test.js", "__tests__/snapshot.test.js"],
    "snapshot-crlf": ["__tests__/crlf.test.js"],
    "snapshot-escape": ["__tests__/snapshot.test.js", "__tests__/snapshotEscapeRegex.js",
                        "__tests__/snapshotEscapeSubstitution.test.js"],
    "snapshot-unknown": ["__tests__/works.test.js"],
    "test-failing": ["__tests__/statuses.test.js", "__tests__/worksWithOnlyMode.test.js",
                     "__tests__/worksWithSkipMode.test.js"],
    "test-failing-snapshot": ["__tests__/inlineSnapshot.test.js", "__tests__/snapshot.test.js"],
    "test-failing-snapshot-all-pass": ["__tests__/inlineSnapshot.test.js",
                                       "__tests__/snapshot.test.js"],
    "test-todo": ["__tests__/only-todo.test.js", "__tests__/statuses.test.js",
                  "__tests__/todoMultipleArgs.test.js", "__tests__/todoNoArgs.test.js",
                  "__tests__/todoNonString.test.js", "__tests__/verbose.test.js"],
}

# Vitest test/core/test files. 86 of the 165 pass when copied alone under a
# plain config; the rest need test/core's aliases, defines, virtual modules or
# workspace packages. Of the 86, left out: tests of Vitest's own internals
# (they import @vitest/runner, @vitest/utils, @vitest/mocker or vitest/node:
# diff, error, pretty-format, chainable, test-tags-filter, browserAutomocker,
# utils-display, replace-matcher, skip-reset-state, fixture-initialization,
# repeats, test-extend), Vitest-only APIs with no Jest counterpart (test.extend
# fixtures, test.for, retry and repeats options, onTestFailed/onTestFinished,
# runIf/skipIf, expect.poll, vi.waitFor, stubGlobal/stubEnv, test options
# objects, vi.setConfig hook sequencing, chai-style assertions), concurrent
# tests and fake timers (out of scope), files that only check the environment
# they run in (builtin, env-glob, import-client, url-ssr, node-protocol,
# child-specific, threads-specific, sourcemap, dynamic-import,
# write-file-dynamic-import, moved-snapshot, which needs a resolveSnapshotPath
# config), and two that add bulk, not cases (lot-of-tests, math).
#
# jest-expect.test.ts imports two helpers from @vitest/expect and
# @vitest/utils; it is kept anyway as Vitest's port of Jest's expect tests.
VITEST = [
    "each.test.ts", "execution-order.test.ts", "expect-circular.test.ts", "fn.test.ts",
    "hooks.test.js", "hooks.test.ts", "isolate.test.ts", "jest-expect-no-url.test.ts",
    "jest-expect.test.ts", "jest-matcher-utils.test.ts", "jest-mock.test.ts",
    "nested-only.test.ts", "nested-suite.test.ts", "nested-test.test.ts", "only.test.ts",
    "pattern.test.ts", "skip.test.ts", "snapshot-async.test.ts",
    "snapshot-custom-serializer.test.ts", "snapshot-inline-(parentheses).test.ts",
    "snapshot-inline.test.ts", "strict.test.js", "unhandled-skip.test.ts",
]

NOT_COPIED = {"package.json", "yarn.lock", "babel.config.js"}
VITEST_PACKAGE_JSON = '{\n  "type": "module"\n}\n'


def is_test_name(name):
    """Whether bun, vitest and jest all collect a file by this name alone."""
    stem, _, ext = name.rpartition(".")
    return ext in ("js", "cjs", "mjs", "ts", "cts", "mts") and stem.endswith((".test", ".spec"))


def renamed(rel):
    path = Path(rel)
    if is_test_name(path.name):
        return rel
    return str(path.with_name(f"{path.stem}.test{path.suffix}"))


def jest_test_files(project):
    """What Jest's default testMatch collects in an e2e project directory."""
    found = set()
    for path in project.rglob("*"):
        if not path.is_file() or "node_modules" in path.parts or "__snapshots__" in path.parts:
            continue
        rel = path.relative_to(project).as_posix()
        if ("__tests__" in path.parts and path.suffix in (".js", ".ts")) or is_test_name(path.name):
            found.add(rel)
    return found


def fixture_name(directory, test_file):
    stem = Path(test_file).name.split(".")[0]
    return f"{directory.replace('/', '-')}-{stem}"


def plan(src):
    """{fixture: {"upstream", "dialect", "files": {dest: source}, "changes": [...]}}"""
    fixtures = {}
    e2e = src["jest"] / UPSTREAM["jest"]["root"]
    for directory, tests in JEST.items():
        project = e2e / directory
        every_test = jest_test_files(project) | set(tests)
        for test in tests:
            others = every_test - {test}
            files, changes = {}, []
            for path in sorted(project.rglob("*")):
                rel = path.relative_to(project).as_posix()
                if not path.is_file() or path.name in NOT_COPIED or rel in others:
                    continue
                if "__snapshots__" in path.parts:
                    owner = path.name.removesuffix(".snap")
                    if any(Path(other).name == owner for other in others):
                        continue
                dest = renamed(rel) if rel == test else rel
                if dest != rel:
                    changes.append(f"renamed {rel} to {dest}")
                files[dest] = f"{UPSTREAM['jest']['root']}/{directory}/{rel}"
            if directory == "nested-test-definitions":
                changes.append("runs under testEnvironment node; upstream configures jsdom")
            fixtures[fixture_name(directory, test)] = {
                "upstream": "jest", "dialect": "jest", "files": files, "changes": changes}
    core = src["vitest"] / UPSTREAM["vitest"]["root"]
    for test in VITEST:
        files = {test: f"{UPSTREAM['vitest']['root']}/{test}"}
        snapshot = core / "__snapshots__" / f"{test}.snap"
        if snapshot.exists():
            files[f"__snapshots__/{test}.snap"] = f"{UPSTREAM['vitest']['root']}/__snapshots__/{test}.snap"
        # hooks.test.js and hooks.test.ts are two fixtures, so the extension stays.
        fixtures[test.replace(".test.", ".").replace(".spec.", ".")] = {
            "upstream": "vitest", "dialect": "vitest", "files": files,
            "changes": ["package.json is {\"type\": \"module\"}, as test/core/package.json declares"]}
    return fixtures


def checkout(name, into):
    upstream = UPSTREAM[name]
    target = into / name
    subprocess.run(["git", "clone", "--quiet", "--depth", "1", "--branch", upstream["tag"],
                    upstream["url"], str(target)], check=True,
                   stderr=subprocess.DEVNULL)
    return target


def verify_commit(name, path):
    head = subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"], capture_output=True,
                          text=True, check=True).stdout.strip()
    if head != UPSTREAM[name]["commit"]:
        sys.exit(f"seed: {path} is at {head}, not {name} {UPSTREAM[name]['tag']} "
                 f"({UPSTREAM[name]['commit']})")


def existing_location(name, dialect):
    for cls in CLASSES:
        path = CORPUS / cls / dialect / name
        if path.exists():
            return path
    return None


def digest(data):
    return hashlib.sha256(data).hexdigest()


def expected_files(fixture, src):
    files = {dest: (src[fixture["upstream"]] / source).read_bytes()
             for dest, source in fixture["files"].items()}
    if fixture["dialect"] == "vitest":
        files["package.json"] = VITEST_PACKAGE_JSON.encode()
    return files


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--src", help="a folder holding jest/ and vitest/ checkouts at the pinned "
                                      "tags (default: clone them into a temporary folder)")
    parser.add_argument("--check", action="store_true",
                        help="verify the committed corpus against upstream instead of writing it")
    args = parser.parse_args()

    scratch = None
    if args.src:
        root = Path(args.src).resolve()
    else:
        scratch = Path(tempfile.mkdtemp(prefix="opal-seed-"))
        root = scratch
        for name in UPSTREAM:
            checkout(name, root)
    try:
        src = {name: root / name for name in UPSTREAM}
        for name, path in src.items():
            verify_commit(name, path)
        fixtures = plan(src)
        problems = []
        for name, fixture in fixtures.items():
            files = expected_files(fixture, src)
            location = existing_location(name, fixture["dialect"])
            if args.check:
                if location is None:
                    problems.append(f"{name}: missing from corpus/")
                    continue
                present = {p.relative_to(location).as_posix(): p.read_bytes()
                           for p in location.rglob("*") if p.is_file()}
                present.pop("PIN.json", None)
                for dest, data in files.items():
                    if dest not in present:
                        problems.append(f"{name}: {dest} is missing")
                    elif present[dest] != data:
                        problems.append(f"{name}: {dest} differs from upstream")
                for extra in sorted(set(present) - set(files)):
                    problems.append(f"{name}: {extra} is not from upstream")
                continue
            target = location or CORPUS / "agrees" / fixture["dialect"] / name
            pin = (target / "PIN.json").read_bytes() if (target / "PIN.json").exists() else None
            if target.exists():
                shutil.rmtree(target)
            for dest, data in files.items():
                (target / dest).parent.mkdir(parents=True, exist_ok=True)
                (target / dest).write_bytes(data)
            if pin is not None:
                (target / "PIN.json").write_bytes(pin)
        if args.check:
            known = {(fixtures[name]["dialect"], name) for name in fixtures}
            for cls in CLASSES:
                for dialect_dir in sorted((CORPUS / cls).glob("*")):
                    for fixture_dir in sorted(p for p in dialect_dir.iterdir() if p.is_dir()):
                        if (dialect_dir.name, fixture_dir.name) not in known:
                            problems.append(f"{cls}/{dialect_dir.name}/{fixture_dir.name}: "
                                            f"not in seed.py")
            for problem in problems:
                print(problem)
            print(f"{len(fixtures)} fixtures checked against jest {UPSTREAM['jest']['tag']} and "
                  f"vitest {UPSTREAM['vitest']['tag']}: "
                  + ("all match" if not problems else f"{len(problems)} problems"))
            return 1 if problems else 0
        manifest = {
            "upstream": UPSTREAM,
            "fixtures": {name: {"upstream": f["upstream"],
                                "files": {dest: {"from": source,
                                                 "sha256": digest(expected_files(f, src)[dest])}
                                          for dest, source in f["files"].items()},
                                "changes": f["changes"]}
                         for name, f in sorted(fixtures.items())},
        }
        SOURCES.write_text(json.dumps(manifest, indent=2) + "\n")
        for name in UPSTREAM:
            licence = HERE / "LICENSES" / f"{name}.txt"
            licence.parent.mkdir(exist_ok=True)
            shutil.copyfile(src[name] / "LICENSE", licence)
        print(f"seeded {len(fixtures)} fixtures; provenance in {SOURCES.relative_to(HERE.parent.parent)}")
        return 0
    finally:
        if scratch is not None:
            shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
