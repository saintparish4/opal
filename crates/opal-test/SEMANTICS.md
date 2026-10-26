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
- [Related tests and watch mode](#related-tests-and-watch-mode)
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

Each worker has `JEST_WORKER_ID`, 1 up to the number of workers, as jest-worker numbers them; a worker that replaces a crashed one takes its number. `NODE_ENV` is `test` unless it was set, as jest-cli sets it.

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
- **What it maps out.** pretty-format becomes opal's own port (`runtime/format.cjs`). Three functions of `jest-util` and `jest-message-util` come from `runtime/support.cjs`. chalk is chalk 4.1.2 itself, with ansi-styles, color-convert and supports-color, copied from the oracle's install by [`port/port-chalk.mjs`](./port/port-chalk.mjs).
- **Where it runs.** `expect` is evaluated inside each test file's context, as Jest loads it, so `instanceof` checks and assertion counts see the test's own realm.
- **A project on Jest 29.** It also gets the matchers Jest 30 removed, which its tests may use: `toThrowError`, and the spy aliases from `toBeCalled` to `nthReturnedWith`. opal reads the installed Jest's major version from the project's `node_modules` (`config.cjs`). A spy alias is the matcher it stands for, so its failure message names that matcher, where Jest 29's named the alias.

Messages are colored when Jest's would be. jest-worker gives each worker `FORCE_COLOR=1` when supports-color says the terminal running Jest supports color, or when `--color` is given, and no `FORCE_COLOR` otherwise. opal's pool makes the same decision (`src/colors.rs`). So a message snapshotted under `--color` carries chalk's codes byte for byte, as it does in Jest; `compat/test-expect.cjs --color` checks that. Stack traces in messages leave out opal's runtime frames the way Jest leaves out its own.

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

Resolution is Node's, as jest-resolve's default resolver is: `moduleNameMapper` first, then relative paths with `moduleFileExtensions`, then a bare name through `exports` and `node_modules` up the tree. A package can import itself by its own name through its `exports`, as Node and jest-resolve both allow. es-toolkit's tests import `es-toolkit/compat` from inside es-toolkit.

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

Jest transforms a file with the first `transform` entry whose pattern matches it, babel-jest for `\.[jt]sx?$` when the config sets none. opal reads `transform` and `preset` as jest-config does (`runtime/config.cjs`, after `normalize.ts` and `setupPreset`), and decides, entry by entry, who runs it:

- **babel-jest, when the project's Babel config does nothing opal's own transform doesn't:** opal's transform (`src/transform.rs`), below. That is a config with only `@babel/preset-env` (without `loose` or `useBuiltIns: 'usage'`), `@babel/preset-typescript` (without `onlyRemoveTypeImports`, `allExtensions` or `rewriteImportExtensions`), `@babel/preset-react` (whose runtime and import source opal then uses), and plugins `@babel/preset-env` is itself made of. The config is read without loading Babel (`runtime/babel-config.cjs`): the root `babel.config.*` and rootDir's `.babelrc.*` or `package.json` `babel`, a JavaScript one evaluated with a stand-in for Babel's config API, with the `env` block for `BABEL_ENV`, `NODE_ENV` or `test`. A config with anything else, `overrides`, `only`, `ignore` or `extends` included, or one that can't be read that way, is babel-jest's. So is a file below a `.babelrc` of its own in a subfolder of rootDir. A project with no `transform` and no babel-jest installed, running opal test without Jest, gets opal's transform whatever its Babel config says, with a note that says so: Jest would use the babel-jest it ships with.
- **ts-jest:** ts-jest itself, emitting only, with its type-check done once for the run ([ts-jest](#ts-jest)).
- **Anything else, babel-jest otherwise included:** the transformer, called as Jest calls it ([Jest's transformer protocol](#jests-transformer-protocol)).

### opal's own transform

Where Jest runs babel-jest and the plan above lets it, opal runs its own transform, in Rust with oxc. A worker asks for it in the middle of a `require` over a second connection: a worker thread does the I/O while the main thread waits on `Atomics.wait`. The result is cached in memory and on disk, keyed by the source, its extension, its options and the opal binary's own hash.

- **What gets transformed.** A file outside `transformIgnorePatterns` (default: `/node_modules/`) when it is TypeScript or JSX, or when its text has `import`, `export` or a hoisted `jest` call. That goes for a file no `transform` pattern matches too, which Jest would leave as it is and then fail to load. An ES module inside `node_modules`, `.mjs` or `.js` under `"type": "module"`, gets transformed regardless. Jest fails on those unless told to transform them; opal loads them, into the test's context like everything else. A config with `transform: {}` turns transforms off, as it does in Jest.
- **TypeScript.** oxc's transformer strips types, compiles enums and parameter properties, and lowers legacy decorators with `design:` metadata. The options come from the nearest `tsconfig.json`, `extends` included: `experimentalDecorators`, `emitDecoratorMetadata`, `useDefineForClassFields` (defaulting from `target` as tsc does), `jsx` and `jsxImportSource`. Node's strip-only mode can't run the Nest scaffold's sources; this can.
- **ES modules to CommonJS.** These follow @babel/plugin-transform-modules-commonjs, because Jest's users depend on its interop:
  - imports become `require`s hoisted to the top;
  - an imported binding is read through its module object at every use, with `(0, _m.f)()` for calls;
  - Babel's `_interopRequireDefault` and `_interopRequireWildcard` handle default and namespace imports;
  - an export is a writable property of `exports`, so `jest.spyOn(module, 'f')` can replace it, and every assignment to an exported binding updates it;
  - re-exports are getters placed right after their `require`, and a name the module exports itself wins over `export *`.
- **Hoisting.** `jest.mock`, `unmock`, `enableAutomock`, `disableAutomock` and `deepUnmock` at the top of a file move above everything, as babel-plugin-jest-hoist moves them, and so do `vi.mock`, `vi.unmock` and `vi.hoisted`.
- **The wrapper's names.** An ES module's own top-level `require`, `module`, `exports`, `__filename` or `__dirname`, as in `const __dirname = dirname(fileURLToPath(import.meta.url))`, is renamed (`_require`, `_dirname`) wherever it is used, as Babel renames it, so it doesn't collide with the CommonJS wrapper's parameters.
- **`import.meta`** is one object per module, from the loader, with the module's `url`, `filename`, `dirname` and `resolve`.
- **Where opal's output differs from Babel's: a function's own text.** `Function.prototype.toString` gives a transformed function's source as the transform printed it, and Babel and oxc reformat differently. Only a test that reads a function's source sees this. The common case is `it.each` with `%s` of a function, whose name, and any snapshot keyed by that name, then differs. Under ts-jest this doesn't arise: ts-jest emits.
- **Stack traces.** Every source map token is carried through these edits. Each test's context gets an `Error.prepareStackTrace` that maps frames back to the lines the user wrote, as Jest installs source-map-support. Inline snapshots find their call through those frames.

### Jest's transformer protocol

From @jest/transform's `ScriptTransformer.ts`, in `runtime/transformers.cjs`:

- **Loading.** A transformer loads into the worker's own realm, as in Jest, when a file first needs it: its module, its `default` export, `createTransformer(transformerConfig)` when it has one. It must have `process`; opal loads modules synchronously, so a transformer with only `processAsync`, or a `createTransformer` that returns a promise, fails the file with Jest's message.
- **Calling.** `process(source, filename, options)` gets Jest's TransformOptions: the project config in jest-config's shape (`rootDir`, `cwd`, `globals`, `testMatch`, `moduleNameMapper` as pairs, a `cacheDirectory` under opal's cache), its `configString`, `transformerConfig`, a `cacheFS`, `instrument: false`, and the caller flags jest-runtime gives a CommonJS run (`supportsStaticESM: false` and the rest). A shebang is cut first. What comes back must be `{code}`, or the file fails with Jest's "Invalid return value".
- **Errors.** What a transformer throws goes through Jest's `handlePotentialSyntaxError`: a `SyntaxError` becomes "Jest encountered an unexpected token", with the original after "Details:".
- **Caching.** Results are kept on disk across runs under ScriptTransformer's own key: the transformer's `getCacheKey` when it has one, else the source, `configString`, filename and caller flags. That needs the transformer loaded, as it does in Jest.
- **Source maps.** The `map` it returns, or else the last inline `sourceMappingURL` comment in its code, as Jest reads them; the mappings are decoded only when a stack trace passes through the file.

### ts-jest

ts-jest type-checks each file as it emits it: per worker, a LanguageService holding every file it has seen, which builds a new Program each time it adds one. A file whose diagnostics aren't empty throws ts-jest's TSError from its transform, so the file, or the test that required it, fails. opal splits the two:

- **Emitting.** The workers run ts-jest with `isolatedModules` set on its ConfigSet after ts-jest has read it, so it emits with `transpileModule` file by file, with its own hoisting transformer and source maps, and prints nothing about the setting (ts-jest's own option for it is deprecated and warns). A project that sets `isolatedModules` itself is left alone: ts-jest then checks nothing more than `transpileModule` reports, in Jest too. Results are cached under a key opal works out without loading ts-jest: the versions of ts-jest and TypeScript, ts-jest's options, the tsconfig files it reads (`extends` included), the config, the file and its source. A run whose files are all in the cache doesn't load TypeScript. With `babelConfig`, `astTransformers` or `compiler` set, the key is ts-jest's own.
- **Checking.** One process checks types for the run (`runtime/checker.cjs`, started by `src/checker.rs`). It makes ts-jest's own ConfigSet, so the compiler options, the files reported on (`diagnostics.exclude`, `checkJs`), the codes ignored and the TSError's text are ts-jest's, and one Program: the tsconfig's files but its tests, as ts-jest starts with, and the run's test files, which ts-jest adds as it meets them. Modules resolve with the tsconfig's options, as ts-jest 29.4 resolves them. A worker asks about each file ts-jest emits for it and waits for the answer; one with errors throws ts-jest's TSError there, where ts-jest would have thrown it, so `expect(() => require('./broken')).toThrow()` passes as it does in Jest. Between questions the checker works ahead through the test files in the order they were handed out. `diagnostics: {warnOnly: true}` logs through ts-jest's logger instead.
- **Between runs.** A file's answer is kept with what it depended on: the files it imports, all the way down, and every file that declares globally (a script, `declare global`, `declare module`, `/// <reference>`), as TypeScript's own builder decides what an edit affects. An answer holds while none of those files changed, no tsconfig did, and no file was added to or removed from a folder the tsconfig globs or an `@types` folder. An error is looked at again each run. A run in which every answer holds doesn't load TypeScript; one that changed a test file checks that file.
- **Cost.** On jest-extended (79 test files, 159 files checked), Jest and opal run in turn on one machine: opal takes 0.42 of Jest's time from cold, 0.15 warm, and 0.56 after an edit to one test file. Jest's warm run checks no types at all, since ts-jest checks only a file it transforms; opal's gives the answers a cold run would.
- **Where this differs from ts-jest's own emit.** `transpileModule` sees one file. It keeps `export {Shape} from './types'` as an export when `Shape` is an interface, where the LanguageService drops it, so `Object.keys` of the module has `Shape`; and it can't inline an ambient `const enum` (one declared in a `.d.ts`), so code using one throws a ReferenceError where ts-jest's output has the value. A `const enum` exported from another `.ts` file works, read from the module at run time instead of inlined. `compat/test-transforms.py` pins both.
- **A file with both syntax and type errors** throws one TSError with both, as the LanguageService reports both together; the emit reports nothing on its own.

## Related tests and watch mode

`--findRelatedTests`, `-o` (`--onlyChanged`) and `--watch` all ask one question: which test files does a change affect. Jest answers it with jest-resolve-dependencies' `resolveInverse` over its haste map. opal answers with a reverse-dependency query on opal-core's module graph (`src/related.rs`), built to give the same answer:

- **The files.** Only files in the haste map take part: those under `roots` with an extension in `moduleFileExtensions` (or `.snap`), outside `node_modules`, and not matching `modulePathIgnorePatterns`. A changed file outside it relates to nothing, and a chain of dependencies passes only through files inside it.
- **The graph.** One opal-core graph is walked from every test file and every manual mock. Imports are resolved with `moduleNameMapper` first (the first pattern that matches, `$1` and so on filled in, the first candidate that resolves), then with `moduleFileExtensions` in Jest's order. The graph stops at packages.
- **What counts as a dependency.** These count, as Jest's dependency extractor counts them:
  - `import`, `export ... from`, `require` and `import()` with a string;
  - `jest.requireActual`, `jest.requireMock` and `jest.createMockFromModule`.

  Type-only imports and `jest.mock` calls don't count.
- **Manual mocks.** A module that imports `./x` also depends on the manual mock named `x` when that mock is in the `__mocks__` folder beside what `./x` resolved to. An import that doesn't resolve depends on the manual mock named by the whole specifier. Built-in modules are skipped.
- **Snapshots.** A changed snapshot file stands for its test file (jest-snapshot's default `resolveTestPath`).
- **The answer.** The related tests are the changed files that are tests, plus every test that reaches a changed file.
- **`-o`.** The changed files are what git reports for each root's repository, as jest-changed-files asks for them: `git diff --cached --name-only`, then `git ls-files --other --modified --exclude-standard`. Mercurial and Sapling aren't read.
- **`--watch`.** The first run takes what `-o` would. Each later one takes the tests related to the files changed since the run before, gathered until changes stop for 100 ms. Test files are found afresh each time, so a new test or a new import counts.
- **`--watchAll`.** Every test runs each time.
- **What counts as a change in watch mode.** As in Jest, only a file the haste map would hold, outside `watchPathIgnorePatterns`.
- **Between runs.** The workers stay up, with the transforms and compiled code they hold.
- **What's missing.** Jest's interactive prompt isn't there; Ctrl+C stops.

`compat/test-related.py` generates projects with every kind of edge above, and compares the two runners' `--listTests --json` answers to random queries and to `-o`.

## Vitest's dialect

A project is in Vitest's dialect when its folder has no Jest config but a `vitest.config.*` (or `vite.config.*`), or lists vitest as a dependency; a test file is when it imports from `vitest`, whatever its project. The config is evaluated by Node with `vitest/config` and `vite` stubbed through `module.registerHooks`, which costs about 40 ms instead of the half second loading Vite takes. `include` and `exclude`, `globals`, `setupFiles`, `testTimeout`, the mock-reset flags and `resolve.alias` are read.

Vitest is an alias layer over the Jest semantics above (`runtime/vitest.cjs`, `runtime/vitest-expect.cjs`). Where a difference is one of API, naming or file format, opal takes Vitest's side. That covers:

- the `vitest` module and `vi`;
- the test context and hook cleanups;
- `each` titles, formatted by Vitest's own code;
- snapshot keys, comparison, and thrown-value snapshots;
- Vitest's extra matchers;
- top-level `await` in a test file;
- `import.meta.env`.

Where it is one of semantics (hook order, equality, mock functions, module loading), opal does what Jest does. [`COMPATIBILITY.md`](./COMPATIBILITY.md) lists every difference, with what measures it.

## Checked so far

| check | result |
|---|---|
| `compat/test-lifecycle.py`, seeds 2 to 6, 1,000 files each | 5,000 of 5,000 files identical to Jest, about 32,000 tests, on verdicts and on the order every hook and test ran |
| the same with `-t '[13579]$'` and `-t 'describe1\|test2'` (seed 8) | 2,000 of 2,000 identical |
| `compat/test-expect.cjs`, seeds 1 to 3, 20,000 cases each, against Jest's installed `expect` build | 300,000 calls to `toEqual`, `toStrictEqual`, `toMatchObject`, `toContainEqual` and `toContain`: no verdict differs, and no failure message differs by a byte |
| the same with `--color` (seed 3, 3,000 cases) | 15,000 calls: no colored message differs by a byte |
| the same with a bug planted, `Object.is` replaced by `===` in equality, or `-0` printed as `0` | caught: 380 verdicts, or 1,688 messages, differ in 5,000 cases |
| `compat/test-mock.cjs`, seeds 1 and 2, 3,000 programs of 30 operations each, against Jest's installed `jest-mock` build | 180,000 operations: the full recorded state of every mock identical after each one |
| the same with a bug planted, each call's `this` recorded as `undefined` | caught: 484 of 500 programs differ |
| `compat/test-runners.py --opal`, all 88 Jest-dialect fixtures | identical to Jest in verdicts and in every file written, the known differences included; all 34 planted failures fail |
| `compat/test-snapshot-format.cjs`, seeds 1 and 2, 20,058 values each (the expect generator's, plus mocks, spies, errors with causes, functions, classes, awkward strings, typed arrays and URLs) | every value printed into a snapshot byte for byte as Jest's installed build prints it |
| the same with a bug planted, `-0` printed as `0` | caught: 252 of 4,057 values differ |
| `compat/test-runners.py --opal`, the 23 Vitest-dialect fixtures | Vitest's verdicts and writes on 20; the other 3 differ only as their `opal` pins say (COMPATIBILITY.md) |
| `compat/test-repos.py`: luxon (Jest 29, babel-jest), jest-extended (Jest 30, ts-jest, 357 snapshots with color codes) and es-toolkit (Vitest 4.1, 24 inline snapshots), each run by its own runner and by opal test | luxon 1,222 of 1,222 tests agree, jest-extended 901 of 901; es-toolkit 5,179 of 5,184, the rest declared differences (a function's printed source, Jest's realm isolation, happy-dom files); `opal test -u` writes every committed snapshot back byte for byte |
| `compat/test-transforms.py`: 15 projects under Jest 30.5.2 and opal: ts-jest (type errors in a test, an import, a lazy require; `diagnostics` off, `warnOnly`, `ignoreCodes`, `exclude`, `isolatedModules`, `globals`; decorator metadata), babel-jest (a plugin of the project's own, a `.babelrc` in a subfolder, preset-react's two runtimes), custom transformers, transformer errors and @swc/jest; each opal run cold, warm, and after edits to an import and a global declaration | every verdict, every "failed to run" message (each TSError byte for byte) and every source-mapped frame agree, but for the 2 tests pinned as transpile-only emit differences; a stale answer planted in the type-check's cache, for an import or for a global declaration, is caught |
| the Nest 12 scaffold's own suites (`compat/frameworks/nest`, Vitest, decorators with metadata, ES-module-only `@nestjs/*`), unit and e2e | 1 of 1 and 1 of 1 passing, as under `vitest run` |

## Not yet

- **Fake timers.** `jest.useFakeTimers` and friends throw rather than pass.
- **Jest's failure output in full.** A failing test's message carries its whole stack, opal's own frames included, where Jest leaves its own out and shows a code frame. A file that fails to run gets its stack without opal's frames, but no code frame either. Verdicts don't depend on this; reading the output does.
- **Transformers that only work asynchronously** (`processAsync` alone, or a `createTransformer` that returns a promise), and ES modules as a transform's output: opal loads modules synchronously, as CommonJS.
- **Running the project's prettier on inline snapshots.**
- **Describe-level retries, `--randomize`, test locations in results, and custom test environments.**
- **Yarn Plug'n'Play installs.** opal resolves through `node_modules`, as Jest does without a PnP resolver.
