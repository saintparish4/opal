#!/usr/bin/env python3
"""Twenty app-shaped projects, each run by its own runner and by opal test,
to count what breaks under opal and why.

    python3 compat/survey-repos.py --opal PATH [--install-only] [REPO...]

compat/test-repos.py holds three projects to exact agreement. This is the
step before that for the next twenty: a measurement, not a gate. Each project
is pinned to a commit, cloned under target/survey/repos/ and installed the way
its CI installs it. Then:

1. its own runner runs its unit tests as its CI does, and writes Jest's JSON
   report (Vitest's json reporter writes the same shape);
2. opal test runs the command CI runs, unchanged ("as is"): does swapping
   opal in start at all;
3. opal test runs again unit by unit: a monorepo's packages one at a time, and
   a Jest config with `projects` split into one config per project, as the
   project's own Jest expands them (`jest --showConfig`). That shows what else
   breaks behind a missing feature that stops the run as is;
4. every test the reference passed and opal didn't is put down to a cause,
   read off opal's own failure: the first line of the test's message, or of
   the file's, matched against CAUSES.

The result, per project and per cause, is printed as tables and written to
compat/results/survey-repos-*.json, with each run's output under
target/survey/logs/. The causes rank the work that gets opal test to a
working state: a project whose CI command, with opal swapped in, passes what
its own runner passes.

This installs and runs each project's own code, so only add projects you
trust. End-to-end suites (Playwright, Cypress), suites that need a database,
and type-only test scripts are left out.
"""

import argparse
import importlib.util
import json
import os
import re
import subprocess
import sys
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
SURVEY = ROOT / "target" / "survey"
REPOS_DIR = SURVEY / "repos"
LOGS = SURVEY / "logs"

_spec = importlib.util.spec_from_file_location("test_repos", HERE / "test-repos.py")
test_repos = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(test_repos)

# Installs run lifecycle scripts, as CI's do, but none that download browsers
# or set up git hooks.
INSTALL_ENV = {
    "HUSKY": "0",
    "CI": "true",
    "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD": "1",
    "CYPRESS_INSTALL_BINARY": "0",
    "PUPPETEER_SKIP_DOWNLOAD": "1",
    "npm_config_audit": "false",
    "npm_config_fund": "false",
}

PNPM = ["corepack", "pnpm", "install", "--frozen-lockfile"]
JEST = "{root}/node_modules/.bin/jest"
VITEST = "{root}/node_modules/.bin/vitest"
JEST_JSON = ["--ci", "--json", "--outputFile={out}"]
VITEST_JSON = ["run", "--reporter=json", "--outputFile={out}"]


def unit(cwd=".", args=(), env=None):
    return {"cwd": cwd, "args": list(args), "env": env or {}}


def reference(cmd, cwd=".", env=None):
    return {"cwd": cwd, "cmd": cmd, "env": env or {}}


