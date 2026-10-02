#!/usr/bin/env python3
"""Opal against npm, pnpm, yarn, and bun on one real package.json.

    python3 benchmarks/compare-pms.py <express|next> [--opal PATH] [--rounds cold=3,ci=3,warm=5,noop=5]

Four scenarios, each a different question:
  cold  no lockfile, no cache, no node_modules: a first install
  ci    lockfile only: a fresh CI runner
  warm  lockfile + cache, node_modules deleted: a reinstall on your machine
  noop  everything present: re-running install with nothing to do

Each tool gets its own copy of the project and its own empty cache next to it,
on one filesystem, so every tool that hardlinks can. Runs interleave round by
round, starting from a different tool each round, so a swing in network or
page-cache state lands on every tool instead of on whichever ran then (absolute
timings have moved 2-3x between sessions on the same machine). Install scripts
are off for everyone because opal does not run them, and update checks are off
because they are a network request that isn't installing.

Wall time and peak RSS come from wait4 on the tool's process. Compare tools
within one run, never across runs.

Needs Python 3.9+, network access, and node plus every tool being compared on
PATH (`--tools` picks a subset). yarn is Yarn 1 (classic). The README's numbers
launched pnpm as `node <pnpm.mjs>`; see --pnpm below. Raw samples are written
as JSON to benchmarks/results/ (--results), beside this script, so a published
table can be committed together with the samples it was computed from. The
file records the path of the opal binary it ran, so run a binary that sits
somewhere you are happy to publish.

`opal --version` is the same for a release and for any local build of that
version, so the SHA-256 of the binary that ran is recorded too. For a release
binary it should equal `sha256sum` of the `opal` inside the release archive.
"""

import argparse
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

PROJECTS = {
    "express": {
        "manifest": {
            "name": "bench-express",
            "version": "1.0.0",
            "private": True,
            "dependencies": {"express": "^5"},
        },
        "check": "require('express')",
    },
    # The package.json create-next-app@16.3.2 writes, kept verbatim so every
    # run installs the same project. Its caret ranges still float, so a later
    # run can resolve newer versions of those packages.
    "next": {
        "manifest": {
            "name": "next-test",
            "version": "0.1.0",
            "private": True,
            "scripts": {
                "dev": "next dev",
                "build": "next build",
                "start": "next start",
                "lint": "eslint",
            },
            "dependencies": {
                "next": "16.3.2",
                "react": "19.2.8",
                "react-dom": "19.2.8",
            },
            "devDependencies": {
                "@tailwindcss/postcss": "^4",
                "@types/node": "^20",
                "@types/react": "^19",
                "@types/react-dom": "^19",
                "eslint": "^9",
                "eslint-config-next": "16.3.2",
                "tailwindcss": "^4",
                "typescript": "^5",
            },
        },
        "check": "require('next/package.json'); require('react'); require('react-dom/server')",
    },
}
SCENARIOS = {
    "cold": {"lockfile", "cache", "node_modules"},
    "ci": {"cache", "node_modules"},
    "warm": {"node_modules"},
    "noop": set(),
}
DEFAULT_ROUNDS = {"cold": 3, "ci": 3, "warm": 5, "noop": 5}
RUN_TIMEOUT = 30 * 60


def tool_table(opal, pnpm, cache):
    return {
        "opal": {
            "cmd": [opal, "install", "--cache-dir", cache / "opal"],
            "lockfiles": ["opal.lock"],
            "caches": [cache / "opal"],
            "version": [opal, "--version"],
        },
        "npm": {
            "cmd": ["npm", "install", "--cache", cache / "npm", "--ignore-scripts",
                    "--no-audit", "--no-fund", "--no-update-notifier"],
            "lockfiles": ["package-lock.json"],
            "caches": [cache / "npm"],
            "version": ["npm", "--version"],
        },
        "pnpm": {
            "cmd": [*pnpm, "install", "--store-dir", cache / "pnpm-store",
                    "--cache-dir", cache / "pnpm-cache", "--ignore-scripts",
                    "--config.update-notifier=false"],
            "lockfiles": ["pnpm-lock.yaml"],
            "caches": [cache / "pnpm-store", cache / "pnpm-cache"],
            "version": [*pnpm, "--version"],
        },
        # Yarn 1 (classic), which is what `yarn` resolves to here through corepack.
        "yarn": {
            "cmd": ["yarn", "install", "--cache-folder", cache / "yarn",
                    "--ignore-scripts", "--non-interactive"],
            "lockfiles": ["yarn.lock"],
            "caches": [cache / "yarn"],
            "version": ["yarn", "--version"],
            "files": {".yarnrc": "disable-self-update-check true\n"},
        },
        "bun": {
            "cmd": ["bun", "install", "--ignore-scripts"],
            "lockfiles": ["bun.lock", "bun.lockb"],
            "caches": [cache / "bun"],
            "env": {"BUN_INSTALL_CACHE_DIR": str(cache / "bun")},
            "version": ["bun", "--version"],
        },
    }


