#!/usr/bin/env python3
"""Install each framework's own starter project with opal, then build it, test
it, and serve it.

    python3 compat/frameworks.py --opal PATH [next vite-react ...] [--timeout build=900,runtime=120] [--keep]

The scaffolds are under frameworks/<name>/, each generated once by the
framework's own scaffolder and committed with a SCAFFOLD file naming the
command, the scaffolder's version, and the date (express is hand-written).
Running the scaffolders here instead would make the matrix move with their
releases. Their caret ranges still float, so the versions a run resolved are
recorded with it.

Each scaffold is copied into a scratch folder and installed with
`opal install` against an empty OPAL_CACHE_DIR. Then up to three checks, each
with a timeout:
  build    `npm run build`; it has to exit 0 and leave its output behind
           (.next/BUILD_ID, dist/index.html, dist/main.js, .svelte-kit/output)
  test     `npm test`, where the scaffold ships tests (nest)
  runtime  the production server through its npm script on a free port,
           polled until it answers HTTP 200, then its whole process group is
           killed
Every script runs through npm with the project's own node_modules, which is
what a user does: opal is only the installer. A check that depends on one
that failed is not run, and is shown as such rather than as a pass.

Opal does not run install scripts and says so in a warning naming each
package it skipped. That list is parsed out of its output into the "scripts
skipped" column, so a pass is never read as "nothing was skipped".

Needs Python 3.9+, network access, and node and npm on PATH. Results are
written as JSON to compat/results/ (--results) with the SHA-256 of the opal
binary that ran and the node, npm, and opal versions, and the markdown table
is printed for compat/COMPATIBILITY.md. Exits non-zero if any cell failed.
The scratch folder is deleted afterwards unless --keep is given.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import platform
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
# Loading compare-pms.py would otherwise leave a __pycache__ in benchmarks/,
# which nothing ignores.
sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location(
    "compare_pms", HERE.parent / "benchmarks" / "compare-pms.py")
compare_pms = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(compare_pms)
remove, count_packages, fmt = compare_pms.remove, compare_pms.count_packages, compare_pms.fmt

# `serve` is the npm script that starts the production server, then the
# arguments passed to it after `--`. PORT is set for every server as well,
# which is how nest and express are told.
FRAMEWORKS = {
    "next": {
        "build": "build", "output": ".next/BUILD_ID",
        "serve": ["start", "--port", "{port}", "--hostname", "127.0.0.1"], "path": "/",
        "versions": ["next", "react"],
    },
    "vite-react": {
        "build": "build", "output": "dist/index.html",
        "serve": ["preview", "--port", "{port}", "--strictPort", "--host", "127.0.0.1"],
        "path": "/", "versions": ["vite", "react"],
    },
    "vite-vue": {
        "build": "build", "output": "dist/index.html",
        "serve": ["preview", "--port", "{port}", "--strictPort", "--host", "127.0.0.1"],
        "path": "/", "versions": ["vite", "vue"],
    },
    # Astro 7 moves `astro preview` into a detached background process when it
    # thinks a coding agent launched it, and that process outlives the kill.
    # `--ignore-lock` keeps it in the foreground wherever this runs; in a fresh
    # copy there is no lock for it to ignore.
    "astro": {
        "build": "build", "output": "dist/index.html",
        "serve": ["preview", "--port", "{port}", "--host", "127.0.0.1", "--ignore-lock"],
        "path": "/", "versions": ["astro"],
    },
    # adapter-auto finds no deployment target here and writes no adapter
    # output; `vite preview` serves the build from .svelte-kit/output.
    "sveltekit": {
        "build": "build", "output": ".svelte-kit/output",
        "serve": ["preview", "--port", "{port}", "--strictPort", "--host", "127.0.0.1"],
        "path": "/", "versions": ["@sveltejs/kit", "vite"],
    },
    "nest": {
        "build": "build", "output": "dist/main.js", "test": "test",
        "serve": ["start:prod"], "path": "/", "versions": ["@nestjs/core", "vitest"],
    },
    "express": {"serve": ["start"], "path": "/health", "versions": ["express"]},
}
CHECKS = ["install", "build", "test", "runtime"]
DEFAULT_TIMEOUTS = {"install": 900, "build": 900, "test": 600, "runtime": 120}
# Copied scaffolds never bring these along, so a stray local install in the
# repository can't turn a run into a warm one.
NOT_COPIED = ["SCAFFOLD", "node_modules", "opal.lock", "package-lock.json",
              "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb"]
ANSI = re.compile(r"\x1b\[[0-9;]*m")
TAIL_LINES = 25


def kill_group(process):
    """Stop everything a command started, not just npm.

    `npm run` starts a shell that starts the tool, which may start workers of
    its own; killing npm alone leaves those running and holding the port.
    """
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


def run(cmd, cwd, env, log, timeout):
    """(exit code or None on timeout, seconds) for one command, output to `log`."""
    with open(log, "w") as out:
        started = time.perf_counter()
        child = subprocess.Popen([str(c) for c in cmd], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                 stdout=out, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            code = None
        finally:
            seconds = time.perf_counter() - started
            kill_group(child)
    return code, seconds


def tail(log, work):
    lines = ANSI.sub("", Path(log).read_text(errors="replace")).splitlines()
    return [line.replace(str(work), "<work>") for line in lines[-TAIL_LINES:]]


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def serve(cmd, cwd, env, log, url, timeout):
    """Start a server and wait for HTTP 200 at `url`; always stop it after."""
    # A proxy from the environment must never answer for localhost.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with open(log, "w") as out:
        started = time.perf_counter()
        child = subprocess.Popen([str(c) for c in cmd], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                 stdout=out, stderr=subprocess.STDOUT, start_new_session=True)
        last = "no answer"
        try:
            while time.perf_counter() - started < timeout:
                if child.poll() is not None:
                    return False, time.perf_counter() - started, f"server exited {child.returncode}"
                try:
                    with opener.open(url, timeout=5) as response:
                        if response.status == 200:
                            return True, time.perf_counter() - started, "HTTP 200"
                        last = f"HTTP {response.status}"
                except urllib.error.HTTPError as error:
                    last = f"HTTP {error.code}"
                except (urllib.error.URLError, OSError) as error:
                    last = str(getattr(error, "reason", error))
                time.sleep(0.25)
            return False, time.perf_counter() - started, f"no HTTP 200 in {timeout}s ({last})"
        finally:
            kill_group(child)


def still_listening(port):
    with socket.socket() as probe:
        probe.settimeout(1)
        return probe.connect_ex(("127.0.0.1", port)) == 0


def scripts_not_run(output):
    """What opal's install-scripts warnings name: ({package: events}, the lines).

    The two warnings `opal install` prints on stderr (crates/opal-cli/src/
    main.rs): one line for the project's own scripts, and a header for
    dependencies followed by one indented `  name@version: events` line each.
    The project's own scripts are keyed "(project)". The lines are kept as
    printed, so a change in wording shows up as lines nothing was parsed from.
    """
    lines = output.splitlines()
    found, printed = {}, []
    for index, line in enumerate(lines):
        if "this project's own install scripts were not run" in line:
            found["(project)"] = line.rsplit("): ", 1)[-1].strip()
            printed.append(line)
        elif "scripts were not run" in line:
            printed.append(line)
            for item in lines[index + 1:]:
                if not item.startswith("  "):
                    break
                printed.append(item)
                package, _, events = item.strip().partition(": ")
                found[package] = events
    return found, printed


def installed_version(project, package):
    try:
        manifest = json.loads((project / "node_modules" / package / "package.json").read_text())
        return manifest.get("version", "?")
    except (OSError, ValueError):
        return None


def npm_command(script, args, port):
    cmd = ["npm", "run", script]
    if args:
        cmd += ["--", *(arg.replace("{port}", str(port)) for arg in args)]
    return cmd


def cell(check):
    if check is None:
        return "-"
    if check["status"] == "pass":
        return f"pass ({fmt(check['seconds'])})"
    if check["status"] == "not run":
        return "not run"
    return f"FAIL: {check['reason']}"


def report(name, check, entry):
    print(f"{name:<10} {check:<8} {cell(entry)}", flush=True)
    for line in entry.get("tail", [])[-10:]:
        print(f"           | {line}", flush=True)


def check_framework(name, config, opal, work, base_env, timeouts):
    framework_dir = work / name
    project = framework_dir / "project"
    logs = framework_dir / "logs"
    logs.mkdir(parents=True)
    shutil.copytree(HERE / "frameworks" / name, project,
                    ignore=shutil.ignore_patterns(*NOT_COPIED))
    env = {**base_env, "OPAL_CACHE_DIR": str(framework_dir / "cache")}
    result = {"framework": name, "scaffold": scaffold_line(name)}

    def finish(check, code, seconds, log, missing=None):
        entry = {"status": "pass", "exit": code, "seconds": round(seconds, 3)}
        if code is None:
            entry.update(status="fail", reason=f"timeout {timeouts[check]}s")
        elif code != 0:
            entry.update(status="fail", reason=f"exit {code}")
        elif missing:
            entry.update(status="fail", reason=f"no {missing}")
        if entry["status"] != "pass":
            entry["tail"] = tail(log, work)
        result[check] = entry
        report(name, check, entry)
        return entry["status"] == "pass"

    log = logs / "install.log"
    code, seconds = run([opal, "install"], project, env, log, timeouts["install"])
    output = ANSI.sub("", log.read_text(errors="replace"))
    installed = finish("install", code, seconds, log)
    result["install"]["summary"] = next((line for line in output.splitlines()
                                         if " packages " in line and " installed" in line), "")
    result["install"]["packages"] = count_packages(project / "node_modules")
    result["scripts_not_run"], result["scripts_warning"] = scripts_not_run(output)
    result["versions"] = {package: installed_version(project, package)
                          for package in config["versions"]}

    built = installed
    if "build" in config:
        if installed:
            log = logs / "build.log"
            code, seconds = run(["npm", "run", config["build"]], project, env, log,
                                timeouts["build"])
            missing = None if (project / config["output"]).exists() else config["output"]
            built = finish("build", code, seconds, log, missing)
        else:
            result["build"] = {"status": "not run"}
    else:
        result["build"] = None

    if "test" in config:
        if installed:
            log = logs / "test.log"
            code, seconds = run(["npm", "run", config["test"]], project, env, log, timeouts["test"])
            finish("test", code, seconds, log)
        else:
            result["test"] = {"status": "not run"}
    else:
        result["test"] = None

    if built:
        port = free_port()
        log = logs / "runtime.log"
        url = f"http://127.0.0.1:{port}{config['path']}"
        script, *args = config["serve"]
        ok, seconds, reason = serve(npm_command(script, args, port), project,
                                    {**env, "PORT": str(port)}, log, url, timeouts["runtime"])
        entry = {"status": "pass" if ok else "fail", "seconds": round(seconds, 3),
                 "url": f"http://127.0.0.1:<port>{config['path']}", "reason": reason}
        if not ok:
            entry["tail"] = tail(log, work)
        result["runtime"] = entry
        report(name, "runtime", entry)
        # A server that detached from its process group is out of reach of the
        # kill, and would hold the port and the scratch folder after the run.
        if still_listening(port):
            entry["left_listening"] = True
            print(f"{name:<10} warning: something still listens on port {port} after the "
                  f"server's process group was killed", flush=True)
    else:
        result["runtime"] = {"status": "not run"}
    return result


def commands(config):
    listed = {"install": "opal install"}
    for check in ("build", "test"):
        if check in config:
            listed[check] = f"npm run {config[check]}"
    script, *args = config["serve"]
    listed["runtime"] = " ".join(npm_command(script, args, "<port>"))
    return listed


def scaffold_line(name):
    """The SCAFFOLD file's first line: the command that generated it."""
    try:
        return (HERE / "frameworks" / name / "SCAFFOLD").read_text().splitlines()[0]
    except (OSError, IndexError):
        return ""


