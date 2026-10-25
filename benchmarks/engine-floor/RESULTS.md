# Engine floor: results, 2026-10-24

Method and limits are in [README.md](./README.md). The raw samples are in
`results/engine-floor-20261024-212507.json` (timing) and
`results/engine-floor-20261024-212538.json` (memory).

**Machine:** AMD Ryzen 5 5625U, 8 threads, WSL2 (kernel 6.6.87.2), node
24.19.0, bun 1.4.2, hyperfine 1.20.0, deno_core 0.413.0. At most three other
processes were runnable when any block started, and the median was zero. The
machine was quiet. An earlier run under load measured every row at roughly
twice these times, which is why the ratios are the result and the milliseconds
are not.

## Hello world

5 interleaved rounds of 20 runs. Ratios are of medians.

| runner | min | median | × bun | × node | peak RSS | minor faults |
|---|---:|---:|---:|---:|---:|---:|
| floor: empty static binary | 0.5 ms | 0.5 ms | 0.07 | 0.014 | 0.5 MB | 42 |
| node hello.js | 35.3 ms | 37.8 ms | 5.29 | 1.000 | 44.4 MB | 2880 |
| bun hello.js | 6.2 ms | 7.1 ms | 1.00 | 0.189 | 11.8 MB | 716 |
| A deno_core, no snapshot | 17.5 ms | 18.9 ms | 2.64 | 0.500 | 24.8 MB | 1624 |
| A deno_core, snapshot | 10.4 ms | 11.9 ms | 1.66 | 0.315 | 18.6 MB | 1225 |
| B node orchestrator | 35.6 ms | 39.1 ms | 5.48 | 1.037 | 44.2 MB | 2952 |

## One file, ten tests

5 interleaved rounds of 6 runs. Every opal row gives the file a fresh context.

| runner | isolation | min | median | × bun test | × jest | peak RSS | minor faults |
|---|---|---:|---:|---:|---:|---:|---:|
| jest 30.5.2 | vm context per file | 573.7 ms | 587.2 ms | 60.90 | 1.000 | 93.4 MB | 12330 |
| vitest 4.1.12 | worker + module graph per file | 545.0 ms | 553.6 ms | 57.41 | 0.943 | 128.4 MB | 23192 |
| bun test | none | 8.3 ms | 9.6 ms | 1.00 | 0.016 | 17.5 MB | 914 |
| node --test | process per file | 128.0 ms | 132.9 ms | 13.78 | 0.226 | 52.5 MB | 8407 |
| A deno_core, no snapshot | V8 context per file | 19.4 ms | 21.0 ms | 2.18 | 0.036 | 25.5 MB | 1721 |
| A deno_core, snapshot | V8 context per file | 12.5 ms | 14.2 ms | 1.47 | 0.024 | 20.0 MB | 1347 |
| B node orchestrator | vm context per file | 39.8 ms | 42.5 ms | 4.41 | 0.072 | 45.5 MB | 3153 |
| B node orchestrator, snapshot | vm context per file | 53.2 ms | 56.8 ms | 5.89 | 0.097 | 45.8 MB | 6105 |

Peak RSS is the largest single process, not the sum over jest's or vitest's
workers.

## Forty files, 400 tests

3 interleaved rounds of 4 runs. The last column is the cost of each file after
the first, from the one-file and forty-file medians.

| runner | isolation | min | median | × bun test | × jest | per extra file |
|---|---|---:|---:|---:|---:|---:|
| jest 30.5.2 | vm context per file, worker pool | 1.08 s | 1.18 s | 55.30 | 1.000 | 15 ms |
| vitest 4.1.12 | worker + module graph per file | 2.19 s | 2.34 s | 109.90 | 1.987 | 46 ms |
| bun test | none | 19.9 ms | 21.3 ms | 1.00 | 0.018 | 0.30 ms |
| node --test | process per file, parallel | 784.7 ms | 805.9 ms | 37.83 | 0.684 | 17 ms |
| A deno_core, shared | none | 20.6 ms | 23.1 ms | 1.08 | 0.020 | 0.23 ms |
| A deno_core, context | V8 context per file, 1 thread | 49.0 ms | 50.8 ms | 2.38 | 0.043 | 0.94 ms |
| A deno_core, runtime | isolate per file, 1 thread | 160.0 ms | 166.2 ms | 7.80 | 0.141 | 3.9 ms |
| B node orch., shared | none | 55.4 ms | 59.9 ms | 2.81 | 0.051 | 0.45 ms |
| B node orch., context | vm context per file, 1 thread | 89.3 ms | 93.9 ms | 4.41 | 0.080 | 1.3 ms |
| B node orch., process | process per file, parallel | 298.7 ms | 318.1 ms | 14.93 | 0.270 | 7.1 ms |