def remove(path):
    if not path.exists() and not path.is_symlink():
        return
    # CAS objects and the trees linked from them are read-only.
    subprocess.run(["chmod", "-R", "u+w", path], stderr=subprocess.DEVNULL)
    subprocess.run(["rm", "-rf", path], check=True)


def timed(cmd, cwd, env, log):
    with open(log, "w") as out:
        started = time.perf_counter()
        child = subprocess.Popen([str(c) for c in cmd], cwd=cwd, env=env,
                                 stdout=out, stderr=subprocess.STDOUT)
        deadline = started + RUN_TIMEOUT
        while True:
            pid, status, usage = os.wait4(child.pid, os.WNOHANG)
            if pid:
                break
            if time.perf_counter() > deadline:
                child.kill()
                pid, status, usage = os.wait4(child.pid, 0)
                break
            time.sleep(0.005)
        wall = time.perf_counter() - started
    child.returncode = os.waitstatus_to_exitcode(status)
    # ru_maxrss is kilobytes on Linux and bytes on macOS.
    per_mb = 1024 * 1024 if sys.platform == "darwin" else 1024
    return wall, usage.ru_maxrss / per_mb, child.returncode


def count_packages(node_modules):
    """Installed package directories, symlinks not followed.

    Counts every real directory holding a package.json directly under a
    node_modules (or a scope inside one), so hoisted, nested, and pnpm's
    .pnpm layouts are all counted the same way.
    """
    seen = 0
    stack = [node_modules]
    while stack:
        directory = stack.pop()
        try:
            entries = list(os.scandir(directory))
        except OSError:
            continue
        for entry in entries:
            if not entry.is_dir(follow_symlinks=False):
                continue
            path = Path(entry.path)
            if entry.name.startswith("@") and directory.name == "node_modules":
                stack.append(path)
            elif entry.name == ".pnpm":
                for inner in os.scandir(path):
                    if inner.is_dir(follow_symlinks=False):
                        stack.append(Path(inner.path) / "node_modules")
            elif (path / "package.json").exists() and not entry.name.startswith("."):
                seen += 1
                stack.append(path / "node_modules")
    return seen


def summarize(samples):
    if not samples:
        return "-"
    median = statistics.median(samples)
    if len(samples) == 1:
        return fmt(median)
    return f"{fmt(median)} ({fmt(min(samples))}–{fmt(max(samples))})"