# Each project:
#   url, commit, install (from `dir`, default the checkout), prepare (commands
#   run once after installing), env (for every run);
#   reference: the runs of its own runner, each writing a JSON report to {out};
#   as_is: opal's arguments for the command CI runs;
#   opal: opal's runs unit by unit, or {"jest_projects": ...} to split a Jest
#   config's projects through the project's own Jest.
REPOS = {
    # Browser and React apps on Jest
    "ant-design": {
        "url": "https://github.com/ant-design/ant-design.git",
        "commit": "33530b68fe0f5c6753232a8186704258fa4259f8",
        "runner": "Jest 30.5.2, its own transformer, jsdom",
        "lockfile": False,
        "install": ["npm", "install"],
        "prepare": [["npm", "run", "version"]],
        # CI's `--no-cache`: with Jest's shared cache, a stale transform of
        # @ant-design/cssinjs-utils fails every file.
        "reference": [reference([JEST, "--config", ".jest.js", "--no-cache", *JEST_JSON])],
        "as_is": unit(args=["--config", ".jest.js", "--no-cache"]),
        "opal": [unit(args=["--config", ".jest.js"])],
    },
    "react-hook-form": {
        "url": "https://github.com/react-hook-form/react-hook-form.git",
        "commit": "06bd82b45f67de433ee8f622f21dc445e25773e5",
        "runner": "Jest 30, @swc/jest, jest-fixed-jsdom, projects",
        "install": PNPM,
        "reference": [reference([JEST, "--config", "./scripts/jest/jest.config.js", *JEST_JSON])],
        "as_is": unit(args=["--config", "./scripts/jest/jest.config.js"]),
        "opal": {"jest_projects": {"config": "./scripts/jest/jest.config.js"}},
    },
    "swr": {
        "url": "https://github.com/vercel/swr.git",
        "commit": "8148ef012fd5b317d1e8ec2da4b2ca0673f63c0f",
        "runner": "Jest 30, @swc/jest, jsdom",
        "install": PNPM,
        "reference": [reference([JEST, *JEST_JSON])],
        "as_is": unit(),
        "opal": [unit()],
    },
    "downshift": {
        "url": "https://github.com/downshift-js/downshift.git",
        "commit": "1bb8b75e506fe807a5c5201a103d1bd128e5a5e2",
        "runner": "Jest via kcd-scripts, babel-jest, jsdom",
        "lockfile": False,
        "install": ["npm", "install"],
        # kcd-scripts test sets these before it runs Jest.
        "env": {"BABEL_ENV": "test", "NODE_ENV": "test"},
        "reference": [reference([JEST, *JEST_JSON])],
        "as_is": unit(),
        "opal": [unit()],
    },
    "dom-testing-library": {
        "url": "https://github.com/testing-library/dom-testing-library.git",
        "commit": "6049cc0bc7cf2201625c476c95fa6299d0e2fa8b",
        "runner": "Jest via kcd-scripts, projects (jsdom and node)",
        "lockfile": False,
        "install": ["npm", "install"],
        "env": {"BABEL_ENV": "test", "NODE_ENV": "test"},
        "reference": [reference([JEST, *JEST_JSON])],
        "as_is": unit(),
        "opal": {"jest_projects": {"config": "jest.config.js"}},
    },
    "apollo-client": {
        "url": "https://github.com/apollographql/apollo-client.git",
        "commit": "0fc31b82e53008404178e8bb8b52bbfc0a315062",
        "runner": "Jest 30, ts-jest, a jsdom environment of its own, projects",
        "install": ["npm", "ci"],
        "reference": [reference(["node", "--expose-gc", "--experimental-import-meta-resolve",
                                 "--disable-warning=ExperimentalWarning", "{root}/node_modules/jest/bin/jest.js",
                                 "--config", "./config/jest.config.ts", *JEST_JSON])],
        # CI's `npm run test:ci`.
        "as_is": unit(args=["--config", "./config/jest.config.ts", "--reporters=default", "--reporters=jest-junit",
                            "--logHeapUsage", "--watchAll=false", "--maxWorkers=2"],
                      env={"NODE_OPTIONS": "--expose-gc", "TEST_ENV": "ci"}),
        "opal": {"jest_projects": {"config": "./config/jest.config.ts",
                                   "node": ["--experimental-import-meta-resolve"]},
                 "env": {"NODE_OPTIONS": "--expose-gc"}},
    },
    "mobx": {
        "url": "https://github.com/mobxjs/mobx.git",
        "commit": "81bde398c579239a7a5ffb5d8efbf21a5ea9cf83",
        "runner": "Jest 30, ts-jest js-with-ts, jsdom, projects",
        # Its postinstall installs the website's dependencies; the tests
        # don't need them.
        "install": ["npm", "ci", "--ignore-scripts"],
        "reference": [reference([JEST, *JEST_JSON])],
        "as_is": unit(),
        "opal": {"jest_projects": {"config": "jest.config.js"}},
    },
    "motion": {
        "url": "https://github.com/motiondivision/motion.git",
        "commit": "5881b1bb0f96e00d1e06bd33a121d01f50458d22",
        "runner": "Jest 29, ts-jest, jsdom, a config per package",
        "install": ["corepack", "yarn", "install", "--immutable"],
        "reference": [
            reference([JEST, "--config", "jest.config.json", *JEST_JSON], cwd="packages/framer-motion"),
            reference([JEST, "--config", "jest.config.ssr.json", *JEST_JSON], cwd="packages/framer-motion"),
            reference([JEST, "--config", "jest.config.json", *JEST_JSON], cwd="packages/motion-dom"),
            reference([JEST, "--config", "jest.config.json", *JEST_JSON], cwd="packages/motion-utils"),
            reference([JEST, "--config", "jest.config.json", *JEST_JSON], cwd="packages/motion"),
        ],
        "as_is": unit(cwd="packages/framer-motion", args=["--config", "jest.config.json", "--max-workers=4"]),
        "opal": [
            unit(cwd="packages/framer-motion", args=["--config", "jest.config.json"]),
            unit(cwd="packages/framer-motion", args=["--config", "jest.config.ssr.json"]),
            unit(cwd="packages/motion-dom", args=["--config", "jest.config.json"]),
            unit(cwd="packages/motion-utils", args=["--config", "jest.config.json"]),
            unit(cwd="packages/motion", args=["--config", "jest.config.json"]),
        ],
    },
    "react-router": {
        "url": "https://github.com/remix-run/react-router.git",
        "commit": "bb7c1f021363e96745dd3cfc5d2431e9b80ffe61",
        "runner": "Jest 30 in ESM mode, babel-jest, jsdom, projects",
        "install": PNPM,
        "reference": [reference(["node", "--experimental-vm-modules", "--no-warnings=ExperimentalWarning",
                                 "{root}/node_modules/jest/bin/jest.js", *JEST_JSON])],
        "as_is": unit(),
        "opal": {"jest_projects": {"config": "package.json", "node": ["--experimental-vm-modules"]}},
    },
    # Framework transformers
    "next-with-jest": {
        "url": "https://github.com/vercel/next.js.git",
        "commit": "cbf49db30a655ac17c92c2951885c4256db14b9e",
        "subdir": "examples/with-jest",
        "runner": "Jest 29, next/jest (SWC), jsdom",
        "lockfile": False,
        "install": ["npm", "install"],
        "reference": [reference([JEST, *JEST_JSON])],
        "as_is": unit(),
        "opal": [unit()],
    },
    "jest-preset-angular": {
        "url": "https://github.com/thymikee/jest-preset-angular.git",
        "commit": "307675d2c1ed49d50a6589c87165b84f92dfd25b",
        "runner": "Jest 30, jest-preset-angular, jsdom, Angular 20",
        "install": ["corepack", "yarn", "install", "--no-immutable"],
        "install_dir": "examples/example-app-v20",
        "reference": [reference(["{root}/examples/example-app-v20/node_modules/.bin/jest", *JEST_JSON],
                                cwd="examples/example-app-v20")],
        "as_is": unit(cwd="examples/example-app-v20", args=["--no-cache"]),
        "opal": [unit(cwd="examples/example-app-v20")],
    },
    # Node backends
    "nest-starter": {
        "url": "https://github.com/nestjs/typescript-starter.git",
        "commit": "fa03e8d88569be672a420b54578fd852b26baa6d",
        "runner": "Jest 29, ts-jest, Nest 11, supertest e2e",
        "install": ["npm", "ci"],
        "reference": [reference([JEST, *JEST_JSON]),
                      reference([JEST, "--config", "./test/jest-e2e.json", *JEST_JSON])],
        "as_is": unit(),
        "opal": [unit(), unit(args=["--config", "./test/jest-e2e.json"])],
    },
    "ghostfolio": {
        "url": "https://github.com/ghostfolio/ghostfolio.git",
        "commit": "5ab32ffc1ef539e72ed88af19a20168223e8dcb8",
        "runner": "Jest 30, Nest (ts-jest) and Angular (jest-preset-angular), nx",
        "install": ["npm", "ci"],
        "env_file": ".env.example",
        # CI runs `nx run-many --target=test`: each project's jest.config.ts.
        "reference": [reference([JEST, "--config", f"{p}/jest.config.ts", *JEST_JSON])
                      for p in ("apps/api", "apps/client", "libs/common", "libs/ui")],
        "as_is": unit(),
        "opal": [unit(args=["--config", f"{p}/jest.config.ts"]) for p in ("apps/api", "apps/client", "libs/common", "libs/ui")],
    },
    "apollo-server": {
        "url": "https://github.com/apollographql/apollo-server.git",
        "commit": "0c22029c2af47d3829e730e9820c01cef0557627",
        "runner": "Jest 30, ts-jest with type checking, node, projects",
        "install": ["npm", "ci"],
        "prepare": [["npm", "run", "compile"]],
        "reference": [reference([JEST, *JEST_JSON])],
        # CI's `npm run test:ci`.
        "as_is": unit(args=["--coverage", "--maxWorkers=2", "--reporters=default", "--reporters=jest-junit", "--verbose"]),
        "opal": {"jest_projects": {"config": "package.json"}},
    },
    "graphql-yoga": {
        "url": "https://github.com/dotansimha/graphql-yoga.git",
        "commit": "9e18c6a7e01d431253bff49353ac96d87167ce17",
        "runner": "Jest 30, ts-jest and Babel, node, projects",
        "install": PNPM,
        "reference": [reference([JEST, "--detectOpenHandles", *JEST_JSON])],
        "as_is": unit(args=["--detectOpenHandles"]),
        "opal": {"jest_projects": {"config": "jest.config.js"}},
    },
    "express-rate-limit": {
        "url": "https://github.com/express-rate-limit/express-rate-limit.git",
        "commit": "28331a79a51b8cc9270c4255e2b850d1043667ee",
        "runner": "Jest 30, ts-jest's ESM preset, collectCoverage",
        "install": PNPM,
        "reference": [reference([JEST, *JEST_JSON])],
        "as_is": unit(),
        "opal": [unit()],
    },
    # Vitest
    "tanstack-query": {
        "url": "https://github.com/TanStack/query.git",
        "commit": "2b5b6417fae536dfb68d1ef749342c9363481822",
        "runner": "Vitest 5, jsdom, type tests",
        "install": PNPM,
        "reference": [reference([VITEST, *VITEST_JSON], cwd=f"packages/{p}") for p in ("query-core", "react-query")],
        "as_is": unit(cwd="packages/query-core"),
        "opal": [unit(cwd=f"packages/{p}") for p in ("query-core", "react-query")],
    },
    "jotai": {
        "url": "https://github.com/pmndrs/jotai.git",
        "commit": "e5971ba44866d070564a0a30187f6976fee0e350",
        "runner": "Vitest 5, jsdom",
        "install": PNPM,
        "reference": [reference([VITEST, *VITEST_JSON])],
        "as_is": unit(),
        "opal": [unit()],
    },
    "ahooks": {
        "url": "https://github.com/alibaba/hooks.git",
        "commit": "43003f3bd5e6921173cfa2a791ed9d34e5221935",
        "runner": "Vitest 4, jsdom, a config per package",
        "install": PNPM,
        "reference": [reference([VITEST, *VITEST_JSON], cwd=f"packages/{p}") for p in ("hooks", "use-url-state")],
        "as_is": unit(cwd="packages/hooks"),
        "opal": [unit(cwd=f"packages/{p}") for p in ("hooks", "use-url-state")],
    },
    "xstate": {
        "url": "https://github.com/statelyai/xstate.git",
        "commit": "060cd63378b46b32e7559d569ed3ad112ee60084",
        "runner": "Vitest 3, jsdom and happy-dom, projects",
        "install": PNPM,
        "reference": [reference([VITEST, *VITEST_JSON])],
        "as_is": unit(),
        "opal": "vitest_projects",
    },
}

