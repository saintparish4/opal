#!/usr/bin/env python3
"""N `opal install`s at once, into N copies of one project, on one empty store.

    python3 benchmarks/concurrent-installs.py <express|next> --opal PATH [--n 1,2,4,8,16,32]

The shared store is the architecture's bet, and the test suite had run no
more than two installs against it at once. For each N this starts N installs
together, each in its own copy of the project, all with one empty
OPAL_CACHE_DIR, all through registry-proxy.py so every request is counted.
Then it records:
  wall          until the last install exited, and against N = 1
  peak RSS      each install's, summed, read through GNU time (see below)
  verify        `opal cache verify` on the store afterwards
  trees         how many of the N node_modules equal a reference install's,
                file for file (kill-test.py's comparison), and opal.lock with it
  store         the cache's size on disk, each file counted once
  tarballs/pkg  tarball requests over distinct tarballs: 1.0 means each was
                downloaded once for the whole run, N means every install
                downloaded everything
  metadata/pkg  the same for packuments

Two things it is there to answer, predicted before it ran (base/release/
v041.md, Step 8):
  - Installs take the store's lock shared and hold it to the end, so they
    never wait on each other there. Wall should grow like each install's share
    of the bandwidth, not like N times one install.
  - Opal dedupes downloads inside one process only, so N installs started
    together into an empty store should make close to N times the tarball
    requests. Correct, since storing identical bytes twice is a no-op, but
    wasteful.

The reference install runs first, alone, with its own cache, through the same
proxy (lockfiles record tarball URLs, so the address has to be the same).
The registry can publish between the reference and a later N; that shows as
a tree difference and is reported, not explained away.

Peak memory goes through GNU time (`/usr/bin/time -f %M`), which forks the
install itself. On Linux, `exec` carries a parent's peak into its child's
ru_maxrss, so read straight from wait4 here every install would report at
least this harness's own size, the proxy's threads and buffers included:
`/bin/true` started by a 500 MB process reads 511 MB. Without GNU time the
peaks are left out rather than reported wrong.

Needs Python 3.9+ and network access. Results go to benchmarks/results/
(--results) as JSON with the SHA-256 of the binary that ran. Scratch is a
fresh folder under the system temp folder, deleted afterwards unless --keep.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
# Loading the sibling scripts would otherwise leave a __pycache__ beside them.
sys.dont_write_bytecode = True


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


compare_pms = load("compare_pms", "compare-pms.py")
kill_test = load("kill_test", "kill-test.py")
registry_proxy = load("registry_proxy", "registry-proxy.py")
PROJECTS, remove, fmt = compare_pms.PROJECTS, compare_pms.remove, compare_pms.fmt
disk_bytes, snapshot, differences = compare_pms.disk_bytes, kill_test.snapshot, kill_test.differences

RUN_TIMEOUT = 30 * 60


def project_copy(directory, manifest_text):
    directory.mkdir(parents=True)
    (directory / "package.json").write_text(manifest_text)
    return directory


def gnu_time():
    try:
        version = subprocess.run(["/usr/bin/time", "--version"], capture_output=True, text=True)
    except OSError:
        return None
    return "/usr/bin/time" if "GNU" in version.stdout + version.stderr else None


def tail(log, lines=15):
    """A failed install's last lines, without colour codes."""
    text = re.sub(r"\x1b\[[0-9;]*m", "", log.read_text(errors="replace"))
    return text.splitlines()[-lines:]


