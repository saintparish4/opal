#!/usr/bin/env python3
"""Opal's runtime against node and bun, with the floor beside them.

    python3 benchmarks/compare-runtimes.py bench [--opal PATH] [--engine NAME=PATH ...] [--rounds 10]
    python3 benchmarks/compare-runtimes.py gate --opal PATH [--budget benchmarks/runtime-budget.json] [--rebase]

`bench` answers two questions, separately:

  startup  how long `hello.js` takes, start to exit, which is nearly all
           process start: exec, the dynamic loader, engine and bootstrap
           setup, and teardown. The controls run in the same rounds: an empty
           statically linked C program (`floor`, the least any process costs
           on this machine) and `opal --version` (opal's own start with no
           runtime work). Ratios to bun and node are what the targets are
           stated in, because absolute times are a property of the machine.
  steady   how fast code runs once started: a recursive numeric workload
           (fib), JSON stringify and parse, and a regex scan. A runtime
           without a JIT can win startup and lose these by an order of
           magnitude; reporting only hello world would hide that.

Every workload prints one deterministic line, and a run counts only if its
output is the line node prints. Each round runs every command once through
`hyperfine -N` (no shell between it and the command), starting from a
different command each round, so a swing in machine state lands on every
command instead of whichever ran then. Reported: median and min of every
hyperfine sample across the rounds, never the mean, which one slow outlier
moves. Beside the times: user and system CPU, minor page faults and peak RSS
from wait4, taken the way `/usr/bin/time -v` takes them (through a small C
shim that forks the command, see SHIM; it agrees with `time -v` to within a
few faults), syscalls from `strace -f -c` (Linux only), and the executable's
size on disk.

`gate` is for CI, where wall time is too noisy to fail a build on. It counts
syscalls and minor page faults of opal commands, subtracts the same counts
for the floor (which absorbs kernel and libc differences between machines),
and fails when a command goes over its budget in runtime-budget.json.
`--rebase` writes the measured counts plus headroom back to the budget.

Versions of node and bun are pinned (PINNED below): a different version is
refused unless --unpinned, because a ratio to "bun" means a ratio to one bun.
Raw samples are written as JSON to benchmarks/results/ (--results), so a
published table can be committed with the samples it was computed from. As
in compare-pms.py, the opal binary's path and SHA-256 are recorded, and a
binary in a temporary folder is refused when the results would go into the
repository.

Needs Python 3.9+, hyperfine, a C compiler for the floor, node and bun on
PATH, and strace on Linux for the syscall counts.
"""

import argparse
import hashlib
import json
import math
import os
import platform
import shlex
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from datetime import datetime
from pathlib import Path

PINNED = {"node": "v24.19.0", "bun": "1.4.2"}

# Each prints one line, and only through console.log, so that an engine spike
# with nothing else bound can run them all.
WORKLOADS = {
    # The program bun.com's startup table times.
    "hello": 'console.log("hello")\n',
    # ~1.6M calls: call overhead and integer arithmetic.
    "fib": (
        "function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2) }\n"
        "console.log(fib(30))\n"
    ),
    # Allocation, property access, and the JSON codec, ~2 MB of text each way.
    "json": (
        "const rows = []\n"
        "for (let i = 0; i < 20000; i++) {\n"
        "  rows.push({ id: i, name: 'row' + i, tags: ['a', 'b', String(i % 7)], ok: i % 3 === 0 })\n"
        "}\n"
        "let total = 0\n"
        "for (let round = 0; round < 5; round++) {\n"
        "  const text = JSON.stringify(rows)\n"
        "  const back = JSON.parse(text)\n"
        "  total += text.length + back.length\n"
        "}\n"
        "console.log(total)\n"
    ),
    # The regex engine over ~1 MB of synthetic log lines.
    "regex": (
        "const lines = []\n"
        "for (let i = 0; i < 20000; i++) {\n"
        "  lines.push('2026-10-24 12:' + (i % 60) + ' GET /item/' + i + ' from 10.' + (i % 256) + '.' + (i % 17) + '.' + (i % 9) + ' status=' + (200 + i % 5))\n"
        "}\n"
        "const text = lines.join('\\n')\n"
        "let hits = 0\n"
        "for (let round = 0; round < 5; round++) {\n"
        "  const re = /(\\d+)\\.(\\d+)\\.(\\d+)\\.(\\d+) status=(\\d+)/g\n"
        "  for (const m of text.matchAll(re)) { if (m[5] === '200') hits++ }\n"
        "}\n"
        "console.log(hits)\n"
    ),
}
STARTUP = "hello"
CONTROLS = ("floor", "opal-version")
# hyperfine samples per command per round; the steady workloads run longer,
# so fewer of them buy the same confidence.
RUNS = {"hello": (5, 20), "fib": (1, 3), "json": (1, 3), "regex": (1, 3)}  # (warmup, runs)
CENSUS_REPEATS = 3
TIMEOUT = 120
SHIM_BINARY = None