# What a failure says, read off its first line; the first match names the
# cause. Anything else is counted under its own first line, paths and numbers
# taken out.
CAUSES = [
    ("fake timers", r"fake timers (?:are|is) not supported"),
    ("a Vite plugin in the config", r"The requested module 'vite' does not provide an export"),
    ("Jest projects", r"does not support Jest `projects`"),
    ("command-line flag", r"unexpected argument '(--?[\w.-]+)"),
    ("DOM environment", r"\b(document|window|navigator|HTMLElement|Element|Node|localStorage|sessionStorage|location|"
                        r"requestAnimationFrame|getComputedStyle|MutationObserver|Event|CustomEvent|DOMParser) is not defined"),
    ("ES module syntax", r"Cannot use import statement|Unexpected token 'export'|ERR_REQUIRE_ESM|import\.meta"),
    ("module not found", r"Cannot find module '([^']+)'"),
    ("timeout", r"Exceeded timeout|timed out"),
]


def cause_of(message):
    first = next((line.strip() for line in message.splitlines() if line.strip() and not line.strip().startswith("●")), "")
    for name, pattern in CAUSES:
        match = re.search(pattern, message)
        if match:
            return f"{name}: {match.group(1)}" if match.groups() and name in ("command-line flag", "module not found") else name
    first = re.sub(r"\x1b\[[0-9;]*m", "", first)
    first = re.sub(r"/[^\s'\"():]+", "<path>", first)
    first = re.sub(r"\d+", "N", first)
    return f"other: {first[:110]}"