def failed(result):
    return any(result.get(check) and result[check]["status"] != "pass" for check in CHECKS)


def table(results, meta):
    lines = [
        f"Frameworks: opal {meta['versions']['opal']} (SHA-256 {meta['opal_binary']['sha256']}), "
        f"node {meta['versions']['node']}, npm {meta['versions']['npm']}, {meta['date']}",
        "",
        "| Framework | Resolved | install | build | test | runtime | Scripts skipped |",
        "|---|---|---|---|---|---|---|",
    ]
    for result in results:
        resolved = ", ".join(f"{package} {version or '?'}"
                             for package, version in result["versions"].items())
        skipped = result["scripts_not_run"]
        own = skipped.get("(project)")
        packages = [package for package in skipped if package != "(project)"]
        parts = ([f"project ({own})"] if own else []) + ([", ".join(packages)] if packages else [])
        lines.append(f"| {result['framework']} | {resolved} | "
                     + " | ".join(cell(result[check]) for check in CHECKS)
                     + f" | {'; '.join(parts) or 'none'} |")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("frameworks", nargs="*", metavar="FRAMEWORK",
                        help=f"a subset to run (default: all of {', '.join(FRAMEWORKS)})")
    parser.add_argument("--opal", required=True)
    parser.add_argument("--timeout", default="",
                        help="seconds per check, e.g. install=900,build=900,test=600,runtime=120")
    parser.add_argument("--keep", action="store_true", help="keep the scratch folder")
    parser.add_argument("--work", default=tempfile.gettempdir(),
                        help="where the scratch folder is made (default: the system temp folder)")
    parser.add_argument("--results", default=str(HERE / "results"))
    args = parser.parse_args()

    opal = Path(args.opal).resolve()
    scratch = Path(tempfile.gettempdir()).resolve()
    if args.results == parser.get_default("results") and opal.is_relative_to(scratch):
        sys.exit(f"frameworks: {opal} is under {scratch}, and its path is recorded in the results, "
                 f"which default to the repository. Move the binary, or pass --results DIR.")
    timeouts = dict(DEFAULT_TIMEOUTS)
    for item in filter(None, args.timeout.split(",")):
        check, seconds = item.split("=")
        if check not in timeouts:
            sys.exit(f"frameworks: --timeout names {check}; the checks are {', '.join(CHECKS)}")
        timeouts[check] = int(seconds)
    unknown = [name for name in args.frameworks if name not in FRAMEWORKS]
    if unknown:
        sys.exit(f"frameworks: no scaffold named {', '.join(unknown)}; "
                 f"the scaffolds are {', '.join(FRAMEWORKS)}")
    names = args.frameworks or list(FRAMEWORKS)

    work = Path(tempfile.mkdtemp(prefix="opal-compat-", dir=args.work)).resolve()
    (work / "tmp").mkdir()
    base_env = {k: v for k, v in os.environ.items()
                if k not in ("CI", "OPAL_CACHE_DIR", "NODE_ENV", "PORT")}
    base_env.update({
        # Whatever a framework writes to the temp directory goes with the
        # scratch folder instead of outliving the run.
        "TMPDIR": str(work / "tmp"),
        # Telemetry and update checks are network requests that aren't the
        # thing being checked.
        "NEXT_TELEMETRY_DISABLED": "1",
        "ASTRO_TELEMETRY_DISABLED": "1",
        "npm_config_update_notifier": "false",
        "npm_config_fund": "false",
    })

    def version(cmd):
        return subprocess.run(cmd, capture_output=True, text=True,
                              env=base_env).stdout.strip().removeprefix("opal ")

    meta = {
        "date": datetime.now().isoformat(timespec="seconds"),
        "host": f"{platform.system()} {platform.release()}, {os.cpu_count()} threads",
        "versions": {"opal": version([str(opal), "--version"]),
                     "node": version(["node", "--version"]).removeprefix("v"),
                     "npm": version(["npm", "--version"])},
        "opal_binary": {"path": str(opal), "sha256": hashlib.sha256(opal.read_bytes()).hexdigest()},
        "timeouts": timeouts,
        "commands": {name: commands(FRAMEWORKS[name]) for name in names},
    }
    print(json.dumps(meta, indent=2), flush=True)

    results = []
    started = time.perf_counter()
    try:
        for name in names:
            results.append(check_framework(name, FRAMEWORKS[name], opal, work, base_env, timeouts))
    finally:
        if args.keep:
            print(f"\nscratch kept: {work}")
        else:
            remove(work)
    meta["seconds"] = round(time.perf_counter() - started, 1)

    results_dir = Path(args.results)
    results_dir.mkdir(parents=True, exist_ok=True)
    out = results_dir / f"frameworks-{datetime.now():%Y%m%d-%H%M%S}.json"
    out.write_text(json.dumps({"meta": meta, "results": results}, indent=2) + "\n")

    failures = [result["framework"] for result in results if failed(result)]
    print("\n" + table(results, meta) + "\n")
    print(f"{len(results) - len(failures)} of {len(results)} frameworks passed every check"
          + (f"; failed: {', '.join(failures)}" if failures else "")
          + f" ({fmt(meta['seconds'])})")
    print(f"raw results: {out}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
