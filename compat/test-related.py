#!/usr/bin/env python3
"""Random projects, asked under Jest and under opal test which tests a change
affects, compared on the answer.

    python3 compat/test-related.py --opal PATH [--seed N] [--projects N] [--queries N] [--keep]

Watch mode reruns the tests a change affects, and `--findRelatedTests` runs
them once. opal answers with a reverse-dependency query on opal-core's module
graph (crates/opal-test/src/related.rs); Jest answers with
jest-resolve-dependencies' resolveInverse over its haste map. This generates
projects that exercise what decides the answer, and asks both the same
questions:

- modules in nested folders, in every extension Jest reads, and directory
  indexes, imported with and without extensions;
- `import`, `require`, `export ... from`, `import()`, `jest.requireActual`,
  `jest.requireMock` and `jest.createMockFromModule`; type-only imports and
  commented-out ones, which are not edges; `jest.mock` calls, which are not
  edges either;
- `moduleNameMapper` aliases, root manual mocks, manual mocks beside the
  module, and a manual mock for a package;
- a package in node_modules, Node built-ins, imports that don't resolve,
  cycles, snapshot files, `roots` that leave files out and
  `modulePathIgnorePatterns`.

Each query is one to three files (or a file that doesn't exist), given to
`--findRelatedTests ... --listTests --json`. Then each project is committed to
a git repository, a few of its files are edited, staged or added, and both
runners answer `-o --listTests --json`, which relates the tests to what git
says has changed. The two runners have to list the same test files. Jest 30.5.2 is the reference, from its installed copy in
compat/test-runners/runners/. A mismatch prints the project's seed, the query
and both answers, and the run exits non-zero; `--keep` keeps the projects.
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

DIRS = ["src", "src/a", "src/a/b", "src/c", "lib", "shared"]
MODULE_EXTENSIONS = ["js", "js", "js", "ts", "ts", "jsx", "tsx", "mjs", "cjs"]


def relative_specifier(src, dst, rng):
    """How `src` might import `dst`: relative, with or without extension, or
    through its folder when it is an index."""
    rel = os.path.relpath(dst, os.path.dirname(src)).replace(os.sep, "/")
    if not rel.startswith("."):
        rel = "./" + rel
    stem, ext = os.path.splitext(rel)
    if ext == ".json":
        return rel
    if stem.endswith("/index") and rng.random() < 0.5:
        return stem[: -len("/index")] or "."
    # .mjs and .cjs keep theirs: Jest tries moduleFileExtensions in order, so an
    # extensionless import would find a .js sibling first.
    if ext in (".mjs", ".cjs") or rng.random() < 0.4:
        return rel
    return stem


def generate(seed):
    """{relative path: text} for one project, and its jest.config.js object."""
    rng = random.Random(seed)
    files = {}
    config = {"testEnvironment": "node"}
    roots_src = rng.random() < 0.25
    if roots_src:
        config["roots"] = ["<rootDir>/src"]
    alias = rng.random() < 0.5
    if alias:
        config["moduleNameMapper"] = {"^@/(.*)$": "<rootDir>/src/$1", "^~shared$": "<rootDir>/shared/s0.js"}
    ignored_dir = rng.random() < 0.2
    if ignored_dir:
        config["modulePathIgnorePatterns"] = ["<rootDir>/src/c/"]

    modules = []
    for i in range(rng.randint(6, 22)):
        directory = rng.choice(DIRS)
        if rng.random() < 0.15:
            path = f"{directory}/m{i}/index.{rng.choice(['js', 'ts'])}"
        else:
            path = f"{directory}/m{i}.{rng.choice(MODULE_EXTENSIONS)}"
        modules.append(path)
    modules.append("shared/s0.js")
    data = [f"{rng.choice(DIRS)}/d{i}.json" for i in range(rng.randint(0, 2))]
    tests = []
    for i in range(rng.randint(3, 9)):
        style = rng.random()
        directory = rng.choice(["src", "src/a", "src/a/b", "src/c", "lib"])
        if style < 0.4:
            tests.append(f"{directory}/t{i}.test.{rng.choice(['js', 'ts', 'tsx'])}")
        elif style < 0.7:
            tests.append(f"{directory}/__tests__/t{i}.{rng.choice(['js', 'ts'])}")
        else:
            tests.append(f"{directory}/t{i}.spec.js")

    # Manual mocks: some root ones named like a module, some beside a module,
    # and one for the package.
    mocks = []
    for module in rng.sample(modules, k=min(len(modules), rng.randint(0, 3))):
        name = os.path.splitext(os.path.basename(module))[0]
        if name == "index":
            continue
        if rng.random() < 0.5:
            mocks.append(f"__mocks__/{name}.js")
        else:
            mocks.append(f"{os.path.dirname(module)}/__mocks__/{name}.js")
    if rng.random() < 0.4:
        mocks.append("__mocks__/pkg.js")
    mocks = sorted(set(mocks))

    importable = modules + data

    def imports_for(path, count, is_test):
        """Import statements for `path`, valid for its extension."""
        ext = os.path.splitext(path)[1]
        lines = []
        for _ in range(count):
            roll = rng.random()
            if roll < 0.08:
                spec = "pkg"
            elif roll < 0.12:
                spec = rng.choice(["fs", "node:path"])
            elif roll < 0.16:
                spec = "./missing-" + str(rng.randint(0, 3))
            elif roll < 0.24 and alias:
                target = rng.choice(modules)
                if target.startswith("src/"):
                    spec = "@/" + os.path.splitext(target[len("src/"):])[0]
                else:
                    spec = "~shared"
            else:
                spec = relative_specifier(path, rng.choice(importable), rng)
            form = rng.random()
            name = f"v{len(lines)}"
            if ext == ".cjs":
                lines.append(f"const {name} = require({json.dumps(spec)});")
            elif ext == ".mjs":
                if form < 0.6:
                    lines.append(f"import {name} from {json.dumps(spec)};")
                elif form < 0.8:
                    lines.append(f"export * from {json.dumps(spec)};")
                else:
                    lines.append(f"export const {name} = () => import({json.dumps(spec)});")
            else:
                if form < 0.3:
                    lines.append(f"import {name} from {json.dumps(spec)};")
                elif form < 0.5:
                    lines.append(f"const {name} = require({json.dumps(spec)});")
                elif form < 0.6:
                    lines.append(f"export {{ default as {name} }} from {json.dumps(spec)};")
                elif form < 0.68:
                    lines.append(f"export const {name} = () => import({json.dumps(spec)});")
                elif form < 0.76 and is_test:
                    fn = rng.choice(["requireActual", "requireMock", "createMockFromModule"])
                    lines.append(f"const {name} = jest.{fn}({json.dumps(spec)});")
                elif form < 0.82 and is_test:
                    # Not a dependency for either runner.
                    lines.append(f"jest.mock({json.dumps(spec)});")
                elif form < 0.88 and ext in (".ts", ".tsx"):
                    lines.append(f"import type {{ T{len(lines)} }} from {json.dumps(spec)};")
                elif form < 0.94:
                    lines.append(f"// import {name} from {json.dumps(spec)};")
                else:
                    lines.append(f"/* require({json.dumps(spec)}) */")
        return lines

    for path in modules:
        lines = imports_for(path, rng.randint(0, 3), False)
        ext = os.path.splitext(path)[1]
        lines.append("module.exports = 1;" if ext == ".cjs" else "export default 1;")
        files[path] = "\n".join(lines) + "\n"
    for path in data:
        files[path] = '{"value": 1}\n'
    for path in mocks:
        files[path] = "\n".join(imports_for(path, rng.randint(0, 2), False) + ["module.exports = 1;"]) + "\n"
    for path in tests:
        lines = imports_for(path, rng.randint(1, 4), True)
        lines.append("test('t', () => {});")
        files[path] = "\n".join(lines) + "\n"
        if rng.random() < 0.3:
            directory, name = os.path.split(path)
            files[f"{directory}/__snapshots__/{name}.snap"] = "// Jest Snapshot v1\n"
    files["node_modules/pkg/package.json"] = '{"name": "pkg", "main": "index.js"}\n'
    files["node_modules/pkg/index.js"] = "module.exports = require('./inner');\n"
    files["node_modules/pkg/inner.js"] = "module.exports = 1;\n"
    files["package.json"] = '{"name": "project"}\n'
    files["jest.config.js"] = "module.exports = " + json.dumps(config, indent=2) + ";\n"
    return files


def queries(seed, files, count):
    """Lists of one to three paths to ask about."""
    rng = random.Random(seed * 7919 + 1)
    candidates = sorted(p for p in files if p not in ("package.json", "jest.config.js"))
    out = []
    for _ in range(count):
        query = rng.sample(candidates, k=min(len(candidates), rng.randint(1, 3)))
        if rng.random() < 0.1:
            query.append("src/does-not-exist.js")
        out.append(query)
    return out


def git(project, *args):
    subprocess.run(["git", "-c", "user.email=o@example.com", "-c", "user.name=o", *args],
                   cwd=project, check=True, capture_output=True)


def change_some(seed, project, files):
    """Edits, stages and adds files after the commit, as a working copy would."""
    rng = random.Random(seed * 104_729 + 3)
    candidates = sorted(p for p in files if p not in ("package.json", "jest.config.js"))
    for rel in rng.sample(candidates, k=min(len(candidates), rng.randint(1, 3))):
        with open(project / rel, "a") as f:
            f.write("\n" if rel.endswith(".json") or rel.endswith(".snap") else "\n// changed\n")
        if rng.random() < 0.4:
            git(project, "add", rel)
    if rng.random() < 0.4:
        target = rng.choice([p for p in candidates if p.endswith(".js") and "node_modules" not in p] or ["src/x.js"])
        new = f"src/new{seed % 7}.test.js" if rng.random() < 0.5 else f"lib/new{seed % 7}.js"
        spec = relative_specifier(new, target, rng)
        (project / new).parent.mkdir(parents=True, exist_ok=True)
        (project / new).write_text(f"require({json.dumps(spec)});\ntest('n', () => {{}});\n")


def ask(cmd, cwd, out_file, env):
    if out_file.exists():
        out_file.unlink()
    proc = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=300)
    if not out_file.exists():
        return None, (proc.stdout + proc.stderr)[-2000:]
    try:
        return sorted(set(json.loads(out_file.read_text() or "[]"))), None
    except json.JSONDecodeError as error:
        return None, f"unreadable output: {error}"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--opal", required=True)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--projects", type=int, default=40)
    parser.add_argument("--queries", type=int, default=5)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()

    opal = str(Path(args.opal).resolve())
    scratch = Path(tempfile.mkdtemp(prefix="opal-related-"))
    env = {**os.environ, "FORCE_COLOR": "0", "CI": "1"}
    mismatches = 0
    asked = 0
    try:
        for index in range(args.projects):
            seed = args.seed * 100_003 + index
            files = generate(seed)
            project = scratch / f"p{seed}"
            for rel, text in files.items():
                path = project / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            out = project.parent / f"p{seed}.json"
            for query in queries(seed, files, args.queries):
                asked += 1
                expected, jest_error = ask(
                    ["node", str(JEST), "--ci", "--cacheDirectory", str(project.parent / "jest-cache"),
                     "--findRelatedTests", *query, "--listTests", "--json", "--outputFile", str(out)],
                    project, out, env)
                actual, opal_error = ask(
                    [opal, "test", "--findRelatedTests", *query, "--listTests", "--json", "--outputFile", str(out)],
                    project, out, env)
                if expected is None:
                    print(f"project {seed}, query {query}: jest gave no answer\n{jest_error}")
                    mismatches += 1
                    continue
                if expected != actual:
                    mismatches += 1
                    strip = lambda paths: [os.path.relpath(p, project) for p in paths or []]
                    print(f"\nproject {seed} ({project}), query {query}:")
                    print(f"  jest: {strip(expected)}")
                    print(f"  opal: {strip(actual) if actual is not None else opal_error}")

            # -o: the same question, about what git says has changed.
            git(project, "init", "-q")
            git(project, "add", "-A")
            git(project, "commit", "-q", "-m", "start")
            change_some(seed, project, files)
            asked += 1
            expected, jest_error = ask(
                ["node", str(JEST), "--ci", "--cacheDirectory", str(project.parent / "jest-cache"),
                 "-o", "--listTests", "--json", "--outputFile", str(out)],
                project, out, env)
            actual, opal_error = ask([opal, "test", "-o", "--listTests", "--json", "--outputFile", str(out)],
                                     project, out, env)
            if expected is None or expected != actual:
                mismatches += 1
                status = subprocess.run(["git", "status", "--short"], cwd=project, capture_output=True, text=True).stdout
                strip = lambda paths: [os.path.relpath(p, project) for p in paths or []]
                print(f"\nproject {seed} ({project}), -o after:\n{status}")
                print(f"  jest: {strip(expected) if expected is not None else jest_error}")
                print(f"  opal: {strip(actual) if actual is not None else opal_error}")
    finally:
        if args.keep:
            print(f"projects kept in {scratch}")
        else:
            shutil.rmtree(scratch, ignore_errors=True)
    print(f"\n{args.projects} projects, {asked} queries: {asked - mismatches} identical, "
          f"{mismatches} differ (seed {args.seed})")
    sys.exit(1 if mismatches else 0)


if __name__ == "__main__":
    main()