def env_for(spec, extra=None):
    env = {**os.environ, "COREPACK_ENABLE_DOWNLOAD_PROMPT": "0", **spec.get("env", {})}
    env.pop("FORCE_COLOR", None)
    env.pop("CI", None)
    if spec.get("env_file"):
        for line in (repo_dir(spec) / spec["env_file"]).read_text().splitlines():
            match = re.match(r"\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$", line)
            if match and not line.lstrip().startswith("#"):
                env.setdefault(match.group(1), match.group(2).strip().strip("'\""))
    env.update(extra or {})
    return env


def repo_dir(spec):
    name = next(n for n, s in REPOS.items() if s is spec)
    return REPOS_DIR / name


def checkout(name, spec):
    """The checkout at its commit, installed. A project that's a folder of a
    larger repository is that folder, copied out, as its scaffolder would."""
    repo = REPOS_DIR / name
    source = SURVEY / "sources" / name if spec.get("subdir") else repo
    if not (source / ".git").exists():
        source.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "init", "-q"], cwd=source, check=True)
        subprocess.run(["git", "remote", "add", "origin", spec["url"]], cwd=source, check=True)
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=source, capture_output=True, text=True).stdout.strip()
    if head != spec["commit"]:
        if spec.get("subdir"):
            subprocess.run(["git", "sparse-checkout", "set", spec["subdir"]], cwd=source, check=True)
        subprocess.run(["git", "fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", spec["commit"]], cwd=source, check=True)
        subprocess.run(["git", "checkout", "-q", "FETCH_HEAD"], cwd=source, check=True)
    if spec.get("subdir") and not repo.exists():
        subprocess.run(["cp", "-r", str(source / spec["subdir"]), str(repo)], check=True)
    install_dir = repo / spec.get("install_dir", ".")
    if not (install_dir / "node_modules").is_dir():
        log = LOGS / f"{name}-install.log"
        env = {**env_for(spec), **INSTALL_ENV}
        with open(log, "w") as out:
            proc = subprocess.run(spec["install"], cwd=install_dir, env=env, stdout=out, stderr=subprocess.STDOUT)
        if proc.returncode != 0:
            return None, f"install failed (exit {proc.returncode}), see {log.relative_to(ROOT)}"
        for i, command in enumerate(spec.get("prepare", [])):
            with open(LOGS / f"{name}-prepare{i}.log", "w") as out:
                proc = subprocess.run(command, cwd=repo, env=env, stdout=out, stderr=subprocess.STDOUT)
            if proc.returncode != 0:
                return None, f"`{' '.join(command)}` failed (exit {proc.returncode})"
        if spec.get("lockfile") is False:
            # What a project without a lockfile resolved to, kept with the run.
            lock = install_dir / "package-lock.json"
            if lock.exists():
                (SURVEY / "locks").mkdir(exist_ok=True)
                (SURVEY / "locks" / f"{name}.package-lock.json").write_bytes(lock.read_bytes())
    return repo, None