The per-file figures for `shared` and `context` are against the one-file
`context` row, the only one-file row each engine has; within one engine, a
fresh context per file costs (50.8 − 23.1) / 39 = 0.71 ms on A and
(93.9 − 59.9) / 39 = 0.87 ms on B.

## What the numbers say

1. **Jest's isolation is cheap. Jest is not.** A fresh context per file costs
   under a millisecond on either engine. Jest pays about 15 ms per file and
   about 590 ms before the first. Opal can keep the isolation thousands of
   suites depend on and still run 400 tests in about 0.04 to 0.08 of jest's
   time.
2. **The engine choice is a constant, not a slope.** A and B differ by about
   28 ms per invocation (Node's own start) and by a fraction of a millisecond
   per file. Over a real suite the constant disappears into the tests. In
   watch mode a resident Node pays it once.
3. **A needs its snapshot.** Without one, deno_core spends about 7 ms
   evaluating its own bootstrap on every start.
4. **Node's user-land snapshot is a loss.** It made B's start about 14 ms
   slower and doubled its page faults: the 6.2 MB blob replaces Node's built-in
   snapshot instead of extending it. Node also warns that `vm` isn't verified
   inside one.
5. **Isolate per file is the wrong granularity.** 3.9 ms per file, four times
   a context, for no isolation a context lacks here.
6. **Process per file is what makes node --test and Jest's pool slow.** B's
   `process` mode, which is the same model, costs 7 ms per file even with
   every CPU busy.

## What A costs that the timing doesn't show

The fixtures import nothing. The Nest scaffold's one unit test, the first
real suite in the done criteria, was traced in plain Node with a resolve hook
on its built output. It reaches **18 Node built-ins** before its single
`expect`: async_hooks, buffer, crypto, events, fs, http, https, module, net,
os, path, perf_hooks, querystring, stream, tty, url, util and zlib. That's
not counting globals like `process`, `Buffer` and `setImmediate`. deno_core
provides none of them. On A, every one is opal's to write and keep compatible
before that test can pass. B has all of them today, on the Node version the
project runs in production.

## Recommendation: B, with A kept possible

Build `opal test` on the Node orchestrator, with Jest's isolation done
in-process (a fresh `vm` context per file, in a small pool of long-lived Node
workers), not with a process per file.

- **It meets the done criteria now.** Nest and the three popular repos need
  Node's surface, not a fast empty engine.
- **It is already 0.072 of jest on one file and 0.080 on forty**, with the
  same isolation model.
- **The JavaScript half stays engine-agnostic.** `harness.js` ran unchanged
  in both engines here. Keep the runner's JavaScript behind that narrow host
  contract (print, read a file, evaluate in a fresh context, await), and A
  stays a later swap instead of a rewrite.

**What it rules out:**

- Beating bun on a one-shot, single-file run. B's floor is Node's start,
  about 5.5 × bun's hello world. Only an embedded engine gets under that.
- A test runner with no Node installed. `opal test` would need `node` on
  `PATH`, as opal's package manager already assumes for running anything it
  installs.
- Node user-land snapshots as a startup lever, measured above.
- "First consumer of opal's own runtime" in the literal sense, until `opal
  run` has an engine of its own. The runtime branch hasn't chosen one yet.
  This choice doesn't block either outcome there, but if `opal run` embeds V8,
  the test runner's move onto it should wait until that engine runs the Nest
  test above.

**If you pick A instead**, the order of work changes. Node built-in
compatibility comes before every step-2 item, starting with those 18. The done
criteria then wait on it, not on the runner.

## A correction to step 2

"TypeScript by oxc stripping with Node's default behavior as the reference"
can't pass the Nest done criterion. Node 24's default strip-only mode refuses
both of the scaffold's source files:

```text
src/app.service.ts     SyntaxError at @Injectable()   (decorators are not erasable)
src/app.controller.ts  ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX: TypeScript parameter property
                       is not supported in strip-only mode
```

Its tsconfig sets `experimentalDecorators` and `emitDecoratorMetadata`, and
Nest's dependency injection reads that metadata at run time. The reference has
to be `tsc`'s emit under the project's own tsconfig. Node's stripping is the
reference only for files that use erasable syntax alone. oxc's transformer has
legacy decorators and decorator metadata, so oxc can stay the tool.
