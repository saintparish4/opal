# Engine floor

What does the engine under `opal run` and `opal test` cost before any of the
runner is written? This folder holds two throwaway prototypes of that engine
and the script that times them against bun, node, jest and vitest. It is the
step-0 measurement for `opal test`. Nothing here is the test runner.

- [The prototypes](#the-prototypes)
- [What is held equal](#what-is-held-equal)
- [Reproducing](#reproducing)
- [What these numbers do not show](#what-these-numbers-do-not-show)

The results and the recommendation are in [RESULTS.md](./RESULTS.md).

## The prototypes

**A, `deno/`: V8 embedded through deno_core 0.413.** One Rust binary that owns
the isolate. It runs with or without a startup snapshot that already holds the
harness, and it has three isolation modes for a multi-file run:

| `--isolate` | what one file can see of another | the model of |
|---|---|---|
| `shared` | everything on `globalThis` | bun test |
| `context` | nothing: a fresh V8 context per file, one isolate, harness evaluated again | Jest |
| `runtime` | nothing: a fresh `JsRuntime` (isolate) per file | nobody; an upper bound |

deno_core has no public API for a second context, so `context` mode creates
it with the `v8` crate directly. Its async test work is drained with a
microtask checkpoint, which is enough for these fixtures and not for timers or
I/O.

**B, `node/`: a Rust orchestrator over Node child processes.** `std` only.
`driver.cjs` is the JavaScript half that runs inside Node. It runs with or
without a Node user-land startup snapshot built from `driver.cjs`, and it has
`shared` and `context` modes like A, plus `process`: one Node per file, as
many at once as there are CPUs, which is node --test's model and Jest's with
its worker pool.

Both run the same [`harness.js`](./harness.js): describe, test, the four
hooks, and the dozen matchers the fixtures use. It is a stand-in of realistic
size, so the timings include a harness. It is not the start of opal's
`expect`, and nothing in it is checked against Jest.

## What is held equal

- **The test files.** `fixtures/one` and `fixtures/forty` are the same ten
  tests, run unchanged by jest, vitest (with `globals: true`), bun test and
  both prototypes. node:test has no Jest globals, so `fixtures/node-test`
  holds the same tests written against `node:test` and `node:assert`.
- **How a file is evaluated.** Both prototypes read each file and evaluate it
  inside a function wrapper, as a CommonJS loader does. Evaluated bare, two
  files that each declare a top-level `let` collide in a shared context, which
  is the first thing the prototypes ran into.
- **The checks.** Before timing, each prototype was run on all 40 files (40
  of 40 pass), on a file with a planted failure (it exits 1 and names both
  failing tests), and on two files where the first sets a global and the
  second asserts it can't see it (only `shared` lets it through).
- **The reference runners' versions.** [`runners/`](./runners/) pins jest
  30.5.2 and vitest 4.1.12, installed with `opal install`. Jest's config is
  [`jest.config.cjs`](./jest.config.cjs) in this folder because jest 30.5
  fails to find a config passed with `--config` from outside the working
  directory.

## Reproducing

```sh
cd benchmarks/engine-floor
cargo build --release              # its own workspace; deno_core never enters opal's Cargo.lock
(cd runners && opal install)
python3 run.py                     # about 15 minutes; writes results/engine-floor-*.json
python3 run.py --quick             # one round, to check every command runs
```

`run.py` builds the snapshots and a static empty binary for the floor row when
they're missing. It interleaves: several rounds, the command order shuffled
each round with a fixed seed, a short hyperfine block per command, every sample
pooled. It reports min and median, never mean, and it records how many other
processes were runnable when each block started, so a disturbed run shows in
the output instead of hiding in the numbers.

## What these numbers do not show

- **Node compatibility.** The fixtures import nothing. Test files for real
  code import `node:fs`, `node:path`, `node:events` and their packages' own
  dependencies. B has all of Node. A has none of it: no `require`, no
  `process`, no `Buffer`, no `node:` modules. That gap is the cost of A, and no
  timing here measures it.
- **Tuning.** Neither prototype is tuned. A uses deno_core's defaults (V8
  platform threads, tokio, its bundled extensions). B spawns `node` from
  `PATH`. Both have headroom; neither has been measured for it.
- **Parallelism.** A's modes run on one thread. B's `process` mode uses every
  CPU. The 40-file table names this per row.
- **Other machines.** One WSL2 laptop, whatever its CPU frequency was doing.
  Ratios carry to another machine better than milliseconds, and the
  milliseconds roughly halved between a loaded and a quiet run on this one.