def expand(parts, repo, out=None):
    return [p.replace("{root}", str(repo)).replace("{out}", str(out) if out else "") for p in parts]


def run_logged(cmd, cwd, env, log, timeout=3600):
    with open(log, "w") as out:
        try:
            proc = subprocess.run(cmd, cwd=cwd, env=env, stdout=out, stderr=subprocess.STDOUT, timeout=timeout)
            return proc.returncode
        except subprocess.TimeoutExpired:
            out.write(f"\n[survey] timed out after {timeout} s\n")
            return None


def jest_project_units(name, spec, repo):
    """One opal config per project, from what the project's own Jest makes of
    its `projects` (`--showConfig`), turned back into config options."""
    how = spec["opal"]["jest_projects"]
    log = LOGS / f"{name}-showConfig.log"
    cmd = ["node", *how.get("node", []), str(repo / "node_modules" / "jest" / "bin" / "jest.js"), "--showConfig"]
    if how["config"] != "package.json":
        cmd += ["--config", how["config"]]
    proc = subprocess.run(cmd, cwd=repo, env=env_for(spec), capture_output=True, text=True)
    log.write_text(proc.stdout + proc.stderr)
    try:
        configs = json.loads(proc.stdout[proc.stdout.index("{"):])["configs"]
    except (ValueError, KeyError):
        return None
    folder = repo / ".opal-survey"
    folder.mkdir(exist_ok=True)
    units = []
    for i, config in enumerate(configs):
        options = {k: v for k, v in config.items()
                   if k not in ("id", "cacheDirectory", "haste", "testRunner", "runner", "dependencyExtractor",
                                "forceCoverageMatch", "unmockedModulePathPatterns", "watchPathIgnorePatterns",
                                "cwd", "snapshotResolver", "testSequencer", "prettierPath", "resolver",
                                # Already applied to every option it sets.
                                "preset")
                   or (k == "resolver" and v)}
        options["transform"] = {pattern: [path, opts] for pattern, path, opts in config.get("transform", [])}
        options["moduleNameMapper"] = {pattern: target for pattern, target in config.get("moduleNameMapper", [])}
        if not options.get("testRegex"):
            options.pop("testRegex", None)
        target = folder / f"project-{i}.json"
        target.write_text(json.dumps(options, indent=1))
        units.append(unit(args=["--config", str(target)], env=spec["opal"].get("env")))
    return units


