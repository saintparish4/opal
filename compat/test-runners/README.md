# Test-runner oracle

`opal test` has to give Jest's verdict on every test Jest can run, and Vitest's on every test written for Vitest. This folder is how that gets checked, and it exists before the runner does. [`../test-runners.py`](../test-runners.py) runs one corpus of test files under jest, vitest, bun test and opal test, and diffs what each says about every test and what each writes to disk. Without an opal binary it checks the three reference runners against each other. That's how the corpus below was sorted.

- [Running it](#running-it)
- [How a run works](#how-a-run-works)
- [The corpus](#the-corpus)
- [Pins](#pins)
- [The mutation set](#the-mutation-set)
- [What opal test has to provide](#what-opal-test-has-to-provide)
- [Results: 2026-10-25](#results-2026-10-25)
- [Where the reference runners disagree](#where-the-reference-runners-disagree)
- [Not covered yet](#not-covered-yet)

## Running it

```sh
(cd compat/test-runners/runners && opal install)   # jest 30.5.2, vitest 4.1.12, @babel/parser, pinned
python3 compat/test-runners.py                      # about two minutes; needs node and bun on PATH
python3 compat/test-runners.py --opal target/release/opal
python3 compat/test-runners.py jest/each-success vitest/hooks.ts --no-mutants
```

| flag | what it does |
|---|---|
| `--opal PATH` | adds opal test as a fourth runner |
| `--runners jest,bun` | runs a subset |
| `--repeat N` | runs the corpus N times per runner; any change between runs is a failure |
| `--no-mutants` | skips the mutation set |
| `--reclassify` | moves fixtures between `agrees` and `known_difference` and drafts pins (see [Pins](#pins)) |
| `--full` | records every runner's verdict and failure message for every test, for curating pins |
| `--keep` | keeps the scratch folder, with each runner's log and report |

The script exits non-zero on any failure, and writes the run's record to `compat/results/test-runners-*.json`.

## How a run works

Each runner gets a fresh copy of the fixtures it can run, under one config of the oracle's own: the node environment, a 5-second test and hook timeout, Vitest's globals on, and no CI variables, so missing snapshots are written rather than failed. Jest and Vitest each run the whole corpus in one invocation. Bun runs it as `bun test --isolate`, so files can't see each other's globals.

A fixture's **dialect** says whose semantics it is written against, and so which runner is its reference: `jest` for files that use Jest's globals, `vitest` for files that import from `vitest`. Every other runner that can run the fixture is compared with the reference:

| runner | runs |
|---|---|
| jest | the jest dialect, since it has no `vitest` module |
| vitest | the vitest dialect, and the jest dialect except files that use the `jest` object or `@jest/globals` |
| bun | both |
| opal | both |

What is compared, per test file:

- **Verdicts:** pass, fail, skip or todo for every test, keyed by its describe path and title. There's also `(file)`, the file's own pass or fail, which is where errors outside any test land: a throwing `afterAll`, or a file that fails to load. Bun's JUnit report needs two readings. A hook that throws appears as a testcase named `(unnamed)`, and that's the file's failure, not a test. A file that throws while loading is missing from the report altogether, and counts as failed.
- **Writes:** every file the run created, changed or deleted inside the fixture. That covers snapshot files and inline snapshots written back into the test source.

Two differences in snapshot files are systematic, so they're removed before comparing instead of being pinned fixture by fixture:

1. the header line, which names the runner that wrote the file;
2. Vitest's ` > ` between the titles in a key, where Jest and Bun put a space.

Each runner has to write exactly its own header and key format, and anything else stays a difference. opal is held to the dialect's format, Jest's for jest files and Vitest's for vitest files, because that's what lets it round-trip a project's committed snapshots.

## The corpus

`corpus/<class>/<dialect>/<fixture>/` holds one test file per fixture, plus the helpers, `__mocks__` and committed snapshots from the directory it came from. [`seed.py`](./seed.py) copies every fixture out of two tags and records the source, SHA-256 and any change for every file in [`SOURCES.json`](./SOURCES.json). `seed.py --check` re-verifies the committed bytes against upstream. Both projects are MIT-licensed, and their licences are in [`LICENSES/`](./LICENSES/).

- **88 from Jest v30.5.2's `e2e/` fixture projects.** These are taken whatever their verdicts, since many of them fail on purpose. Left out:
  - anything that needs a config key beyond `testEnvironment`;
  - coverage, watch mode, jsdom, native ESM and fake timers;
  - concurrent tests and retries;
  - console and stack-trace output;
  - the jasmine globals Jest 30 removed;
  - files written with the Jest repository's own Babel config (Flow, ESM).
- **23 from Vitest v4.1.12's `test/core/test/`.** The rule is mechanical. A file is taken only if all three hold:
  - it passes under the pinned Vitest with a plain config when copied alone (86 of 165 do);
  - it imports nothing but `vitest`, its own packages and Node built-ins;
  - it stays inside what Jest also has.

  That leaves out tests of Vitest's internals and Vitest-only APIs: `test.extend`, `test.for`, retries, `expect.poll`, `vi.stubGlobal` and chai-style assertions.

`seed.py` names each exclusion and the reason for it. Changes on the way in are few, and recorded:

- a test file only Jest would collect (`__tests__/index.js`) gets `.test` before its extension;
- `package.json`, `yarn.lock` and `babel.config.js` aren't copied;
- each Vitest fixture gets `{"type": "module"}` as its `package.json`;
- the four `nested-test-definitions` files run under the node environment, where upstream configures jsdom. None of them touches the DOM.

## Pins

The two classes are held to different things, the way [`npm-cross-check.rs`](../../crates/opal-pm/tests/npm-cross-check.rs) holds its fixtures:

- **`agrees`:** every runner that ran gives the reference's verdicts and writes the reference's files.
- **`known_difference`:** each runner differs from the reference exactly as the fixture's `PIN.json` says, no more and no less.

A pin records, per runner, the verdicts that differ (`[reference, runner]`, with `null` for a test the runner didn't report), a unified diff for every written file that differs, and any planted failure the runner let through. Each runner's entry starts with a `why`, a person's account of the cause, written after reading the runners' failure messages. For example, from `known_difference/jest/declaration-errors-describeReturnSomething/PIN.json`:

```json
{
  "vitest": {
    "why": "A describe callback that returns a value: Jest fails the file ('A \"describe\" callback must not return a value'); Vitest ignores it and runs the test inside.",
    "verdicts": {
      "__tests__/describeReturnSomething.test.js": {
        "(file)": ["fail", "pass"],
        "describe return errors › t": [null, "pass"]
      }
    }
  }
}
```

A pin that fails means the difference grew, shrank or was fixed. Whichever it is, the fix is a decision about that difference and an edit to the pin, not a retry. `--reclassify` does the mechanical half. It moves a fixture whose runners disagree into `known_difference` and drafts its pin with `"why": "TODO"`, and it moves one that no longer differs back to `agrees`. A TODO is a failure until a person replaces it.

## The mutation set

A fixture whose runners agree can still agree by accident. A runner that never runs a test's body agrees with one that runs it and passes. So [`mutate.cjs`](./mutate.cjs) plants failures. Up to five candidates come from each test file, in two kinds, both inside the callback of a `test` or `it`:

1. **Flip an assertion:** toggle `.not` before the matcher of an `expect(...)` chain. Snapshot matchers are never flipped.
2. **Throw at the end of the test's body:** only used for files without assertions to flip.

The reference runner decides which candidate is the file's planted failure: the first that turns exactly one passing test into a failing one and changes nothing else. Every other runner that passed that test before the mutation must fail it after. A runner that doesn't has a difference like any other, pinned as `"mutant"`, with one exception: opal can't pin one, so a false pass in opal is always a failure.

The first run's planted failures found a false pass in both Vitest and Bun. Jest runs a test written as a generator function (`function* () {...}`) to its end. Vitest and Bun call it, get a generator object back, and pass the test without running its body. Both runners' verdicts on the unmutated file matched Jest's.

55 of the 111 files have a planted failure. 21 have no candidate, mostly hook, declaration and `done` fixtures with no `expect` in a test body. 35 have candidates but none the reference accepts: every test already fails or is skipped, or every assertion sits in an `.each` body, where one edit flips a whole table.

## What opal test has to provide

The harness runs `opal test --json --outputFile=<file>` in a folder holding the corpus and its own `jest.config.cjs` and `vitest.config.mjs`. opal has to write Jest's `--json` report, which is also the format Vitest's JSON reporter writes. A stand-in that forwards to Jest gives 88 of 88 on the jest fixtures and 34 of 34 planted failures, which checks this path before opal test exists.

opal test has to satisfy these, in order of strictness:

1. Every jest fixture, in both classes: Jest's verdicts and Jest's writes. The `known_difference` pins are about Vitest and Bun, not opal.
2. Every vitest fixture: Vitest's verdicts and writes, unless an `opal` entry in the pin says otherwise. Those entries are the Vitest compatibility list.
3. No planted failure gets through.

## Results: 2026-10-25

jest 30.5.2, vitest 4.1.12, bun 1.4.2 and node 24.19.0 on WSL2 (Linux 6.6.87.2), 8 threads, run twice per runner, with nothing different between the runs. The record is [`compat/results/test-runners-20261025-112955.json`](../results/test-runners-20261025-112955.json).

| runner | agrees | known_difference, as pinned | planted failures failed |
|---|---|---|---|
| jest | reference for 27 | reference for 61 | - |
| vitest | 26 of 26, reference for 6 | 49 of 49, reference for 17 | 18 of 19 |
| bun | 33 of 33 | 78 of 78 | 32 of 33 |
| opal | not run | not run | not run |

Of the 111 fixtures, 33 agree everywhere. The other 78 hold 102 pinned runner differences: 38 for Vitest and 64 for Bun. The table counts fixtures, so a fixture pinned only for Bun still counts toward Vitest's 49.

## Where the reference runners disagree

All three runners call themselves Jest-compatible, and the pins say exactly how far that goes. Grouped by cause, with fixture counts:

**Silent false passes.** These are the ones that matter most, because nothing in the output says a check didn't happen.

| runner | what passes when Jest fails it | fixtures |
|---|---|---|
| Vitest, Bun | a generator-function test: the body never runs | 1 |
| Vitest | a test taking `done`: there are no done callbacks, so the test returns at once and passes, even when its expectation fails later or it times out | 3 |
| Bun | a tagged-template `.each` table: it runs more rows than written, with `undefined` values, so a failing table passes | 2 |
| Bun | `done(error)` in a hook: the error is ignored | 2 |
| Bun | calling `done` twice: allowed | 1 |
| Vitest, Bun | a test that returns a non-Promise value: allowed | 1 |

**Declarations Jest rejects and the others accept.**

| what is declared | accepted by | fixtures |
|---|---|---|
| a `describe` returning a value or a Promise | Vitest, Bun | 2 |
| tests added after a Promise tick | Vitest, Bun | 1 |
| hooks inside a running test | Vitest | 2 |
| a describe with hooks and no tests | Bun | 3 |
| `test.todo` with a callback or no title | Vitest, Bun | 3 |
| a test that both takes `done` and returns | Bun | 1 |
| a template table whose cells don't fill its headings | Vitest, Bun | 1 |

**`.each` titles.**

- Bun formats none of them: `%s`, `%p`, `%j` and `$name` stay as written. That affects 9 fixtures.
- Vitest quotes interpolated strings, doesn't support `%p` in array tables, prints a `$path` through `null` as `undefined`, and stops a `$key` at `(`. That affects 4 fixtures.
- Both throw on `%j` with a BigInt.

**`.only` and `.skip`.**

- A `test.only` inside a `describe.skip` puts Vitest and Bun into only-mode for the file. Jest ignores it.
- Bun leaves tests it doesn't run out of its report, where Jest and Vitest report them as skipped. That affects 7 fixtures.

**Snapshots.**

| behavior | runner |
|---|---|
| Jest escapes `\r` and `\n` in keys; the other runner writes them raw, so it can't find Jest's entries | Vitest, Bun |
| hint keys are `test > hint 1` where Jest writes `test: hint 1` | Vitest |
| a Jest-written file is rewritten under Vitest's own header even when no entry changes | Vitest |
| RegExps are written without Jest's `escapeRegex` | Bun |
| entries are not sorted by key | Bun |
| `toThrowErrorMatchingInlineSnapshot` snapshots the message, not the error | Bun |
| `expect.addSnapshotSerializer` is not implemented | Bun |

**Missing APIs.**

| API | missing in | fixtures |
|---|---|---|
| `fit`, `xit` | Vitest (3), Bun (3) | 4 |
| `test.failing` (Vitest has `test.fails`) | Vitest | 7 |
| `test.skip.failing` | Bun | 2 |
| `jest.mock(path)` without a factory | Bun | 12 |
| the `assert` and `suite` exports of `vitest` | Bun | 4 |
| much of jest-mock: `settledResults`, `vi.isMockFunction`, accessor spies, restoring originals | Bun | 1 |
| test context (`({ expect, skip })`) | Bun | 2 |
| `this.utils.diff` in custom matchers | Bun | 1 |

Each line is backed by a pin with the exact verdicts, which fails the day the behavior changes.

## Not covered yet

- **Jest's runtime-written fixtures.** Jest's e2e tests that write their fixtures from strings at run time (`to-match-snapshot`, `to-match-inline-snapshot` and the like) hold most of its snapshot-update semantics. They aren't imported yet.
- **Module mocking.** It's covered only as far as `mock-json` and `mock-names` go.
- **TypeScript, and the Nest suite and the three popular repositories from the done criteria.** These belong with step 2 and its fixtures.
- **Timing.** This harness checks behavior; the timing table is `benchmarks/`'s job.
- **Other machines and versions.** One machine, one version of each runner. Re-seeding at a new tag is `seed.py` with new pins in `UPSTREAM`, followed by a run with `--reclassify` and a review of every drafted pin.
