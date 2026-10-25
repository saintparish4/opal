# Engine spikes

Which JavaScript engine `opal run app.js` should embed, decided by measurement rather than by anyone's documentation. Two minimal runtimes, each a console and nothing else, timed by [`../compare-runtimes.py`](../compare-runtimes.py) in the same interleaved rounds as node, bun, and two controls.

- [What was built](#what-was-built)
- [Results: 2026-10-24](#results-2026-10-24)
- [What they show](#what-they-show)
- [The decision](#the-decision)
- [The event loop and the tokio ban](#the-event-loop-and-the-tokio-ban)
- [Reproducing](#reproducing)

## What was built

A Cargo workspace of its own (`Cargo.toml` here), so neither engine enters opal's `Cargo.lock` or its `cargo deny` run until one is chosen.

- **`quickjs/`**: quickjs-ng 0.16.2 through `rquickjs` 0.14.0. QuickJS has no startup snapshot, so the bootstrap is parsed and compiled on every start.
- **`v8/`**: V8 through the `v8` crate 152.2.0 (V8 15.2), on V8's single-threaded platform with `--single-threaded`, so that starting V8 starts no worker threads. With a snapshot, the bootstrap is run once by `v8::SnapshotCreator` at build time and the blob is embedded in the binary (`include_bytes!`), so starting reads no extra file.

Both run the same bootstrap (`bootstrap.rs`): a `console` whose `log` joins its arguments' `String()` forms, which is all the workloads print. Real runtimes bootstrap far more JavaScript than that (node:path, Buffer, the module loader), so `SPIKE_PAD_KB` adds that many kilobytes of functions that are defined and never called. The `-pad256` builds carry 256 KB of it, which gives the cost of a larger bootstrap a measured slope with and without a snapshot.

`build.sh` builds seven executables into `bin/`:

| Name | Engine | Bootstrap | Snapshot | V8 flags |
|---|---|---|---|---|
| `quickjs` | quickjs-ng | console | - | |
| `quickjs-pad256` | quickjs-ng | console + 256 KB | - | |
| `v8` | V8 | console | - | |
| `v8-pad256` | V8 | console + 256 KB | - | |
| `v8-snap` | V8 | console | yes | |
| `v8-snap-pad256` | V8 | console + 256 KB | yes | |
| `v8-snap-jitless` | V8 | console | yes | `--jitless` |

The release profile is `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.

## Results: 2026-10-24

Laptop: AMD Ryzen 5 5625U (WSL limited to 8 of 12 threads), 16 GB RAM, Linux 6.6 under WSL2. node 24.19.0, bun 1.4.2. Ten rounds; each round runs every command through `hyperfine -N` (5 warmups and 20 runs for hello, 1 and 3 for the others), starting from a different command each round. Median and min are over every sample of every round. Load average was 1.3 at the start and 1.0 at the end. Raw samples: [`../results/runtimes-20261024-214625.json`](../results/runtimes-20261024-214625.json).

**hello** (`console.log("hello")`: startup)

| Command | Median | Min | ×bun | ×node | User | Sys | Minor faults | Peak RSS | Syscalls | Executable |
|---|---|---|---|---|---|---|---|---|---|---|
| floor | 0.5 ms | 0.5 ms | 0.07 | 0.01 | 0.5 ms | 0.0 ms | 38 | 0.5 MB | 14 | 0.7 MB |
| opal --version | 1.7 ms | 1.4 ms | 0.22 | 0.04 | 1.9 ms | 0.0 ms | 172 | 3.6 MB | 71 | 7.4 MB |
| node 24.19.0 | 37.7 ms | 34.3 ms | 4.98 | 1.00 | 20.6 ms | 16.8 ms | 2909 | 43.1 MB | 707 | 120.2 MB |
| bun 1.4.2 | 7.6 ms | 6.2 ms | 1.00 | 0.20 | 0.0 ms | 6.9 ms | 707 | 11.6 MB | 254 | 75.8 MB |
| quickjs | **1.8 ms** | 1.5 ms | **0.24** | **0.05** | 0.0 ms | 1.9 ms | 164 | 3.4 MB | 79 | 1.5 MB |
| quickjs-pad256 | 22.5 ms | 20.6 ms | 2.97 | 0.60 | 16.9 ms | 3.5 ms | 1001 | 6.8 MB | 120 | 1.7 MB |
| v8 | 8.3 ms | 7.0 ms | 1.10 | 0.22 | 3.9 ms | 4.0 ms | 844 | 15.3 MB | 222 | 45.8 MB |
| v8-pad256 | 22.7 ms | 21.1 ms | 3.00 | 0.60 | 12.1 ms | 10.0 ms | 1793 | 19.8 MB | 284 | 46.0 MB |
| v8-snap | 8.0 ms | 7.0 ms | 1.06 | 0.21 | 3.8 ms | 4.0 ms | 837 | 14.9 MB | 221 | 46.2 MB |
| v8-snap-pad256 | 10.2 ms | 8.6 ms | 1.34 | 0.27 | 1.5 ms | 7.6 ms | 1048 | 16.3 MB | 234 | 46.8 MB |
| v8-snap-jitless | 8.6 ms | 7.1 ms | 1.13 | 0.23 | 0.0 ms | 8.1 ms | 838 | 14.9 MB | 216 | 46.2 MB |

User and system CPU are from one run per round, at the kernel's accounting granularity, so the 0.0 ms entries mean "under one tick", not zero.

**Steady state**: wall time to exit, so startup is included (it's the same few milliseconds in every row of an engine).

| Command | fib(30) | ×bun | JSON round trip | ×bun | regex scan | ×bun | Peak RSS (json) |
|---|---|---|---|---|---|---|---|
| node 24.19.0 | 48.8 ms | 2.77 | 165.1 ms | 2.16 | 104.7 ms | 1.87 | 86.6 MB |
| bun 1.4.2 | **17.6 ms** | 1.00 | **76.6 ms** | 1.00 | **56.0 ms** | 1.00 | 43.1 MB |
| quickjs | 120.1 ms | 6.82 | 401.2 ms | 5.24 | 170.4 ms | 3.04 | 27.6 MB |
| v8 | 20.5 ms | 1.17 | 114.2 ms | 1.49 | 77.7 ms | 1.39 | 49.8 MB |
| v8-snap | 20.3 ms | 1.15 | 111.3 ms | 1.45 | 77.6 ms | 1.38 | 49.6 MB |
| v8-snap-jitless | 75.4 ms | 4.28 | 113.6 ms | 1.48 | 123.8 ms | 2.21 | 49.6 MB |

The full steady-state tables, with every build and the CPU, fault, and syscall columns, are in the raw samples and in the harness's printed output.

**Reproduced.** A second run, five rounds, two minutes later ([`../results/runtimes-20261024-214846.json`](../results/runtimes-20261024-214846.json)), gave every ratio to bun within 0.07 of the first and the same order in every table: quickjs 0.24× on hello in both, v8-snap 1.06× and 1.09×, v8 1.10× and 1.14×, node 4.98× and 5.20×.

## What they show

- **QuickJS starts 4× faster than bun and runs 3–7× slower.** Its hello is 1.8 ms, close to `opal --version`. With no JIT it is 6.8× bun on fib, 5.2× on JSON, 3.0× on regex, and 2.4×, 2.4×, and 1.6× *node*. "Start fast" is met with room to spare and "run fast" isn't met at all.
- **QuickJS's startup does not survive a realistic bootstrap.** 256 KB of never-called functions costs it 20.7 ms on every start, because it compiles everything it parses. Precompiled bytecode (`JS_WriteObject`/`JS_ReadObject`) would cut that, but wasn't measured here.
- **V8's floor is about 8 ms here, 6–10% above bun's.** It hasn't been broken down by phase yet. The counters point at memory setup rather than JavaScript: 844 page faults against bun's 707, and system time about equal to user time, for a script that does almost nothing. Neither target is met yet: v8-snap is 1.06× bun (target ≤ 1.00) and 0.21× node (target ≤ 0.20).
- **The snapshot is what makes V8 scale with the bootstrap.** At 256 KB, running the bootstrap on every start costs +14.4 ms, and baking it into the snapshot costs +2.2 ms. Every module opal's runtime adds to its bootstrap moves that slope, so with V8 the snapshot isn't optional.
- **V8 runs within 1.15–1.45× of bun and 2–2.4× faster than node,** with the JIT. `--jitless` buys nothing at startup (8.6 ms) and costs 3.7× on fib, so it's out.
- **Cost in size:** a V8 spike is 46 MB stripped, where opal is 7.4 MB today and node is 120 MB. QuickJS adds about 1.5 MB.

## The decision

Not made in this commit. The measurements don't pick an engine on their own, because neither engine meets both targets:

| | Startup ≤ 1.0× bun, ≤ 0.2× node | Run fast | Binary |
|---|---|---|---|
| quickjs-ng | met (0.24×, 0.05×), until the bootstrap grows | 3–7× slower than bun | +1.5 MB |
| V8 + snapshot | missed by 6% and 5% | 1.15–1.45× bun, faster than node | +46 MB |

The recommendation is **V8 with a startup snapshot**. The gap to "run fast" under QuickJS is the engine's design (no JIT) and can't be closed from outside it. The gap to the startup target under V8 is 0.4–0.6 ms of page faults and system time in isolate creation, which is the kind of cost that can be attacked from outside the engine (untried candidates: fewer pages touched at startup, a smaller snapshot, the order of pages in the binary), and the snapshot keeps it nearly flat as the bootstrap grows. The cost is a binary six times bigger, and a build that downloads V8's prebuilt static library from GitHub, outside what `cargo deny` sees.

That trade (binary size and the startup target against steady-state speed) is the project's to make, so step 3 waits on it.

## The event loop and the tokio ban

Decided before any engine binding is written, as the plan requires. `deny.toml` bans tokio, and the runtime keeps that ban:

- The first runtime surface (console, process, path, sync fs, timers, Buffer) needs no I/O readiness at all. Its event loop is a binary heap of timers, a microtask checkpoint after each macrotask, and a blocking sleep until the next timer is due. That is one thread, no polling, and no dependency.
- When async I/O arrives (fs promises, sockets, child processes), readiness comes from `mio` (MIT, no runtime, epoll and kqueue), or from epoll and kqueue through the `libc` crate opal already uses. That choice is made with that work, and either one keeps the ban.
- `deno_core` is out either way, because it requires tokio.

## Reproducing

```sh
benchmarks/engine-spikes/build.sh       # builds bin/*; the first V8 build downloads V8's static library
python3 benchmarks/compare-runtimes.py bench --opal target/release/opal --rounds 10 \
  $(for b in quickjs quickjs-pad256 v8 v8-pad256 v8-snap v8-snap-pad256 v8-snap-jitless; do
      echo --engine $b=$PWD/benchmarks/engine-spikes/bin/$b; done)
```

The harness refuses a node or bun other than the pinned versions (node 24.19.0, bun 1.4.2) unless you pass `--unpinned`, and `strace` is needed on `PATH` for the syscall column.