def compile_c(work, name, source):
    """A C program, linked statically when the platform allows it."""
    (work / f"{name}.c").write_text(source)
    binary = work / name
    for flags, linkage in ((["-static"], "static"), ([], "dynamic")):
        built = subprocess.run(["cc", "-O2", *flags, "-o", binary, work / f"{name}.c"],
                               capture_output=True)
        if built.returncode == 0:
            return binary, linkage
    sys.exit(f"compare-runtimes: cc could not build {name}:\n{built.stderr.decode()}")


def floor_binary(work):
    return compile_c(work, "floor", "int main(void) { return 0; }\n")


# Linux keeps a process's peak RSS across exec, in accounting a fork starts
# fresh, so a command started straight from Python reports Python's own peak
# (~20 MB) when its own is smaller. The rusage runs go through this shim
# instead, which is what /usr/bin/time does: a tiny program that forks, execs
# the command in the child, and writes the child's wait4 rusage to the file
# named by its first argument. The peak carried into the command is the
# shim's, a few hundred KB, which the floor row shows.
SHIM = r"""
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>
int main(int argc, char **argv) {
    if (argc < 3) return 127;
    pid_t pid = fork();
    if (pid == 0) { execvp(argv[2], argv + 2); _exit(127); }
    int status; struct rusage use;
    if (pid < 0 || wait4(pid, &status, 0, &use) < 0) return 127;
    FILE *out = fopen(argv[1], "w");
    if (!out) return 127;
    fprintf(out, "%ld %ld %ld %ld %ld\n", use.ru_minflt, use.ru_majflt, use.ru_maxrss,
            (long)(use.ru_utime.tv_sec * 1000000 + use.ru_utime.tv_usec),
            (long)(use.ru_stime.tv_sec * 1000000 + use.ru_stime.tv_usec));
    fclose(out);
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
"""


def rusage_run(cmd, cwd, env):
    """One run: exit code, stdout, and the command's own rusage."""
    report = Path(SHIM_BINARY).parent / "rusage.txt"
    done = subprocess.run([str(SHIM_BINARY), report, *map(str, cmd)], cwd=cwd, env=env,
                          stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=TIMEOUT)
    minflt, majflt, maxrss, utime, stime = map(int, report.read_text().split())
    # ru_maxrss is kilobytes on Linux and bytes on macOS.
    per_mb = 1024 * 1024 if sys.platform == "darwin" else 1024
    return {
        "exit": done.returncode,
        "stdout": done.stdout.decode(errors="replace"),
        "user_ms": utime / 1000,
        "sys_ms": stime / 1000,
        "minor_faults": minflt,
        "major_faults": majflt,
        "rss_mb": maxrss / per_mb,
    }


def hyperfine(cmd, cwd, env, warmup, runs, scratch):
    out = scratch / "hyperfine.json"
    subprocess.run(["hyperfine", "-N", "--style", "none", "--warmup", str(warmup),
                    "--runs", str(runs), "--export-json", out, shlex.join(map(str, cmd))],
                   cwd=cwd, env=env, check=True, stdout=subprocess.DEVNULL,
                   timeout=TIMEOUT * (warmup + runs))
    return json.loads(out.read_text())["results"][0]["times"]


def census(cmd, cwd, env, scratch):
    """Syscalls made by the command and everything it starts: the total, and
    a count per syscall name."""
    out = scratch / "strace.txt"
    subprocess.run(["strace", "-f", "-c", "-U", "name,calls", "-o", out, "--",
                    *map(str, cmd)], cwd=cwd, env=env, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, timeout=TIMEOUT)
    counts = {}
    for line in out.read_text().splitlines():
        fields = line.split()
        if len(fields) == 2 and fields[1].isdigit():
            counts[fields[0]] = int(fields[1])
    total = counts.pop("total", sum(counts.values()))
    return total, counts


