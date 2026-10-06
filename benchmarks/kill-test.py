#!/usr/bin/env python3
"""SIGKILL real `opal install` runs against the public registry, then check
that running it again finishes the job.

    python3 benchmarks/kill-test.py <express|next> [--opal PATH] [--trials 20] [--seed N]

The crash-safety suite in the repository kills installs of small fixture
packages served from a local folder. This is the same claim made against the
real thing: a real project, the real registry, real downloads, and a kill that
arrives whenever it arrives.

One clean install is the reference. Each trial then starts from an empty
project and an empty cache, runs `opal install`, and kills it after a random
delay somewhere inside the time the clean install took (and a little past it,
so a run that finished first is sampled too). A trial is killed one to three
times in a row, so a kill can land in the recovery from the one before. After
every kill `opal cache verify` must find every stored object matching its key.
Then `opal install` runs to completion, and the result must equal the
reference: the same opal.lock, and the same node_modules file for file, with
the same contents, modes, and symlink targets.

A kill that arrives after the install has exited tested nothing, so kills are
counted by where they landed, read from what was on disk at that moment:
  resolving   no opal.lock yet
  fetching    opal.lock written, node_modules not started
  linking     node_modules has at least one entry
  finished    the install had already exited
The number worth quoting is the kills that interrupted a running install, not
the number of trials.

What this does not cover: a kill of the machine rather than the process (power
loss, where what reached the disk depends on fsync), a full disk, and two
installs racing. The reference and the trials resolve minutes apart against a
registry that can publish in between; a new release inside a floating range
would show up as a lockfile difference in every later trial, and is reported
as a mismatch rather than explained away.

Needs Python 3.9+, network access, and node on PATH. Results are written as
JSON beside this script under results/ (--results), with the SHA-256 of the
binary that ran.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import platform
import random
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from collections import Counter
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("compare_pms", HERE / "compare-pms.py")
compare_pms = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(compare_pms)
PROJECTS, remove = compare_pms.PROJECTS, compare_pms.remove

RUN_TIMEOUT = 30 * 60


def snapshot(project):
    """Every path under node_modules, plus opal.lock, with what it holds."""
    found = {}
    lock = project / "opal.lock"
    found["opal.lock"] = hashlib.sha256(lock.read_bytes()).hexdigest() if lock.exists() else None
    root = project / "node_modules"
    stack = [root]
    while stack:
        directory = stack.pop()
        try:
            entries = list(os.scandir(directory))
        except OSError:
            continue
        for entry in entries:
            path = Path(entry.path)
            name = str(path.relative_to(project))
            status = entry.stat(follow_symlinks=False)
            if stat.S_ISLNK(status.st_mode):
                found[name] = ("link", os.readlink(path))
            elif stat.S_ISDIR(status.st_mode):
                found[name] = ("dir",)
                stack.append(path)
            else:
                executable = bool(status.st_mode & 0o111)
                found[name] = ("file", hashlib.sha256(path.read_bytes()).hexdigest(), executable)
    return found


def differences(expected, actual, limit=8):
    paths = sorted(p for p in expected.keys() | actual.keys() if expected.get(p) != actual.get(p))
    described = []
    for path in paths[:limit]:
        if path not in actual:
            described.append(f"missing: {path}")
        elif path not in expected:
            described.append(f"extra: {path}")
        else:
            described.append(f"differs: {path}")
    return len(paths), described


def landed(project, exited):
    if exited:
        return "finished"
    if not (project / "opal.lock").exists():
        return "resolving"
    node_modules = project / "node_modules"
    try:
        # The install lock lives in node_modules, so the directory exists
        # from the start; a package or scope directory is what linking adds.
        started = any(not entry.name.startswith(".") for entry in os.scandir(node_modules))
    except OSError:
        started = False
    return "linking" if started else "fetching"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("project", choices=PROJECTS)
    parser.add_argument("--opal", default=None)
    parser.add_argument("--trials", type=int, default=20)
    parser.add_argument("--seed", type=int, default=None)
    parser.add_argument("--work", default="/tmp/opal-kill-test")
    parser.add_argument("--results", default=str(HERE / "results"))
    args = parser.parse_args()

    opal = Path(args.opal or shutil.which("opal") or sys.exit("kill-test: no opal on PATH")).resolve()
    scratch = Path(tempfile.gettempdir()).resolve()
    if args.results == parser.get_default("results") and opal.is_relative_to(scratch):
        sys.exit(f"kill-test: {opal} is under {scratch}, and its path is recorded in the results, "
                 f"which default to the repository. Move the binary, or pass --results DIR.")
    seed = args.seed if args.seed is not None else random.SystemRandom().randrange(2**32)
    rng = random.Random(seed)

    project = PROJECTS[args.project]
    manifest_text = json.dumps(project["manifest"], indent=2) + "\n"
    work = Path(args.work) / args.project
    remove(work)
    work.mkdir(parents=True)
    env = {k: v for k, v in os.environ.items() if k not in ("CI", "OPAL_CACHE_DIR")}

    def fresh(name):
        directory = work / name
        (directory / "project").mkdir(parents=True)
        (directory / "project" / "package.json").write_text(manifest_text)
        return directory / "project", directory / "cache"

    def install(project_dir, cache):
        return [str(opal), "install", "--root", str(project_dir), "--cache-dir", str(cache)]

    def verified(cache):
        if not cache.exists():
            return True
        result = subprocess.run([str(opal), "cache", "verify", "--cache-dir", str(cache)],
                                capture_output=True, text=True, env=env)
        return result.returncode == 0

    reference_project, reference_cache = fresh("reference")
    started = time.perf_counter()
    done = subprocess.run(install(reference_project, reference_cache), capture_output=True,
                          text=True, env=env, timeout=RUN_TIMEOUT)
    clean_seconds = time.perf_counter() - started
    if done.returncode != 0:
        sys.exit(f"kill-test: the reference install failed:\n{done.stderr}")
    expected = snapshot(reference_project)
    window = clean_seconds * 1.25
    meta = {
        "project": args.project,
        "date": datetime.now().isoformat(timespec="seconds"),
        "host": f"{platform.system()} {platform.release()}, {os.cpu_count()} threads",
        "seed": seed,
        "trials": args.trials,
        "opal_binary": {"path": str(opal), "sha256": hashlib.sha256(opal.read_bytes()).hexdigest()},
        "opal_version": subprocess.run([str(opal), "--version"], capture_output=True,
                                       text=True).stdout.strip(),
        "clean_install_seconds": round(clean_seconds, 3),
        "reference_paths": len(expected),
    }
    print(json.dumps(meta, indent=2), flush=True)

    trials = []
    for index in range(args.trials):
        project_dir, cache = fresh(f"trial-{index + 1}")
        delays = [rng.uniform(0, window) for _ in range(rng.randint(1, 3))]
        kills, clean_cache = [], True
        for delay in delays:
            child = subprocess.Popen(install(project_dir, cache), env=env,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                child.wait(timeout=delay)
                exited = True
            except subprocess.TimeoutExpired:
                exited = False
                where = landed(project_dir, False)
                child.send_signal(signal.SIGKILL)
                child.wait()
            if exited:
                where = "finished"
            kills.append({"after_seconds": round(delay, 3), "landed": where})
            clean_cache = verified(cache) and clean_cache

        final = subprocess.run(install(project_dir, cache), capture_output=True, text=True,
                               env=env, timeout=RUN_TIMEOUT)
        clean_cache = verified(cache) and clean_cache
        runs = subprocess.run(["node", "-e", project["check"]], cwd=project_dir,
                              capture_output=True).returncode == 0
        differing, described = differences(expected, snapshot(project_dir))
        trial = {
            "trial": index + 1,
            "kills": kills,
            "recovery_exit": final.returncode,
            "cache_verified": clean_cache,
            "runs": runs,
            "differing_paths": differing,
            "differences": described,
        }
        trial["converged"] = (final.returncode == 0 and clean_cache and runs and differing == 0)
        trials.append(trial)
        where = ", ".join(f"{kill['landed']}@{kill['after_seconds']}s" for kill in kills)
        print(f"trial {index + 1:>3}  {'converged' if trial['converged'] else 'FAILED':<9}  {where}",
              flush=True)
        if not trial["converged"]:
            print(f"           exit={final.returncode} cache_verified={clean_cache} runs={runs} "
                  f"differing_paths={differing} {described}", flush=True)
            if final.returncode != 0:
                print(final.stderr.strip()[-600:], flush=True)
        else:
            # A failed trial's folders are kept to look at; the rest are not.
            remove(project_dir.parent)

    counts = Counter(kill["landed"] for trial in trials for kill in trial["kills"])
    total = sum(counts.values())
    interrupted = total - counts["finished"]
    converged = sum(trial["converged"] for trial in trials)
    summary = {"kills": total, "interrupted": interrupted, "landed": dict(counts),
               "trials_converged": converged}
    results_dir = Path(args.results)
    results_dir.mkdir(parents=True, exist_ok=True)
    out = results_dir / f"kill-{args.project}-{datetime.now():%Y%m%d-%H%M%S}.json"
    out.write_text(json.dumps({"meta": meta, "summary": summary, "trials": trials}, indent=2))

    print(f"\n{args.project}: {converged} of {len(trials)} trials converged on the clean install")
    print(f"{total} kills, {interrupted} interrupted a running install: "
          + ", ".join(f"{name} {counts[name]}" for name in
                      ("resolving", "fetching", "linking", "finished")))
    print(f"seed {seed} · raw results: {out}")
    return 0 if converged == len(trials) else 1


if __name__ == "__main__":
    sys.exit(main())