def fmt(seconds):
    return f"{seconds * 1000:.0f}ms" if seconds < 1 else f"{seconds:.2f}s"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("project", choices=PROJECTS)
    parser.add_argument("--opal", default=shutil.which("opal"))
    # pnpm 11 is one 12.8 MB bundle that node compiles on every start (~1.2s
    # here). The standalone @pnpm/exe starts slower still, so `node <pnpm.mjs>`
    # is the fairest launcher; pass it with --pnpm.
    parser.add_argument("--pnpm", default="pnpm")
    parser.add_argument("--tools", default="opal,npm,pnpm,yarn,bun")
    parser.add_argument("--rounds", default="")
    parser.add_argument("--work", default="/tmp/opal-compare")
    parser.add_argument("--results", default=str(Path(__file__).resolve().parent / "results"))
    args = parser.parse_args()

    rounds = dict(DEFAULT_ROUNDS)
    for item in filter(None, args.rounds.split(",")):
        name, count = item.split("=")
        rounds[name] = int(count)
    project = PROJECTS[args.project]
    work = Path(args.work) / args.project
    remove(work)
    cache = work / "cache"
    logs = work / "logs"
    logs.mkdir(parents=True)

    tools = tool_table(Path(args.opal).resolve(), args.pnpm.split(), cache)
    names = args.tools.split(",")
    manifest_text = json.dumps(project["manifest"], indent=2) + "\n"

    base_env = {k: v for k, v in os.environ.items() if k not in ("CI", "OPAL_CACHE_DIR")}
    base_env["COREPACK_ENABLE_DOWNLOAD_PROMPT"] = "0"
    meta = {
        "project": args.project,
        "date": datetime.now().isoformat(timespec="seconds"),
        "host": f"{platform.system()} {platform.release()}, {os.cpu_count()} threads",
        # `free` is Linux-only; macOS has no equivalent worth recording here.
        "memory": (subprocess.run(["free", "-m"], capture_output=True, text=True).stdout
                   if shutil.which("free") else ""),
        "node": subprocess.run(["node", "--version"], capture_output=True, text=True).stdout.strip(),
        "versions": {},
        "opal_binary": {
            "path": str(Path(args.opal).resolve()),
            "sha256": hashlib.sha256(Path(args.opal).resolve().read_bytes()).hexdigest(),
        },
        "rounds": rounds,
    }
    for name in names:
        tool = tools[name]
        env = {**base_env, **tool.get("env", {})}
        meta["versions"][name] = subprocess.run(
            [str(c) for c in tool["version"]], capture_output=True, text=True, env=env
        ).stdout.strip().removeprefix("opal ")
        project_dir = work / name
        project_dir.mkdir()
        (project_dir / "package.json").write_text(manifest_text)
        for file, text in tool.get("files", {}).items():
            (project_dir / file).write_text(text)
    meta["commands"] = {name: " ".join(map(str, tools[name]["cmd"])) for name in names}
    print(json.dumps({k: v for k, v in meta.items() if k != "memory"}, indent=2))
    print(meta["memory"])

    results = []
    for scenario, removed in SCENARIOS.items():
        for round_index in range(rounds.get(scenario, 0)):
            shift = round_index % len(names)
            for name in names[shift:] + names[:shift]:
                tool = tools[name]
                project_dir = work / name
                if "node_modules" in removed:
                    remove(project_dir / "node_modules")
                if "lockfile" in removed:
                    for lockfile in tool["lockfiles"]:
                        remove(project_dir / lockfile)
                if "cache" in removed:
                    for directory in tool["caches"]:
                        remove(directory)
                log = logs / f"{scenario}-{round_index + 1}-{name}.log"
                env = {**base_env, **tool.get("env", {})}
                wall, rss, code = timed(tool["cmd"], project_dir, env, log)
                check = subprocess.run(["node", "-e", project["check"]], cwd=project_dir,
                                       capture_output=True).returncode == 0
                sample = {"scenario": scenario, "round": round_index + 1, "tool": name,
                          "wall": wall, "rss_mb": rss, "exit": code, "runs": check,
                          "packages": count_packages(project_dir / "node_modules")}
                if name == "opal":
                    sample["summary"] = next((line for line in log.read_text().splitlines()
                                              if " packages " in line and
                                              (" in " in line or "already installed" in line)), "")
                results.append(sample)
                status = "ok" if code == 0 and check else f"FAIL exit={code} runs={check}"
                print(f"{scenario:<5} #{round_index + 1} {name:<5} {fmt(wall):>9}  "
                      f"{rss:6.0f} MB  {sample['packages']:4} pkgs  {status}  "
                      f"{sample.get('summary', '')}", flush=True)

    results_dir = Path(args.results)
    results_dir.mkdir(parents=True, exist_ok=True)
    out = results_dir / f"{args.project}-{datetime.now():%Y%m%d-%H%M%S}.json"
    out.write_text(json.dumps({"meta": meta, "results": results}, indent=2))

    ran = [s for s in SCENARIOS if rounds.get(s, 0)]
    print(f"\n{args.project}: median wall time (min–max), {meta['date']}\n")
    print("| Tool | " + " | ".join(ran) + " | Peak RSS (cold) | Packages |")
    print("|---|" + "---|" * (len(ran) + 2))
    for name in names:
        mine = [r for r in results if r["tool"] == name]
        cells = [summarize([r["wall"] for r in mine if r["scenario"] == s and r["exit"] == 0])
                 for s in ran]
        cold_rss = [r["rss_mb"] for r in mine if r["scenario"] == "cold"]
        rss = f"{statistics.median(cold_rss):.0f} MB" if cold_rss else "-"
        packages = sorted({r["packages"] for r in mine})
        failures = sum(1 for r in mine if r["exit"] != 0 or not r["runs"])
        label = f"{name} {meta['versions'][name]}" + (f" ({failures} failed)" if failures else "")
        print(f"| {label} | " + " | ".join(cells) + f" | {rss} | "
              + "/".join(map(str, packages)) + " |")
    print(f"\nraw samples: {out}")


if __name__ == "__main__":
    sys.exit(main())
