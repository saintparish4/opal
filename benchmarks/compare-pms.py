#!/usr/bin/env python3
"""Opal against npm, pnpm, yarn, and bun on one real package.json.

    python3 benchmarks/compare-pms.py <express|next> [--opal PATH] [--rounds cold=3,ci=3,warm=5,noop=5]

Six scenarios, each a different question:
  cold       no lockfile, no cache, no node_modules: a first install
  ci         lockfile only: a fresh CI runner
  warm       lockfile + cache, node_modules deleted: a reinstall on your machine
  ci-cached  the same state as warm, run with each tool's frozen-lockfile
             command (`npm ci` and its equivalents): a CI runner that restored
             its cache
  noop       everything present: re-running install with nothing to do
  add        one new dependency added to an installed project with each
             tool's own command (`opal add`, `npm install <pkg>`, `pnpm add`,
             `yarn add`, `bun add`). Each round adds a different small package
             with no dependencies of its own, pinned to one version, so every
             round pays for metadata and a download it has not seen.

After those, disk usage (skip with --no-disk): the project's node_modules, the
tool's cache, the two together, and what a second copy of the same project
adds on top. Sizes are blocks on disk with every file counted once however
many names it has, which is what hardlinking from a shared store saves.

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
file records the path of the opal binary it ran, so the run is refused when
that binary sits in a temporary folder and the results would go into the
repository: put the binary somewhere you are happy to publish, or point
--results elsewhere for a run you won't commit.

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
import tempfile
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
# What each scenario removes before it runs. `ci-cached` and `add` leave the
# tree the way `noop` needs it and the disk measurement expects it, so `add`,
# which changes the project, goes last.
SCENARIOS = {
    "cold": {"lockfile", "cache", "node_modules"},
    "ci": {"cache", "node_modules"},
    "warm": {"node_modules"},
    "ci-cached": {"node_modules"},
    "noop": set(),
    "add": set(),
}
DEFAULT_ROUNDS = {"cold": 3, "ci": 3, "warm": 5, "ci-cached": 5, "noop": 5, "add": 3}
# One per `add` round, pinned so every tool resolves the same thing. None has
# dependencies, so a round adds exactly one package.
ADDED = [("mitt", "3.0.1"), ("dayjs", "1.11.13"), ("kleur", "4.1.5"),
         ("klona", "2.0.6"), ("dequal", "2.0.3")]
RUN_TIMEOUT = 30 * 60


def tool_table(opal, pnpm, cache):
    return {
        "opal": {
            "cmd": [opal, "install", "--cache-dir", cache / "opal"],
            "frozen": [opal, "install", "--frozen-lockfile", "--cache-dir", cache / "opal"],
            "add": [opal, "add", "--cache-dir", cache / "opal"],
            "lockfiles": ["opal.lock"],
            "caches": [cache / "opal"],
            "version": [opal, "--version"],
        },
        "npm": {
            "cmd": ["npm", "install", "--cache", cache / "npm", "--ignore-scripts",
                    "--no-audit", "--no-fund", "--no-update-notifier"],
            "frozen": ["npm", "ci", "--cache", cache / "npm", "--ignore-scripts",
                       "--no-audit", "--no-fund", "--no-update-notifier"],
            "add": ["npm", "install", "--cache", cache / "npm", "--ignore-scripts",
                    "--no-audit", "--no-fund", "--no-update-notifier"],
            "lockfiles": ["package-lock.json"],
            "caches": [cache / "npm"],
            "version": ["npm", "--version"],
        },
        "pnpm": {
            "cmd": [*pnpm, "install", "--store-dir", cache / "pnpm-store",
                    "--cache-dir", cache / "pnpm-cache", "--ignore-scripts",
                    "--config.update-notifier=false"],
            "frozen": [*pnpm, "install", "--frozen-lockfile", "--store-dir", cache / "pnpm-store",
                       "--cache-dir", cache / "pnpm-cache", "--ignore-scripts",
                       "--config.update-notifier=false"],
            "add": [*pnpm, "add", "--store-dir", cache / "pnpm-store",
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
            "frozen": ["yarn", "install", "--frozen-lockfile", "--cache-folder", cache / "yarn",
                       "--ignore-scripts", "--non-interactive"],
            "add": ["yarn", "add", "--cache-folder", cache / "yarn",
                    "--ignore-scripts", "--non-interactive"],
            "lockfiles": ["yarn.lock"],
            "caches": [cache / "yarn"],
            "version": ["yarn", "--version"],
            "files": {".yarnrc": "disable-self-update-check true\n"},
        },
        "bun": {
            "cmd": ["bun", "install", "--ignore-scripts"],
            "frozen": ["bun", "install", "--frozen-lockfile", "--ignore-scripts"],
            "add": ["bun", "add", "--ignore-scripts"],
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


def disk_bytes(paths):
    """Bytes on disk under `paths`, each file counted once.

    Blocks, not lengths, so thousands of small files cost what they cost; and
    by inode, so a file hardlinked into ten places is one file. Symlinks are
    not followed.
    """
    seen = set()
    total = 0
    stack = [Path(p) for p in paths if Path(p).exists()]
    while stack:
        path = stack.pop()
        try:
            status = os.lstat(path)
        except OSError:
            continue
        key = (status.st_dev, status.st_ino)
        if key not in seen:
            seen.add(key)
            total += status.st_blocks * 512
        if os.path.isdir(path) and not os.path.islink(path):
            try:
                stack.extend(Path(entry.path) for entry in os.scandir(path))
            except OSError:
                pass
    return total


def megabytes(count):
    return f"{count / (1024 * 1024):.1f} MB"


def summarize(samples):
    if not samples:
        return "-"
    median = statistics.median(samples)
    if len(samples) == 1:
        return fmt(median)
    return f"{fmt(median)} ({fmt(min(samples))}–{fmt(max(samples))})"


def fmt(seconds):
    return f"{seconds * 1000:.0f}ms" if seconds < 1 else f"{seconds:.2f}s"


def measure_disk(names, tools, work, base_env, logs):
    """Sizes for each tool's installed project, and for a second copy of it.

    The second copy is the same package.json and lockfile installed beside the
    first from the same cache: what one more project costs on a machine that
    already has one. A tool that copies pays for the whole tree again; one
    that links from a shared store pays for little more than the links.
    """
    disk = {}
    for name in names:
        tool = tools[name]
        first = work / name
        second = work / f"{name}-second"
        second.mkdir()
        for file in ["package.json", *tool["lockfiles"], *tool.get("files", {})]:
            if (first / file).exists():
                shutil.copy2(first / file, second / file)
        env = {**base_env, **tool.get("env", {})}
        _, _, code = timed(tool["cmd"], second, env, logs / f"disk-second-{name}.log")
        one = disk_bytes([first / "node_modules", *tool["caches"]])
        both = disk_bytes([first / "node_modules", second / "node_modules", *tool["caches"]])
        disk[name] = {
            "node_modules": disk_bytes([first / "node_modules"]),
            "cache": disk_bytes(tool["caches"]),
            "project_and_cache": one,
            "second_project_adds": both - one,
            "second_install_exit": code,
        }
        print(f"disk      {name:<5} node_modules {megabytes(disk[name]['node_modules']):>10}  "
              f"cache {megabytes(disk[name]['cache']):>10}  "
              f"second copy adds {megabytes(disk[name]['second_project_adds']):>10}", flush=True)
        remove(second)
    return disk


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
    parser.add_argument("--no-disk", action="store_true")
    parser.add_argument("--work", default="/tmp/opal-compare")
    parser.add_argument("--results", default=str(Path(__file__).resolve().parent / "results"))
    args = parser.parse_args()

    binary = Path(args.opal).resolve()
    scratch = Path(tempfile.gettempdir()).resolve()
    if args.results == parser.get_default("results") and binary.is_relative_to(scratch):
        sys.exit(f"compare-pms: {binary} is under {scratch}, and its path is recorded in the "
                 f"results, which default to the repository ({args.results}).\n"
                 f"Move the binary to a folder you are happy to publish (e.g. ~/opal-bench/), "
                 f"or pass --results DIR for a run you won't commit.")

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
    # The scratch directory is shown as <work>: where it was says nothing about
    # the run, and it would put a machine's temporary path in a published file.
    meta["commands"] = {name: " ".join(map(str, tools[name]["cmd"])).replace(str(work), "<work>")
                        for name in names}
    meta["add_commands"] = {
        name: " ".join(map(str, [*tools[name]["add"], "<package>@<version>"]))
              .replace(str(work), "<work>")
        for name in names}
    print(json.dumps({k: v for k, v in meta.items() if k != "memory"}, indent=2))
    print(meta["memory"])

    if rounds.get("add", 0) > len(ADDED):
        sys.exit(f"compare-pms: add has {len(ADDED)} packages to add, one per round")

    results = []
    disk = {}
    for scenario, removed in SCENARIOS.items():
        # Measured on the project every scenario above installed, before `add`
        # changes it.
        if scenario == "add" and not args.no_disk:
            disk = measure_disk(names, tools, work, base_env, logs)
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
                if scenario == "add":
                    command = [*tool["add"], "@".join(ADDED[round_index])]
                else:
                    command = tool["frozen"] if scenario == "ci-cached" else tool["cmd"]
                wall, rss, code = timed(command, project_dir, env, log)
                check = subprocess.run(["node", "-e", project["check"]], cwd=project_dir,
                                       capture_output=True).returncode == 0
                sample = {"scenario": scenario, "round": round_index + 1, "tool": name,
                          "wall": wall, "rss_mb": rss, "exit": code, "runs": check,
                          "packages": count_packages(project_dir / "node_modules")}
                if name == "opal":
                    sample["summary"] = next((line for line in log.read_text().splitlines()
                                              if " packages " in line and
                                              " installed" in line), "")
                results.append(sample)
                if scenario == "add":
                    sample["added"] = "@".join(ADDED[round_index])
                    # The point of the scenario: the package has to be there.
                    check = check and (project_dir / "node_modules" / ADDED[round_index][0]
                                       / "package.json").exists()
                    sample["runs"] = check
                status = "ok" if code == 0 and check else f"FAIL exit={code} runs={check}"
                print(f"{scenario:<9} #{round_index + 1} {name:<5} {fmt(wall):>9}  "
                      f"{rss:6.0f} MB  {sample['packages']:4} pkgs  {status}  "
                      f"{sample.get('summary', '')}", flush=True)

    results_dir = Path(args.results)
    results_dir.mkdir(parents=True, exist_ok=True)
    out = results_dir / f"{args.project}-{datetime.now():%Y%m%d-%H%M%S}.json"
    out.write_text(json.dumps({"meta": meta, "results": results, "disk": disk}, indent=2))

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
        # `add` grows the tree by one each round; the column is the project's own size.
        packages = sorted({r["packages"] for r in mine if r["scenario"] != "add"})
        failures = sum(1 for r in mine if r["exit"] != 0 or not r["runs"])
        label = f"{name} {meta['versions'][name]}" + (f" ({failures} failed)" if failures else "")
        print(f"| {label} | " + " | ".join(cells) + f" | {rss} | "
              + "/".join(map(str, packages)) + " |")
    if disk:
        print(f"\n{args.project}: disk usage, each file counted once\n")
        print("| Tool | node_modules | Cache | One project, with its cache | A second copy adds |")
        print("|---|---|---|---|---|")
        for name in names:
            sizes = disk[name]
            print(f"| {name} {meta['versions'][name]} | {megabytes(sizes['node_modules'])} | "
                  f"{megabytes(sizes['cache'])} | {megabytes(sizes['project_and_cache'])} | "
                  f"{megabytes(sizes['second_project_adds'])} |")
    print(f"\nraw samples: {out}")


if __name__ == "__main__":
    sys.exit(main())