def median_census(cmd, cwd, env, scratch):
    runs = [census(cmd, cwd, env, scratch) for _ in range(CENSUS_REPEATS)]
    runs.sort(key=lambda run: run[0])
    return runs[len(runs) // 2]


def startup_trace(cmd, cwd, env):
    """opal's OPAL_TRACE_STARTUP timeline for one run, spawn included."""
    env = {**env, "OPAL_TRACE_STARTUP": "1",
           "OPAL_TRACE_SPAWN_NS": str(time.clock_gettime_ns(time.CLOCK_MONOTONIC))}
    done = subprocess.run([str(c) for c in cmd], cwd=cwd, env=env, capture_output=True,
                          text=True, timeout=TIMEOUT)
    return [line for line in done.stderr.splitlines() if line.startswith("opal-trace")]


def executable_size(cmd):
    path = shutil.which(str(cmd[0]))
    return os.path.getsize(os.path.realpath(path)) if path else None


def version(cmd):
    done = subprocess.run(cmd, capture_output=True, text=True)
    return done.stdout.strip().splitlines()[0] if done.returncode == 0 and done.stdout else "?"


def fmt_ms(value):
    if value is None:
        return "-"
    return f"{value:.1f} ms" if value < 1000 else f"{value / 1000:.2f} s"


def fmt_mb(count):
    return "-" if count is None else f"{count / (1024 * 1024):.1f} MB"


def work_dir(path):
    work = Path(path)
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    for name, source in WORKLOADS.items():
        (work / f"{name}.js").write_text(source)
    return work


def check_binary_publishable(parser, args):
    binary = Path(args.opal).resolve()
    scratch = Path(tempfile.gettempdir()).resolve()
    if args.results == parser.get_default("results") and binary.is_relative_to(scratch):
        sys.exit(f"compare-runtimes: {binary} is under {scratch}, and its path is recorded in "
                 f"the results, which default to the repository ({args.results}).\n"
                 f"Move the binary to a folder you are happy to publish, or pass --results DIR "
                 f"for a run you won't commit.")


def bench(args, parser):
    check_binary_publishable(parser, args)
    opal = Path(args.opal).resolve()
    work = work_dir(args.work)
    scratch = work / "scratch"
    scratch.mkdir()
    floor, linkage = floor_binary(scratch)
    global SHIM_BINARY
    SHIM_BINARY, _ = compile_c(scratch, "spawn", SHIM)
    env = {k: v for k, v in os.environ.items() if not k.startswith("OPAL_TRACE")}

    versions = {"node": version(["node", "--version"]), "bun": version(["bun", "--version"]),
                "opal": version([opal, "--version"]).removeprefix("opal ")}
    for tool, pinned in PINNED.items():
        if versions[tool] != pinned and not args.unpinned:
            sys.exit(f"compare-runtimes: {tool} is {versions[tool]}, the harness is pinned to "
                     f"{pinned}. Install that version, or pass --unpinned for a run whose "
                     f"ratios you won't compare with published ones.")

    def runtime_cmds(script):
        cmds = {"node": ["node", script], "bun": ["bun", script],
                "opal": [opal, "run", script]}
        for name, path in args.engine:
            cmds[name] = [path, script]
        return cmds

    expected = {}
    for name in WORKLOADS:
        done = subprocess.run(["node", f"{name}.js"], cwd=work, capture_output=True, text=True)
        expected[name] = done.stdout
    # A command that can't run a workload yet (opal before its engine lands)
    # is left out of that workload's table rather than failing the run.
    plan = {}
    skipped = {}
    for name in WORKLOADS:
        cmds = runtime_cmds(f"{name}.js")
        if name == STARTUP:
            cmds = {"floor": [floor], "opal-version": [opal, "--version"], **cmds}
        plan[name] = {}
        for tool, cmd in cmds.items():
            if tool in CONTROLS:
                plan[name][tool] = cmd
                continue
            probe = subprocess.run([str(c) for c in cmd], cwd=work, capture_output=True,
                                   text=True, env=env, timeout=TIMEOUT)
            if probe.returncode == 0 and probe.stdout == expected[name]:
                plan[name][tool] = cmd
            else:
                skipped.setdefault(name, []).append(
                    f"{tool} (exit {probe.returncode}, printed {probe.stdout.strip()[:40]!r})")

    meta = {
        "date": datetime.now().isoformat(timespec="seconds"),
        "host": f"{platform.system()} {platform.release()}, {os.cpu_count()} threads",
        "cpu": next((line.split(":", 1)[1].strip() for line in
                     Path("/proc/cpuinfo").read_text().splitlines()
                     if line.startswith("model name")), platform.processor())
               if Path("/proc/cpuinfo").exists() else platform.processor(),
        "versions": versions,
        "engines": {name: {"path": str(Path(path).resolve()),
                           "sha256": hashlib.sha256(Path(path).read_bytes()).hexdigest()}
                    for name, path in args.engine},
        "opal_binary": {"path": str(opal), "sha256": hashlib.sha256(opal.read_bytes()).hexdigest()},
        "floor": linkage,
        # Other work on the machine moves every command's time; interleaving
        # spreads it across them, and this says how much there was.
        "loadavg_start": os.getloadavg(),
        "rounds": args.rounds,
        "runs_per_round": {k: {"warmup": w, "runs": r} for k, (w, r) in RUNS.items()},
        "commands": {name: {tool: " ".join(map(str, cmd)).replace(str(work), "<work>")
                            .replace(str(scratch), "<work>")
                            for tool, cmd in cmds.items()} for name, cmds in plan.items()},
        "skipped": skipped,
    }
    print(json.dumps(meta, indent=2), flush=True)

    samples = []
    for round_index in range(args.rounds):
        for name, cmds in plan.items():
            tools = list(cmds)
            shift = round_index % len(tools)
            for tool in tools[shift:] + tools[:shift]:
                cmd = cmds[tool]
                warmup, runs = RUNS[name]
                times = hyperfine(cmd, work, env, warmup, runs, scratch)
                usage = rusage_run(cmd, work, env)
                ok = usage["exit"] == 0 and (tool in CONTROLS or usage["stdout"] == expected[name])
                usage.pop("stdout")
                samples.append({"workload": name, "round": round_index + 1, "tool": tool,
                                "times_ms": [t * 1000 for t in times], "ok": ok, **usage})
                print(f"round {round_index + 1:2} {name:<6} {tool:<13} "
                      f"median {statistics.median(times) * 1000:8.2f} ms  "
                      f"{'ok' if ok else 'FAIL'}", flush=True)

    censuses = {}
    if shutil.which("strace"):
        for name, cmds in plan.items():
            for tool, cmd in cmds.items():
                total, counts = median_census(cmd, work, env, scratch)
                censuses[f"{name}/{tool}"] = {"total": total, "calls": counts}
    traces = {name: startup_trace(cmds["opal"], work, env)
              for name, cmds in plan.items() if "opal" in cmds}
    traces["--version"] = startup_trace([opal, "--version"], work, env)
    sizes = {tool: executable_size(cmd) for cmds in plan.values() for tool, cmd in cmds.items()}

    meta["loadavg_end"] = os.getloadavg()
    results_dir = Path(args.results)
    results_dir.mkdir(parents=True, exist_ok=True)
    out = results_dir / f"runtimes-{datetime.now():%Y%m%d-%H%M%S}.json"
    out.write_text(json.dumps({"meta": meta, "samples": samples, "census": censuses,
                               "traces": traces, "sizes": sizes}, indent=2) + "\n")

    for name, cmds in plan.items():
        print_table(name, cmds, samples, censuses, sizes, meta)
    if skipped:
        print("\nNot measured (output didn't match node's, or the command failed):")
        for name, entries in skipped.items():
            print(f"  {name}: {', '.join(entries)}")
    for name, lines in traces.items():
        print(f"\nopal startup trace, {name}:")
        print("\n".join(lines) or "  (no checkpoints)")
    print(f"\nraw samples: {out}")


def summary(samples, name, tool):
    mine = [s for s in samples if s["workload"] == name and s["tool"] == tool and s["ok"]]
    times = [t for s in mine for t in s["times_ms"]]
    if not times:
        return None

    def med(key):
        return statistics.median(s[key] for s in mine)
    return {"median": statistics.median(times), "min": min(times), "n": len(times),
            "failed": sum(1 for s in samples if s["workload"] == name and s["tool"] == tool
                          and not s["ok"]),
            "user_ms": med("user_ms"), "sys_ms": med("sys_ms"),
            "minor_faults": med("minor_faults"), "rss_mb": med("rss_mb")}


def print_table(name, cmds, samples, censuses, sizes, meta):
    rows = {tool: summary(samples, name, tool) for tool in cmds}
    bun = rows.get("bun")
    node = rows.get("node")
    print(f"\n{name}: {meta['date']}, {meta['cpu']}, {meta['rounds']} rounds\n")
    print("| Command | Median | Min | ×bun | ×node | User | Sys | Minor faults "
          "| Peak RSS | Syscalls | Executable |")
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    for tool, row in rows.items():
        label = tool if tool in CONTROLS else f"{tool} {meta['versions'].get(tool, '')}".strip()
        if row is None:
            print(f"| {label} | failed | | | | | | | | | |")
            continue
        ratio_bun = f"{row['median'] / bun['median']:.2f}" if bun else "-"
        ratio_node = f"{row['median'] / node['median']:.2f}" if node else "-"
        syscalls = censuses.get(f"{name}/{tool}", {}).get("total", "-")
        failed = f" ({row['failed']} failed)" if row["failed"] else ""
        print(f"| {label}{failed} | {fmt_ms(row['median'])} | {fmt_ms(row['min'])} | "
              f"{ratio_bun} | {ratio_node} | {fmt_ms(row['user_ms'])} | {fmt_ms(row['sys_ms'])} | "
              f"{row['minor_faults']:.0f} | {row['rss_mb']:.1f} MB | {syscalls} | "
              f"{fmt_mb(sizes.get(tool))} |")
    opal = rows.get("opal")
    if name == STARTUP and opal and bun and node:
        to_bun = opal["median"] / bun["median"]
        to_node = opal["median"] / node["median"]
        print(f"\ntargets: opal/bun {to_bun:.2f} (≤ 1.00: {'met' if to_bun <= 1 else 'not met'}), "
              f"opal/node {to_node:.2f} (≤ 0.20: {'met' if to_node <= 0.2 else 'not met'})")


def gate(args):
    budget_path = Path(args.budget)
    budget = json.loads(budget_path.read_text())
    work = work_dir(args.work)
    scratch = work / "scratch"
    scratch.mkdir()
    for file, text in budget.get("files", {}).items():
        (work / file).parent.mkdir(parents=True, exist_ok=True)
        (work / file).write_text(text if isinstance(text, str) else json.dumps(text, indent=2))
    floor, _ = floor_binary(scratch)
    global SHIM_BINARY
    SHIM_BINARY, _ = compile_c(scratch, "spawn", SHIM)
    opal = Path(args.opal).resolve()
    env = {k: v for k, v in os.environ.items() if not k.startswith("OPAL_TRACE")}

    def counts(cmd):
        total, _ = median_census(cmd, work, env, scratch)
        faults = statistics.median(rusage_run(cmd, work, env)["minor_faults"] for _ in range(5))
        return total, faults

    floor_calls, floor_faults = counts([floor])
    print(f"floor: {floor_calls} syscalls, {floor_faults:.0f} minor faults\n")
    print("| Command | Syscalls above floor | Budget | Minor faults above floor | Budget | |")
    print("|---|---|---|---|---|---|")
    over = []
    for label, entry in budget["commands"].items():
        calls, faults = counts([opal, *entry["args"]])
        calls -= floor_calls
        faults -= floor_faults
        verdict = []
        if calls > entry["syscalls"]:
            verdict.append("syscalls over")
        if faults > entry["minor_faults"]:
            verdict.append("faults over")
        if verdict:
            over.append(label)
        print(f"| `opal {label}` | {calls} | {entry['syscalls']} | {faults:.0f} | "
              f"{entry['minor_faults']} | {', '.join(verdict) or 'ok'} |")
        if args.rebase:
            # Headroom for what moves between runs and machines without a
            # change to opal: thread scheduling (futex), the environment's
            # size, page-cache state.
            entry["syscalls"] = math.ceil(calls * 1.10) + 5
            entry["minor_faults"] = math.ceil(faults * 1.15) + 10
    if args.rebase:
        budget_path.write_text(json.dumps(budget, indent=2) + "\n")
        print(f"\nrebased {budget_path}")
        return 0
    if over:
        print(f"\nover budget: {', '.join(over)}. A change that needs more should say why "
              f"in its commit and rebase the budget with --rebase.")
        return 1
    return 0


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="mode", required=True)
    for mode in ("bench", "gate"):
        p = sub.add_parser(mode)
        p.add_argument("--opal", default=shutil.which("opal"))
        p.add_argument("--work", default=f"/tmp/opal-runtimes-{mode}")
        p.add_argument("--results", default=str(Path(__file__).resolve().parent / "results"))
    b = sub.choices["bench"]
    b.add_argument("--rounds", type=int, default=10)
    b.add_argument("--engine", action="append", default=[], type=lambda s: s.split("=", 1),
                   metavar="NAME=PATH", help="another runtime binary taking a script path")
    b.add_argument("--unpinned", action="store_true")
    g = sub.choices["gate"]
    g.add_argument("--budget", default=str(Path(__file__).resolve().parent / "runtime-budget.json"))
    g.add_argument("--rebase", action="store_true")
    args = parser.parse_args()
    if not args.opal:
        sys.exit("compare-runtimes: no opal on PATH; pass --opal")
    if args.mode == "bench":
        return bench(args, sub.choices["bench"])
    if not shutil.which("strace"):
        sys.exit("compare-runtimes: gate counts syscalls with strace, which isn't on PATH")
    return gate(args)


if __name__ == "__main__":
    sys.exit(main())