def vitest_project_units(repo):
    """xstate's projects, one package folder each, as its root config lists them."""
    return [unit(cwd=f"packages/{p.name}") for p in sorted((repo / "packages").iterdir())
            if any(p.glob("vitest.config.*")) or (p / "package.json").exists() and any(p.rglob("*.test.ts*"))]


def results(report_paths, repo):
    merged = {}
    raw = []
    for path in report_paths:
        if path.exists():
            report = json.loads(path.read_text())
            raw.append(report)
            merged.update(test_repos.statuses(report, repo))
    return merged, raw


def failure_messages(raw_reports, repo):
    """{(file, name, occurrence) or (file,): the message opal gave}."""
    out = {}
    for report in raw_reports:
        for result in report["testResults"]:
            file = os.path.relpath(result["name"], repo)
            if result.get("message") and not result["assertionResults"]:
                out[(file,)] = result["message"]
            seen = defaultdict(int)
            for test in result["assertionResults"]:
                name = " ".join(test.get("ancestorTitles", []) + [test["title"]])
                key = (file, name, seen[name])
                seen[name] += 1
                if test.get("failureMessages"):
                    out[key] = "\n".join(test["failureMessages"])
    return out


def survey(name, spec, opal):
    repo, error = checkout(name, spec)
    if error:
        return {"error": error}
    print(f"{name} @ {spec['commit'][:10]}", flush=True)
    out_dir = SURVEY / "reports" / name
    out_dir.mkdir(parents=True, exist_ok=True)
    for old in out_dir.glob("*.json"):
        old.unlink()

    # 1. The reference.
    ref_paths = []
    for i, run in enumerate(spec["reference"]):
        path = out_dir / f"reference-{i}.json"
        run_logged(expand(run["cmd"], repo, path), repo / run["cwd"], env_for(spec, run["env"]), LOGS / f"{name}-reference-{i}.log")
        ref_paths.append(path)
    ref, _ = results(ref_paths, repo)

    # 2. opal, as CI runs it.
    as_is = spec["as_is"]
    as_is_path = out_dir / "opal-as-is.json"
    as_is_log = LOGS / f"{name}-opal-as-is.log"
    code = run_logged([opal, "test", *as_is["args"], "--json", f"--outputFile={as_is_path}"],
                      repo / as_is["cwd"], env_for(spec, as_is["env"]), as_is_log)
    as_is_result = {"exit": code, "report": as_is_path.exists()}
    if not as_is_path.exists():
        as_is_result["cause"] = cause_of(as_is_log.read_text(errors="replace"))

    # 3. opal, unit by unit.
    units = spec["opal"]
    if isinstance(units, dict):
        units = jest_project_units(name, spec, repo)
        if units is None:
            return {"error": f"`jest --showConfig` failed, see {(LOGS / f'{name}-showConfig.log').relative_to(ROOT)}"}
    elif units == "vitest_projects":
        units = vitest_project_units(repo)
    opal_paths, unit_errors = [], []
    for i, u in enumerate(units):
        path = out_dir / f"opal-{i}.json"
        log = LOGS / f"{name}-opal-{i}.log"
        run_logged([opal, "test", "--ci", *u["args"], "--json", f"--outputFile={path}"], repo / u["cwd"],
                   env_for(spec, u["env"]), log)
        if not path.exists():
            unit_errors.append({"unit": u, "cause": cause_of(log.read_text(errors="replace"))})
        opal_paths.append(path)
    mine, raw = results(opal_paths, repo)
    messages = failure_messages(raw, repo)

    # 4. Why opal didn't pass what the reference passed.
    causes = Counter()
    examples = {}
    unrun_files = {e["cause"] for e in unit_errors}
    for key, status in ref.items():
        if status != "passed" or mine.get(key) == "passed":
            continue
        if key in mine:
            message = messages.get(key) or messages.get((key[0],), "")
            cause = cause_of(message) if message else f"opal: {mine[key]}"
        elif (key[0],) in messages:
            cause = cause_of(messages[(key[0],)])
        elif unit_errors:
            cause = "unit didn't run: " + "; ".join(sorted(unrun_files))
        elif re.search(r"\.test-d\.[cm]?tsx?$", key[0]):
            cause = "Vitest's type tests (typecheck)"
        else:
            cause = "test not run by opal"
        causes[cause] += 1
        examples.setdefault(cause, f"{key[0]} | {key[1]}")
    extra_passes = sum(1 for k, v in mine.items() if v == "passed" and ref.get(k) not in ("passed", None))
    return {
        "commit": spec["commit"],
        "runner": spec["runner"],
        "reference": dict(Counter(ref.values())),
        "opal": dict(Counter(mine.values())),
        "agree": sum(1 for k in set(ref) | set(mine) if ref.get(k) == mine.get(k)),
        "total": len(set(ref) | set(mine)),
        "as_is": as_is_result,
        "units": len(units),
        "unit_errors": unit_errors,
        "causes": dict(causes.most_common()),
        "examples": examples,
        "opal_passes_reference_fails": extra_passes,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--opal", required=True)
    parser.add_argument("--install-only", action="store_true")
    parser.add_argument("repos", nargs="*", help=f"some of: {', '.join(REPOS)}")
    args = parser.parse_args()
    opal = str(Path(args.opal).resolve())
    LOGS.mkdir(parents=True, exist_ok=True)
    names = args.repos or list(REPOS)
    if args.install_only:
        for name in names:
            _, error = checkout(name, REPOS[name])
            print(f"{name}: {error or 'installed'}", flush=True)
        return
    record = {"date": datetime.now().isoformat(timespec="seconds"),
              "opal_sha256": subprocess.run(["sha256sum", opal], capture_output=True, text=True).stdout.split()[0],
              "node": subprocess.run(["node", "--version"], capture_output=True, text=True).stdout.strip(),
              "repos": {}}
    for name in names:
        record["repos"][name] = survey(name, REPOS[name], opal)
        found = record["repos"][name]
        if "error" in found:
            print(f"  {found['error']}", flush=True)
        else:
            passed = found["reference"].get("passed", 0)
            print(f"  reference passed {passed}; opal passed {found['opal'].get('passed', 0)}; "
                  f"as is: {'ran' if found['as_is']['report'] else found['as_is'].get('cause')}", flush=True)

    print("\n| project | runner | its runner passed | opal passed | agreeing | opal as CI runs it |")
    print("|---|---|---:|---:|---:|---|")
    totals = Counter()
    repos_per_cause = defaultdict(set)
    for name, found in record["repos"].items():
        if "error" in found:
            print(f"| {name} | {REPOS[name]['runner']} | - | - | - | {found['error']} |")
            continue
        as_is = "runs" if found["as_is"]["report"] else f"stops: {found['as_is'].get('cause')}"
        print(f"| {name} | {found['runner']} | {found['reference'].get('passed', 0)} | {found['opal'].get('passed', 0)} | "
              f"{found['agree']} of {found['total']} | {as_is} |")
        for cause, count in found["causes"].items():
            totals[cause] += count
            repos_per_cause[cause].add(name)
        if not found["as_is"]["report"]:
            repos_per_cause[f"as is: {found['as_is'].get('cause')}"].add(name)
    print("\n| why opal didn't pass what the reference passed | tests | projects |")
    print("|---|---:|---|")
    for cause, count in totals.most_common():
        print(f"| {cause} | {count} | {', '.join(sorted(repos_per_cause[cause]))} |")
    print("\n| why `opal test` stopped where CI's command ran | projects |")
    print("|---|---|")
    for cause in sorted(c for c in repos_per_cause if c.startswith("as is: ")):
        print(f"| {cause[7:]} | {', '.join(sorted(repos_per_cause[cause]))} |")
    path = ROOT / "compat" / "results" / f"survey-repos-{datetime.now():%Y%m%d-%H%M%S}.json"
    path.write_text(json.dumps(record, indent=1, default=list) + "\n")
    print(f"\nwrote {path.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
