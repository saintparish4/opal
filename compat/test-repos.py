#!/usr/bin/env python3
"""Real projects' own test suites, run by their own runner and by opal test,
compared test by test.

    python3 compat/test-repos.py --opal PATH [REPO...]

Each project is pinned to a commit and installed the way its CI installs it,
under target/repos/ (cloned on first use, so the first run needs the network).
Then, for each:

1. its own runner runs the suite as its CI runs it, writing Jest's JSON report;
2. opal test runs it with the same environment and flags;
3. every test's status is compared, by file and full name;
4. `opal test -u` runs, and has to leave the checkout as it found it, which
   holds only if every snapshot the project has committed, in files or inline,
   is written back byte for byte. The checkout is restored afterwards.

A test that fails under both runners agrees: that's the machine, not the
runner (es-toolkit's `exec` test expects `ENOENT` from spawning a missing
command, and WSL's PATH gives `EACCES`). Any other difference has to be
declared below with its cause, as a pin is. A declared difference that no
longer happens fails the run too, so the list stays true.

This runs each project's own code, so only add projects you trust.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
REPOS_DIR = ROOT / "target" / "repos"

# A function's source text, as Function.prototype.toString gives it, is the
# transformer's printing: tsc (ts-jest) prints `() => { }`, Vite prints esbuild's
# output with its module runner's edits, opal prints oxc's. A test that puts a
# function in its name (`it.each` with `%s`) is named differently, and a
# snapshot keyed by that name isn't found.
FUNCTION_TEXT = "a function's source text is opal's transform's printing, not the project's transformer's"

REPOS = {
    "luxon": {
        "url": "https://github.com/moment/luxon.git",
        "commit": "f427515a38f6a671f8de663e6bcc040ed81f114e",
        "runner": "Jest 29.4.3 with babel-jest",
        "install": ["npm", "ci", "--no-audit", "--no-fund"],
        # .github/workflows/test.yml
        "env": {"TZ": "America/New_York", "LANG": "en_US.utf8"},
        "reference": ["npx", "jest", "--ci", "--json", "--outputFile={out}"],
        "opal": ["--ci"],
    },
    "jest-extended": {
        "url": "https://github.com/jest-community/jest-extended.git",
        "commit": "06939e8abd223614e293b708413fac487b6ce50a",
        "runner": "Jest 30.5.2 with ts-jest",
        "install": ["corepack", "yarn", "install", "--immutable"],
        "env": {},
        # package.json's `test` script colors on purpose: its snapshots hold
        # matcher messages with their colors.
        "reference": ["corepack", "yarn", "jest", "--color=true", "--ci", "--json", "--outputFile={out}"],
        "opal": ["--color=true", "--ci"],
        "renamed": [("() => { }", "() => {}", FUNCTION_TEXT)],
        "failing": {
            ("test/matchers/toBeArrayOfSize.test.ts", ".toBeArrayOfSize fails when given type of () => {} which is not an array"): FUNCTION_TEXT,
            ("test/matchers/toBeExtensible.test.ts", ".not.toBeExtensible fails when given an extensible object: () => {}"): FUNCTION_TEXT,
            ("test/matchers/toBeObject.test.ts", ".toBeObject fails when not given an object: () => {}"): FUNCTION_TEXT,
        },
        # The same three snapshot keys, renamed by `-u`.
        "round_trip": {
            "test/matchers/__snapshots__/toBeArrayOfSize.test.ts.snap": FUNCTION_TEXT,
            "test/matchers/__snapshots__/toBeExtensible.test.ts.snap": FUNCTION_TEXT,
            "test/matchers/__snapshots__/toBeObject.test.ts.snap": FUNCTION_TEXT,
        },
    },
    "es-toolkit": {
        "url": "https://github.com/toss/es-toolkit.git",
        "commit": "d79f9bb6ce4f4014d9f2a06b5882ed8b93f65e46",
        "runner": "Vitest 4.1.10",
        # CI installs with Yarn Plug'n'Play, which opal doesn't read; the
        # node-modules linker installs the same versions.
        "install": ["corepack", "yarn", "install"],
        "env": {"YARN_NODE_LINKER": "node-modules"},
        # CI's `yarn vitest`. Its config's `exclude` replaces Vitest's default,
        # and a PnP install has no node_modules to find test files in; with
        # node-modules, Vitest has to be told to leave them out.
        "reference": ["node_modules/.bin/vitest", "run", "--exclude", "**/node_modules/**",
                      "--reporter=json", "--outputFile={out}"],
        "opal": ["--ci"],
        "failing": {
            ("src/util/serialize/serializeFunction.spec.ts",
             "serializeFunction should collapse newlines and surrounding whitespace in the source"): FUNCTION_TEXT,
            ("src/error/TimeoutError.spec.ts", "TimeoutError is an instance of Error"):
                "isolation: in a file's vm context DOMException is the host's, not an instance of the context's Error (as under Jest); Vitest runs files in the worker's own realm",
            ("src/error/AbortError.spec.ts", "AbortError is an instance of Error"):
                "isolation: as TimeoutError",
            ("src/object/cloneDeep.dom.spec.ts", "cloneDeep should not clone uncloneable objects like `HTMLElement`s"):
                "asks for `@vitest-environment happy-dom`; DOM environments are out of opal test's scope",
            ("src/predicate/isBrowser.spec.ts", "isBrowser should return true in browser environment"):
                "asks for `@vitest-environment happy-dom`, as above",
        },
        # Measures 32 ms of wall time with Date; a clock correction can make it
        # fail under either runner.
        "flaky": {("src/compat/util/now.spec.ts",
                   "now should return the number of milliseconds that have elapsed since the Unix epoch")},
    },
}


def run(cmd, cwd, env, out=None, timeout=3600):
    started = time.perf_counter()
    proc = subprocess.run([part.replace("{out}", str(out)) for part in cmd], cwd=cwd, env=env,
                          capture_output=True, text=True, timeout=timeout)
    return proc, time.perf_counter() - started


def checkout(name, spec, env):
    repo = REPOS_DIR / name
    if not (repo / ".git").exists():
        repo.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
        subprocess.run(["git", "remote", "add", "origin", spec["url"]], cwd=repo, check=True)
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=repo, capture_output=True, text=True).stdout.strip()
    if head != spec["commit"]:
        subprocess.run(["git", "fetch", "-q", "--depth", "1", "origin", spec["commit"]], cwd=repo, check=True)
        subprocess.run(["git", "checkout", "-q", "FETCH_HEAD"], cwd=repo, check=True)
        shutil.rmtree(repo / "node_modules", ignore_errors=True)
    if not (repo / "node_modules").is_dir():
        proc, _ = run(spec["install"], repo, env)
        if proc.returncode != 0:
            sys.exit(f"{name}: install failed\n{proc.stdout[-2000:]}{proc.stderr[-2000:]}")
    return repo


def statuses(report, repo, rename=lambda title: title):
    """{(file, full name, occurrence): status}, with Vitest's `skipped` as
    Jest's `pending`. Tests can share a name in a file; the occurrence keeps
    them apart, in order."""
    out = {}
    for result in report["testResults"]:
        file = os.path.relpath(result["name"], repo)
        for test in result["assertionResults"]:
            name = rename(" ".join(test.get("ancestorTitles", []) + [test["title"]]))
            status = {"skipped": "pending"}.get(test["status"], test["status"])
            occurrence = 0
            while (file, name, occurrence) in out:
                occurrence += 1
            out[(file, name, occurrence)] = status
    return out


def compare(name, spec, repo, reference, opal):
    problems, notes = [], []
    renames = spec.get("renamed", [])

    def rename(title):
        for before, after, _ in renames:
            title = title.replace(before, after)
        return title

    ref = statuses(reference, repo, rename)
    mine = statuses(opal, repo)
    if renames:
        renamed = sum(1 for k in statuses(reference, repo) if rename(k[1]) != k[1])
        notes.append(f"{renamed} test names differ only as declared ({renames[0][2]})")
    failing = {(file, name, 0): cause for (file, name), cause in spec.get("failing", {}).items()}
    flaky = {(file, name, 0) for file, name in spec.get("flaky", set())}
    seen_declared = set()
    for key in sorted(set(ref) | set(mine)):
        a, b = ref.get(key), mine.get(key)
        if a == b or key in flaky:
            continue
        if a == "passed" and b == "failed" and key in failing:
            seen_declared.add(key)
            continue
        problems.append(f"{key[0]} | {key[1]}: {spec['runner'].split()[0]} {a}, opal {b}")
    for key in failing:
        if key not in seen_declared:
            problems.append(f"declared but not seen: {key[0]} | {key[1]}")
    return problems, notes, ref, mine


def round_trip(name, spec, repo, opal, env, ref, mine):
    """`opal test -u` must leave the checkout as it was, but for files the
    machine (a test failing under both runners) or a declared cause changes."""
    proc, _ = run([opal, "test", "-u", *[a for a in spec["opal"] if a != "--ci"]], repo, env)
    changed = [line[3:] for line in subprocess.run(["git", "status", "--porcelain"], cwd=repo,
                                                    capture_output=True, text=True).stdout.splitlines()]
    subprocess.run(["git", "checkout", "-q", "--", "."], cwd=repo, check=True)
    both_failed = {k[0] for k, v in ref.items() if v == "failed" and mine.get(k) == "failed"}
    allowed = spec.get("round_trip", {})
    problems = [f"`opal test -u` changed {path}" for path in changed
                if path not in allowed and path not in both_failed]
    return problems, changed


def counts(report):
    return {k: report.get(k) for k in ("numTotalTests", "numPassedTests", "numFailedTests", "numPendingTests")}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--opal", required=True)
    parser.add_argument("repos", nargs="*", help=f"some of: {', '.join(REPOS)}")
    args = parser.parse_args()
    opal = str(Path(args.opal).resolve())
    out_dir = ROOT / "target" / "repos" / "results"
    out_dir.mkdir(parents=True, exist_ok=True)
    record = {"date": datetime.now().isoformat(timespec="seconds"),
              "opal_sha256": subprocess.run(["sha256sum", opal], capture_output=True, text=True).stdout.split()[0],
              "node": subprocess.run(["node", "--version"], capture_output=True, text=True).stdout.strip(),
              "repos": {}}
    failures = 0
    rows = []
    for name in args.repos or list(REPOS):
        spec = REPOS[name]
        env = {**os.environ, "COREPACK_ENABLE_DOWNLOAD_PROMPT": "0", **spec["env"]}
        repo = checkout(name, spec, env)
        print(f"{name} @ {spec['commit'][:8]}", flush=True)
        ref_out, opal_out = out_dir / f"{name}-reference.json", out_dir / f"{name}-opal.json"
        for path in (ref_out, opal_out):
            path.unlink(missing_ok=True)
        _, ref_time = run(spec["reference"], repo, env, out=ref_out)
        _, opal_time = run([opal, "test", *spec["opal"], "--json", f"--outputFile={opal_out}"], repo, env)
        if not ref_out.exists() or not opal_out.exists():
            print(f"  no report from {'the reference' if not ref_out.exists() else 'opal'}")
            failures += 1
            continue
        reference, mine_report = json.loads(ref_out.read_text()), json.loads(opal_out.read_text())
        problems, notes, ref, mine = compare(name, spec, repo, reference, mine_report)
        trip_problems, changed = round_trip(name, spec, repo, opal, env, ref, mine)
        problems += trip_problems
        agree = sum(1 for k in set(ref) | set(mine) if ref.get(k) == mine.get(k))
        rows.append((name, spec["runner"], counts(reference), counts(mine_report), agree, len(set(ref) | set(mine)),
                     len(spec.get("failing", {})), ref_time, opal_time))
        for note in notes:
            print(f"  note: {note}")
        for problem in problems:
            print(f"  FAIL {problem}")
        failures += len(problems)
        record["repos"][name] = {"commit": spec["commit"], "reference": counts(reference), "opal": counts(mine_report),
                                 "agree": agree, "seconds": {"reference": round(ref_time, 2), "opal": round(opal_time, 2)},
                                 "round_trip_changed": changed, "problems": problems}
    print("\n| project | runner | its runner: passed / total | opal: passed / total | tests agreeing | declared differences | its runner | opal |")
    print("|---|---|---|---|---|---|---:|---:|")
    for name, runner, ref_c, opal_c, agree, total, declared, ref_time, opal_time in rows:
        print(f"| {name} | {runner} | {ref_c['numPassedTests']} / {ref_c['numTotalTests']} | "
              f"{opal_c['numPassedTests']} / {opal_c['numTotalTests']} | {agree} of {total} | {declared} | "
              f"{ref_time:.1f} s | {opal_time:.1f} s |")
    path = ROOT / "compat" / "results" / f"test-repos-{datetime.now():%Y%m%d-%H%M%S}.json"
    path.write_text(json.dumps(record, indent=1) + "\n")
    print(f"\n{failures} failures; wrote {path.relative_to(ROOT)}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