def install_together(command, projects, env, logs, timer):
    """Starts `command` in every project at once; (seconds to the last exit,
    [(seconds, peak MB or None, exit code)] in project order)."""
    started = time.perf_counter()
    children = {}
    for index, project in enumerate(projects):
        log = open(logs / f"install-{index}.log", "w")
        peak = logs / f"peak-{index}"
        timed = [timer, "-f", "%M", "-o", str(peak), *command] if timer else command
        child = subprocess.Popen(timed, cwd=project, env=env,
                                 stdout=log, stderr=subprocess.STDOUT)
        children[child.pid] = (index, child, log, peak)
    finished = [None] * len(projects)
    deadline = started + RUN_TIMEOUT
    while children:
        for pid in list(children):
            reaped, status, _ = os.wait4(pid, os.WNOHANG)
            if not reaped:
                continue
            index, child, log, peak = children.pop(pid)
            log.close()
            kilobytes = peak.read_text().split()[-1] if timer and peak.exists() else None
            finished[index] = (time.perf_counter() - started,
                               int(kilobytes) / 1024 if kilobytes else None,
                               os.waitstatus_to_exitcode(status))
        if time.perf_counter() > deadline:
            for _, child, _, _ in children.values():
                child.kill()
            deadline = float("inf")
        time.sleep(0.005)
    return time.perf_counter() - started, finished


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("project", choices=PROJECTS)
    parser.add_argument("--opal", required=True)
    parser.add_argument("--n", default="1,2,4,8,16,32")
    parser.add_argument("--upstream", default="https://registry.npmjs.org")
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--work", default=tempfile.gettempdir())
    parser.add_argument("--results", default=str(HERE / "results"))
    args = parser.parse_args()

    opal = Path(args.opal).resolve()
    scratch = Path(tempfile.gettempdir()).resolve()
    if args.results == parser.get_default("results") and opal.is_relative_to(scratch):
        sys.exit(f"concurrent-installs: {opal} is under {scratch}, and its path is recorded in "
                 f"the results, which default to the repository. Move the binary, or pass "
                 f"--results DIR.")
    counts = [int(n) for n in args.n.split(",")]
    manifest_text = json.dumps(PROJECTS[args.project]["manifest"], indent=2) + "\n"
    base_env = {k: v for k, v in os.environ.items() if k not in ("CI", "OPAL_CACHE_DIR")}

    meta = {
        "project": args.project,
        "date": datetime.now().isoformat(timespec="seconds"),
        "host": f"{platform.system()} {platform.release()}, {os.cpu_count()} threads",
        "opal_version": subprocess.run([str(opal), "--version"], capture_output=True,
                                       text=True).stdout.strip().removeprefix("opal "),
        "opal_binary": {"path": str(opal), "sha256": hashlib.sha256(opal.read_bytes()).hexdigest()},
        "upstream": args.upstream,
        "n": counts,
    }
    print(json.dumps(meta, indent=2), flush=True)

    timer = gnu_time()
    if not timer:
        print("concurrent-installs: no GNU time at /usr/bin/time, so peak memory is left out",
              flush=True)
    meta["peak_memory"] = "GNU time, %M" if timer else None

    work = Path(tempfile.mkdtemp(prefix="opal-concurrent-", dir=args.work)).resolve()
    rows = []
    try:
        with registry_proxy.Proxy(args.upstream) as proxy:
            registry = ["--registry", proxy.url]
            reference_dir = project_copy(work / "reference" / "project", manifest_text)
            env = {**base_env, "OPAL_CACHE_DIR": str(work / "reference" / "cache")}
            done = subprocess.run([str(opal), "install", *registry], cwd=reference_dir, env=env,
                                  capture_output=True, text=True)
            if done.returncode != 0:
                sys.exit(f"concurrent-installs: the reference install failed:\n{done.stdout}{done.stderr}")
            reference = snapshot(reference_dir)
            proxy.reset()
            print(f"reference: {sum(1 for v in reference.values() if v and v[0] == 'file')} files",
                  flush=True)

            for n in counts:
                run_dir = work / f"n{n}"
                cache = run_dir / "cache"
                logs = run_dir / "logs"
                logs.mkdir(parents=True)
                projects = [project_copy(run_dir / f"project-{i}", manifest_text) for i in range(n)]
                env = {**base_env, "OPAL_CACHE_DIR": str(cache)}
                wall, installs = install_together([str(opal), "install", *registry], projects,
                                                  env, logs, timer)
                counted = registry_proxy.totals(proxy.reset())
                verify = subprocess.run([str(opal), "cache", "verify", "--cache-dir", str(cache)],
                                        capture_output=True, text=True)
                mismatched = {}
                for index, project in enumerate(projects):
                    count, described = differences(reference, snapshot(project))
                    if count:
                        mismatched[index] = {"count": count, "first": described}
                tarballs, packuments = counted["tarball"], counted["packument"]
                row = {
                    "n": n,
                    "wall_s": round(wall, 3),
                    "installs": [{"wall_s": round(s, 3),
                                  "peak_mb": round(mb, 1) if mb is not None else None, "exit": code}
                                 for s, mb, code in installs],
                    "failed": sum(1 for _, _, code in installs if code != 0),
                    "peak_mb_sum": (round(sum(mb for _, mb, _ in installs), 1)
                                    if all(mb is not None for _, mb, _ in installs) else None),
                    "verify_clean": verify.returncode == 0,
                    "verify_output": (verify.stdout + verify.stderr).strip().splitlines()[-5:],
                    "failures": {index: tail(logs / f"install-{index}.log")
                                 for index, (_, _, code) in enumerate(installs) if code != 0},
                    "trees_equal": n - len(mismatched),
                    "tree_differences": mismatched,
                    "store_bytes": disk_bytes([cache]),
                    "requests": counted,
                    "tarballs_per_package": round(tarballs["requests"] / max(tarballs["distinct"], 1), 2),
                    "packuments_per_package": round(packuments["requests"] / max(packuments["distinct"], 1), 2),
                }
                rows.append(row)
                peak = f"{row['peak_mb_sum']:.0f} MB" if row["peak_mb_sum"] is not None else "-"
                print(f"N={n:<3} wall {fmt(wall):>8}  failed {row['failed']}  "
                      f"peak RSS sum {peak}  verify "
                      f"{'clean' if row['verify_clean'] else 'NOT CLEAN'}  trees {row['trees_equal']}/{n}  "
                      f"store {row['store_bytes'] / 2**20:.0f} MB  tarballs/pkg {row['tarballs_per_package']}  "
                      f"metadata/pkg {row['packuments_per_package']}", flush=True)
                remove(run_dir)
    finally:
        if args.keep:
            print(f"\nscratch kept: {work}")
        else:
            remove(work)

    one = next((row["wall_s"] for row in rows if row["n"] == 1), None)
    print(f"\nConcurrent installs: opal {meta['opal_version']} "
          f"(SHA-256 {meta['opal_binary']['sha256']}), {args.project}, {meta['date']}\n")
    print("| N | Wall | vs N = 1 | Failed | Peak RSS, summed | `cache verify` | Trees equal | Store | Tarball requests per tarball | Packument requests per package |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for row in rows:
        ratio = f"{row['wall_s'] / one:.2f}×" if one else "-"
        peak = f"{row['peak_mb_sum']:.0f} MB" if row["peak_mb_sum"] is not None else "-"
        print(f"| {row['n']} | {fmt(row['wall_s'])} | {ratio} | {row['failed']} | {peak} "
              f"| {'clean' if row['verify_clean'] else 'not clean'} | {row['trees_equal']} of {row['n']} "
              f"| {row['store_bytes'] / 2**20:.0f} MB | {row['tarballs_per_package']} "
              f"| {row['packuments_per_package']} |")

    results = Path(args.results)
    results.mkdir(parents=True, exist_ok=True)
    out = results / f"concurrent-{args.project}-{datetime.now():%Y%m%d-%H%M%S}.json"
    out.write_text(json.dumps({"meta": meta, "rows": rows}, indent=2) + "\n")
    print(f"\nraw results: {out}")
    bad = [row for row in rows if row["failed"] or not row["verify_clean"]
           or row["trees_equal"] != row["n"]]
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
