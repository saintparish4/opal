# opal test: semantics

opal test promises Jest 30.5.2's behavior for files written against Jest's globals. Where this file and Jest disagree, Jest is right and this file is a bug. Each rule names where it lives in Jest's source, so it can be checked against the original rather than against a summary. Two harnesses hold opal to this:

- [`compat/test-runners.py`](../../compat/test-runners.py) diffs opal against Jest, Vitest and Bun on a corpus copied from Jest's and Vitest's own suites.
- [`compat/test-lifecycle.py`](../../compat/test-lifecycle.py) generates random suites and diffs opal against Jest on what ran, in what order, and with what verdict.

- [How a file runs](#how-a-file-runs)
- [Isolation](#isolation)
- [Collecting tests](#collecting-tests)
- [Which tests run](#which-tests-run)
- [Hooks](#hooks)
- [Calling a test or hook](#calling-a-test-or-hook)
- [Where an error is charged](#where-an-error-is-charged)
- [What a file reports](#what-a-file-reports)
- [expect](#expect)
- [Snapshots](#snapshots)
- [Mock functions](#mock-functions)
- [Module mocking](#module-mocking)
- [Transforms](#transforms)
- [Vitest's dialect](#vitests-dialect)
- [Checked so far](#checked-so-far)
- [Not yet](#not-yet)

## How a file runs

The order is jest-runner's `runTest.ts` and jest-circus's `jestAdapter.ts`:

1. a fresh environment;
2. the framework (circus, `expect`, mocks) evaluated inside it;
3. `setupFiles`;
4. the globals (`describe`, `test`, `expect`...), unless `injectGlobals` is off;
5. `setupFilesAfterEnv`, awaiting any that export a function;
6. the test file;
7. the run.

Anything thrown before the run starts fails the file as "Test suite failed to run", with no tests.

## Isolation

Every test file gets its own `vm` context (`runtime/environment.cjs`), built the way jest-environment-node builds one:

- The context has its own built-ins.
- Node's globals are lazy getters onto the host's.
- `Buffer`, `ArrayBuffer` and `Uint8Array` are the host's outright.
- `process` is a copy with its own event registry and its own `env`.

Every file also gets its own module registry (`runtime/loader.cjs`), so no module's state survives into the next file. Node's built-in modules are shared, as they are in Jest, except `process`.

What differs from Jest is cost, not isolation. Files run in long-lived Node workers instead of a fresh worker setup per file. The framework is compiled once per worker and run in each file's context. A module's compiled code is kept per worker and re-run, not recompiled, in the next file's context. Running the same compiled code in a new context yields new functions, new objects and new module state, so nothing a test can observe is shared.

## Collecting tests

From jest-circus's `index.ts` and the definition events in `eventHandler.ts`:

- `describe` runs its callback at once.
  - A callback that returns a Promise fails the file: "Returning a Promise from "describe" is not supported".
  - A callback that returns any other value fails the file too: "A "describe" callback must not return a value".
- A test or hook added after the run has started fails the file: "Cannot add a test after tests have started running".
- Inside a running test:
  - a nested `test` puts "Tests cannot be nested" on the outer test;
  - a hook puts "Hooks cannot be defined inside tests" on it;
  - a `describe` puts "Cannot nest a describe inside a test" on it.
- A `describe` that holds hooks and no tests fails the file: "Invalid: beforeEach() may not be used in a describe block containing no tests".
- `test.todo` takes exactly one string. Anything else fails the file: "Todo must be called with only a description".
- A missing or non-function callback throws at the call. `test` names `test.todo` in its message.
- `.each` follows jest-each. Titles are formatted with printf placeholders (`%p` uses pretty-format, `%j` JSON), or with `$name` and `$a.b` paths for arrays of objects and tagged-template tables. A malformed table becomes one failing test under the unformatted title.

## Which tests run

From `finish_describe_definition` and `add_test` in `eventHandler.ts`, and from `_runTestInContext` in `run.ts`:

- **A block's mode passes to its tests that have none.** The exception is an `only` block that already holds an `only` test.
- **One `only` puts the file in focused mode.** In focused mode, a test without a mode is skipped. A `test.only` inside a `describe.skip` doesn't count.
- **`-t` (`testNamePattern`) is a case-insensitive regular expression.** It is matched against the test's full name: the describe titles and the test's own, joined by spaces.
- **Skipped tests report `pending`, and `todo` tests report `todo`.** The tests a skipped describe holds are skipped.

## Hooks

From `getAllHooksForDescribe`, `getEachHooksForTest` and `hasEnabledTest` in `utils.ts`, and from `_runTestsForDescribeBlockOnce` in `run.ts`:

- **`beforeAll` and `afterAll` run only for a block that has at least one test that will run.** That count includes the block's nested blocks. A skipped block runs none of its hooks.
- **`beforeEach` hooks run from the outermost block in.** `afterEach` hooks run from the innermost block out. Within one block, both run in the order they were declared.
- **Once a `beforeEach` fails, no later `beforeEach` runs, and neither does the test.** Every `afterEach` still runs.
- **A failing `beforeAll` charges its error to every test under its block.** Those tests don't run their bodies, but their `afterEach` hooks do. A failing `afterAll` fails the file.

## Calling a test or hook

From `callAsyncCircusFn` in `utils.ts`, and `_callCircusTest` in `run.ts`:

- **A function that declares a parameter takes `done`.**
  - It finishes when `done()` is called.
  - `done(error)`, or `done(value)` with a non-error value ("Failed: ..."), fails it.
  - Calling `done` twice fails it.
  - A test that takes `done` and also returns something fails, "Test functions cannot both take a 'done' callback and return something", even when `done` was called.
- **A generator function is run to its end through co,** each yielded value awaited.
- **A test (not a hook) that returns anything but a Promise or `undefined` fails:** "test functions can only return Promise or undefined".
- **The timeout comes from the first that applies:**
  1. the test's or hook's own timeout argument;
  2. `jest.setTimeout`;
  3. `--testTimeout` or the `testTimeout` config;
  4. 5000 ms.

  A test that runs out fails with "Exceeded timeout of N ms for a test" (or "for a hook"), adding "while waiting for `done()` to be called" when it takes `done`.
- **`test.failing` inverts the verdict.** One that passes fails with "Failing test passed even though it was supposed to fail".
- **`jest.retryTimes(n)` re-runs a failed test up to n times,** after the rest of its block. With `retryImmediately`, it re-runs straight away instead.

## Where an error is charged

From the `error` event in `eventHandler.ts`, `globalErrorHandlers.ts` and `unhandledRejectionHandler.ts`:

- **An uncaught exception or unhandled rejection goes to whatever was running when it was raised.** That's tracked through Node's AsyncLocalStorage, as Jest does, so a timer a test started is charged to that test.
- **An unhandled rejection is charged when its test or hook finishes,** unless it was handled before then.
- **An error raised with nothing running fails the file.**

## What a file reports

From `runAndTransformResultsToJestFormat` in `jestAdapterInit.ts`, `TestScheduler.ts`, and `@jest/test-result`'s `addResult` and `formatTestResults`:

- A test's status is `pending` for skipped, `todo`, `failed` when it has any error, and `passed` otherwise.
- A file's JSON status:
  - `failed` when any test failed or the file has an error of its own;
  - `focused` when tests were skipped and none failed;
  - `passed` otherwise.
- A file that declares no tests at all fails: "Your test suite must contain at least one test."
- `--json` writes Jest's aggregated result, field for field.

## expect

`expect` is Jest's own: `expect`, `@jest/expect-utils`, `jest-matcher-utils`, `jest-diff`, `@jest/diff-sequences` and `@jest/get-type` at v30.5.2. [`port/port.mjs`](./port/port.mjs) carries them over as the `runtime/jest-*.cjs` files.

- **What the port changes.** It strips the types with Node's own stripper and rewrites ES modules to CommonJS the way Babel does. Nothing else in the code changes, and `port.mjs --check` verifies that the committed files still match.
- **What it maps out.** chalk becomes a shim without colors. pretty-format becomes opal's own port (`runtime/format.cjs`). Three functions of `jest-util` and `jest-message-util` come from `runtime/support.cjs`.
- **Where it runs.** `expect` is evaluated inside each test file's context, as Jest loads it, so `instanceof` checks and assertion counts see the test's own realm.

Messages have no colors. Stack traces in them leave out opal's runtime frames the way Jest leaves out its own.

## Snapshots

`jest-snapshot`, `@jest/snapshot-utils` and `@jest/expect` are ported the same way as `expect`. So the `.snap` format, the key order, escaping and obsolete-snapshot accounting are Jest's own code. `-u` rewrites, and `--ci` writes nothing and fails on a missing snapshot. A test that is skipped or failed, or a `failing` test that passed, keeps its snapshots from counting as obsolete (jestAdapter's `_addSnapshotData`).

Inline snapshots are the exception. Jest finds the matcher call at the snapshot's stack frame with Babel, sets the argument, and regenerates the whole call with `@babel/generator`. It then formats the file with the project's prettier when the project has one. opal's `runtime/inline-snapshots.cjs` finds the same call at the same frame and splices the same argument in, the first template or string argument replaced or one appended, but leaves the rest of the call as written.

- **Where opal and Jest differ.** Two known differences remain:
  - opal doesn't run prettier yet;
  - without prettier, Babel's reprint differs where it normalizes formatting, for example `expect({z: 1})`, which Babel writes as `expect({ z: 1 })`.

  Matching snapshots read the same either way. Only writing a new or changed inline snapshot can differ.

## Mock functions

`jest-mock` is ported as it is. `jest.fn`, `jest.spyOn`, `jest.replaceProperty`, `jest.mocked` and the `mock` record (`calls`, `results`, `instances`, `contexts`, `invocationCallOrder`, `lastCall`) are Jest's own.

- **At the start of each test.** `clearMocks`, `resetMocks`, `restoreMocks` and `resetModules` run in a `beforeEach` registered before any of the file's own, as jestAdapter registers it.
- **At the end of each file.** Every spy is restored. A spy on a built-in module, which later files in the same worker share, would otherwise outlive its file.

## Module mocking

From jest-runtime's `MockState.ts`, `CjsLoader.ts`, `Resolution.ts` and `index.ts`, and from jest-resolve's `getModuleID` and `getMockModule`:

- **Manual mocks.** Every file in a `__mocks__` folder under the roots, outside `node_modules`, is a manual mock. It is named by its path inside that folder, without the extension, as jest-haste-map names it.
- **Explicit mocks.** They are keyed by a module ID made of the resolved path and any manual mock's path, so two spellings of one module share a mock.
  - `jest.mock(name, factory)` registers a factory.
  - `jest.mock(name)` marks the module mocked. It is then served by a manual mock, a root one by name or one in a `__mocks__` folder beside the file, or else by an automock generated from the real module's shape.
  - `{virtual: true}` mocks a module that doesn't exist.
- **A root manual mock named like a package or a core module replaces it without any `jest.mock` call,** unless the test unmocks it. A module never gets itself in place of something it requires.
- **`automock`** mocks every required module except core modules, what `unmockedModulePathPatterns` exempts, and the dependencies of an unmocked `node_modules` package.
- **`jest.requireActual`** skips all of the above. `jest.requireMock` always goes through it.
- **`jest.resetModules` clears the registries and keeps the mocks.** `jest.isolateModules` gives its callback registries of its own.
- **Generating an automock loads the real module in scratch registries,** so nothing it requires stays loaded.
- **`require('@jest/globals')`** gives the globals with the requiring module's `jest`.

## Transforms

Where Jest runs babel-jest, opal runs its own transform (`src/transform.rs`), in Rust with oxc. A worker asks for it in the middle of a `require` over a second connection: a worker thread does the I/O while the main thread waits on `Atomics.wait`. The result is cached in memory and on disk, keyed by the source, its extension, its options and the opal binary's own hash.

- **What gets transformed.** A file outside `transformIgnorePatterns` (default: `/node_modules/`) when it is TypeScript or JSX, or when its text has `import`, `export` or a hoisted `jest` call. An ES module inside `node_modules`, `.mjs` or `.js` under `"type": "module"`, gets transformed regardless. Jest fails on those unless told to transform them; opal loads them, into the test's context like everything else. A config with `transform: {}` turns transforms off, as it does in Jest.
- **TypeScript.** oxc's transformer strips types, compiles enums and parameter properties, and lowers legacy decorators with `design:` metadata. The options come from the nearest `tsconfig.json`, `extends` included: `experimentalDecorators`, `emitDecoratorMetadata`, `useDefineForClassFields` (defaulting from `target` as tsc does), `jsx` and `jsxImportSource`. Node's strip-only mode can't run the Nest scaffold's sources; this can.
- **ES modules to CommonJS.** These follow @babel/plugin-transform-modules-commonjs, because Jest's users depend on its interop:
  - imports become `require`s hoisted to the top;
  - an imported binding is read through its module object at every use, with `(0, _m.f)()` for calls;
  - Babel's `_interopRequireDefault` and `_interopRequireWildcard` handle default and namespace imports;
  - an export is a writable property of `exports`, so `jest.spyOn(module, 'f')` can replace it, and every assignment to an exported binding updates it;
  - re-exports are getters placed right after their `require`, and a name the module exports itself wins over `export *`.
- **Hoisting.** `jest.mock`, `unmock`, `enableAutomock`, `disableAutomock` and `deepUnmock` at the top of a file move above everything, as babel-plugin-jest-hoist moves them, and so do `vi.mock`, `vi.unmock` and `vi.hoisted`.
- **`import.meta`** becomes the module's `url`, `filename`, `dirname` and `resolve`.
- **Stack traces.** Every source map token is carried through these edits. Each test's context gets an `Error.prepareStackTrace` that maps frames back to the lines the user wrote, as Jest installs source-map-support. Inline snapshots find their call through those frames.

## Vitest's dialect

A project is in Vitest's dialect when its folder has no Jest config but a `vitest.config.*` (or `vite.config.*`), or lists vitest as a dependency. The config is evaluated by Node with `vitest/config` and `vite` stubbed through `module.registerHooks`, which costs about 40 ms instead of the half second loading Vite takes. `include` and `exclude`, `globals`, `setupFiles`, `testTimeout`, the mock-reset flags and `resolve.alias` are read.

Vitest is an alias layer over the Jest semantics above (`runtime/vitest.cjs`):

- **Its API.** The `vitest` module and `vi` map onto the same framework.
- **Test functions.** Tests and hooks get Vitest's context object instead of `done`, and a hook may return a cleanup.
- **Snapshots.** Snapshot files use Vitest's header and ` > ` keys.
- **Imports.** `./a.js` resolves to `./a.ts` as Vite resolves it.

Where Vitest's behavior differs from Jest's, opal does what Jest does. [`COMPATIBILITY.md`](./COMPATIBILITY.md) lists each place.

## Checked so far

| check | result |
|---|---|
| `compat/test-lifecycle.py`, seeds 2 to 6, 1,000 files each | 5,000 of 5,000 files identical to Jest, about 32,000 tests, on verdicts and on the order every hook and test ran |
| the same with `-t '[13579]$'` and `-t 'describe1\|test2'` (seed 8) | 2,000 of 2,000 identical |
| `compat/test-expect.cjs`, seeds 1 to 3, 20,000 cases each, against Jest's installed `expect` build | 300,000 calls to `toEqual`, `toStrictEqual`, `toMatchObject`, `toContainEqual` and `toContain`: no verdict differs, and no failure message differs by a byte |
| the same with a bug planted, `Object.is` replaced by `===` in equality, or `-0` printed as `0` | caught: 380 verdicts, or 1,688 messages, differ in 5,000 cases |
| `compat/test-mock.cjs`, seeds 1 and 2, 3,000 programs of 30 operations each, against Jest's installed `jest-mock` build | 180,000 operations: the full recorded state of every mock identical after each one |
| the same with a bug planted, each call's `this` recorded as `undefined` | caught: 484 of 500 programs differ |
| `compat/test-runners.py --opal`, all 88 Jest-dialect fixtures | identical to Jest in verdicts and in every file written, the known differences included; all 34 planted failures fail |
| the Nest 12 scaffold's own suite (`compat/frameworks/nest`, Vitest, decorators with metadata, ES-module-only `@nestjs/*`) | 1 of 1 passing, as under `vitest run` |

## Not yet

- **Fake timers.** `jest.useFakeTimers` and friends throw rather than pass.
- **Running the project's prettier on inline snapshots.**
- **TypeScript and ESM test files.**
- **Watch mode.**
- **Vitest's dialect.**
- **Fake timers, describe-level retries, `--randomize`, test locations in results, and custom test environments.**
