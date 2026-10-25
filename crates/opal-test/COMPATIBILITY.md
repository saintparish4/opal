# opal test: Vitest compatibility

opal test runs a Vitest project with Vitest's API, Vitest's snapshot files and Vitest's test names, over the Jest semantics that [`SEMANTICS.md`](./SEMANTICS.md) describes. This file lists every place where a Vitest suite behaves differently under opal than under Vitest 4.1.12, and says which side each one takes. Each entry is measured, not assumed. The harnesses that measure them are listed in [How this is checked](#how-this-is-checked).

- [Which files are in Vitest's dialect](#which-files-are-in-vitests-dialect)
- [What follows Vitest](#what-follows-vitest)
- [Differences](#differences)
  - [Running tests](#running-tests)
  - [expect](#expect)
  - [Snapshots](#snapshots)
  - [Mock functions](#mock-functions)
  - [Modules](#modules)
  - [Configuration and command line](#configuration-and-command-line)
- [How this is checked](#how-this-is-checked)

## Which files are in Vitest's dialect

- **A project.** A project is in Vitest's dialect when its folder has no Jest config but has a `vitest.config.*` (or a `vite.config.*`), or lists `vitest` and not `jest` as a dependency. `--config vitest.config.ts` picks it too.
- **A file.** A test file that imports from `vitest` is in Vitest's dialect whatever its project is, since a Jest project couldn't run it otherwise.

## What follows Vitest

The alias layer (`runtime/vitest.cjs`, `runtime/vitest-expect.cjs`) gives a Vitest file Vitest's behavior wherever that is a matter of API, naming or file format, not of Jest's core semantics:

- **The `vitest` module.**
  - `describe`/`suite`, `test`/`it` and every chained variant (`skip`, `only`, `todo`, `fails`, `concurrent`, `sequential`, `shuffle`, `each`, `for`, `skipIf`, `runIf`), with Vitest's argument order: `(name, options, fn)` or `(name, fn, timeout)`.
  - The hooks.
  - `onTestFinished` and `onTestFailed`.
  - `vi` and `vitest`.
  - `expectTypeOf` and `assertType`, which check nothing at run time, as in Vitest.
  - `assert`, `chai` and `should`, from the chai the project's vitest depends on.
- **Defining tests.**
  - A test or describe without a function is a todo, and `describe.todo` makes everything in it a todo.
  - `test()` or `describe()` called inside a running test throws Vitest's error. Jest's dialect records the error on the running test instead.
- **Titles.**
  - `each` and `for` titles are formatted by Vitest's own `formatTitle`, `formatTemplateString` and loupe, ported from the pinned install by [`port/port-vitest.mjs`](./port/port-vitest.mjs): `$a` prints a string as `'a'` and an object as `{ a: 1 }`.
  - A row reaches the test as Vitest passes it.
  - Test names join with ` > `, so `expect.getState().currentTestName` and snapshot keys are Vitest's.
- **Test context.**
  - A test and its `beforeEach` and `afterEach` hooks share one context object, with `expect`, `skip`, `onTestFinished`, `onTestFailed`, `signal` and `task.name`. Nothing waits for a `done` callback.
  - `context.skip()` marks the test skipped and throws. Whatever fails after that, an unhandled rejection from a callback included, the test still reports as skipped.
- **Teardown.**
  - A `beforeEach` that returns a function has it run after the test's `afterEach` hooks. A `beforeAll` that returns one has it run after the block's `afterAll` hooks. Both run most recent first.
  - `onTestFinished` callbacks run next, then `onTestFailed` callbacks if the test failed, each most recent first. This is @vitest/runner's `runTest` order.
- **Top-level `await`.** A test file or setup file may `await` at its top level. The run starts once what it awaits settles.
- **`import.meta`.** `import.meta.env` is the test's `process.env`, read through Vitest's proxy, so `DEV`, `PROD` and `SSR` are booleans. Vite's values are set where unset: `MODE` is `test`, `DEV` and `SSR` are true, `PROD` is false and `BASE_URL` is `/`. `import.meta` is one object per module, as it is in Vitest.
- **Environment.** A Vitest file's `process.env` has `VITEST`, `TEST`, `VITEST_MODE`, `VITEST_POOL_ID` and `VITEST_WORKER_ID`, and no `JEST_WORKER_ID`. Both dialects get `NODE_ENV=test` when it isn't set.
- **expect additions.**
  - Vitest's matchers that Jest lacks:
    - `toHaveBeenCalledOnce`, `toHaveBeenCalledExactlyOnceWith`, `toHaveBeenCalledBefore` and `toHaveBeenCalledAfter`;
    - `toHaveResolved`, `toHaveResolvedTimes`, `toHaveResolvedWith`, `toHaveLastResolvedWith` and `toHaveNthResolvedWith`;
    - `toBeTypeOf`, `toBeOneOf`, `toSatisfy` and `toBeNullable`.
  - The aliases Jest 30 removed and Vitest keeps: `toBeCalled`, `toBeCalledTimes`, `toBeCalledWith`, `lastCalledWith`, `nthCalledWith`, `toReturn`, `toReturnTimes`, `toReturnWith`, `lastReturnedWith`, `nthReturnedWith` and `toThrowError`.
  - `expect.soft`, `expect.poll` and `expect.unreachable`.
- **Snapshot files.**
  - Vitest's header, its ` > ` keys, a hint joined with ` > `, and Vitest's comparison, which trims both sides.
  - `toThrowErrorMatchingSnapshot` and its inline form snapshot the thrown value itself (`[Error: msg]`) where Jest snapshots its message. A non-function `expect` argument throws Vitest's `expected must be a function`.
- **Mocks.** `mock.settledResults` is recorded, and a spy is named after the function it spies on, as `[MockFunction method]` in a snapshot. `mockReset` keeps that name.
- **Manual mocks.** A root `__mocks__` file is used only when `vi.mock` asks for it, where Jest's dialect substitutes it without being asked.
- **Imports.** `./a.js` resolves to `./a.ts` as Vite resolves it.

## Differences

Each entry names the side opal takes. "Jest's" means opal keeps Jest's behavior, as the brief requires of an alias layer over Jest's semantics. "Not supported" means the API throws a clear error, or is not provided, rather than passing.

### Running tests

- **Hook order. Jest's.** Vitest runs `afterEach` and `afterAll` hooks in reverse order of declaration within a block (`sequence.hooks: 'stack'`, its default). opal runs them in declaration order, as Jest does and as Vitest does with `sequence.hooks: 'list'`. `sequence.hooks: 'parallel'` isn't read either.
- **Concurrency. Jest's dialect has none either.** `test.concurrent`, `describe.concurrent` and `sequence.concurrent` run tests one at a time, in order; concurrent tests are outside opal test's scope. `describe.shuffle` and `sequence.shuffle` run in order.
- **An async `describe` callback. Jest's.** Vitest awaits a describe callback that returns a promise. opal fails the file with Jest's "Returning a Promise from "describe" is not supported".
- **Timeouts.** Hooks get the test timeout (5 s by default, `testTimeout` in the config), as in Jest. Vitest's separate `hookTimeout` (10 s by default) isn't read.
- **Test options.** `timeout`, `skip`, `only`, `todo` and `fails` are read. `retry` and `repeats` aren't: a Vitest test with `retry` runs once. A describe's options apply only their mode.
- **Fixtures. Not supported.** `test.extend` and `test.scoped` throw. So do `bench` and the fake timers (`vi.useFakeTimers` and the rest), as `jest.useFakeTimers` does in Jest's dialect. `aroundEach` and `aroundAll` aren't provided.
- **The test context.** `context.task` has only `name`. `context.signal` never aborts. `context.annotate` isn't provided. `onTestFinished` and `onTestFailed` callbacks don't get Vitest's own timeout.
- **Unhandled errors. Jest's.** An error thrown or rejected outside a test's own promise chain is charged to whatever was running when it was raised, by Jest's rules (SEMANTICS.md). Vitest collects such errors and fails the run.

### expect

opal's `expect` is Jest's, in Vitest's dialect too. Vitest's is chai's, with Jest-compatible matchers of its own. Run on the same 20,000 generated pairs of values (`node compat/test-expect.cjs --against vitest`, seed 1), `toEqual`, `toStrictEqual`, `toMatchObject`, `toContainEqual` and `toContain` give 100,000 verdicts. 4,157 of them differ, and every one comes from one of the causes below. All are Jest's behavior in opal.

| cause | verdicts | what Vitest does | what opal (Jest) does |
|---|---|---|---|
| `toMatchObject` on a value that isn't an object | 2,490 | compares it: `expect(1).toMatchObject(1)` passes | fails with "received value must be a non-null object" (or "expected value") |
| Errors | 824 | compares `name`, `message`, `cause` and own enumerable properties | compares `message` only, so `new Error('m')` equals `new TypeError('m')` |
| an `undefined` array element | 655 | counts it: `[]` doesn't equal `[undefined]` | ignores it, as it ignores a property holding `undefined` |
| Invalid Dates | 173 | two Invalid Dates are equal | they aren't (`NaN` isn't equal to `NaN` as a time) |
| `toMatchObject` with symbol keys | 7 | ignores the expected object's symbol keys | checks them |
| `toContainEqual` with Maps and Sets | 6 | compares items without iterating them, so any Map equals any Map | compares their contents |
| a symbol key holding `undefined` | 2 | fails `toEqual` even when both sides hold it | passes |

Two more differences, outside what the generator makes:

- **Circular values reached through two Sets or Maps. Jest's.** Jest's equality overflows the stack on these, and so does opal's; Vitest fixed its copy (vitest#5533). The cause is pinned in `compat/test-runners/corpus/known_difference/vitest/expect-circular.ts`.
- **Vitest's own expect packages. Not supported.** A test file that imports `@vitest/expect` fails to load: that package claims the global matcher registry, `Symbol.for('$$jest-matchers-object')`, which opal's expect, like Jest's, already holds. Only Vitest's own test suite does this (`jest-expect.ts` in the corpus).

Beyond verdicts:

- **Failure messages.** They are Jest's, for every matcher, Vitest's additions included. Vitest's are chai's (`expected 1 to be 2 // Object.is equality`).
- **chai's own assertion API.** `expect(x).to.equal(y)`, `.to.be.true`, `.to.have.been.called` and the rest aren't available on `expect`. `assert`, `chai` and `should` from `vitest` are chai's own.
- **`expect.poll`.** It retries until the matcher passes or the time is up (default: every 50 ms for 1 s, as in Vitest). On timeout it throws the matcher's last error, not Vitest's own timeout error. `expect.soft` records a failure on the running test and carries on, as Vitest's does.
- **Not provided:** `expect.schemaMatching`, `expect.assert`, and the `Snapshots` export for custom snapshot matchers.

### Snapshots

- **Printing values. Same as Vitest.** opal prints a value into a snapshot with Jest's pretty-format and snapshot plugins. On 20,058 values, the generator's plus a fixed list of mocks, spies, errors with causes, functions, classes, awkward strings, typed arrays and URLs, that prints every one byte for byte as Vitest's serializer does (`node compat/test-snapshot-format.cjs --against vitest`). So a Vitest project's committed `.snap` files read back as written.
- **Writing an inline snapshot.** opal writes a new or changed inline snapshot the way it does in Jest's dialect: the call stays as written, and the snapshot is indented by Jest's rules. Vitest indents a multi-line inline snapshot by its own rules (`prepareSnapString`), so the bytes it writes into the test file can differ. A snapshot that matches reads the same either way.
- **`toMatchFileSnapshot`. Not supported.**
- **Configuration.** `snapshotFormat`, `snapshotSerializers`, `resolveSnapshotPath` and `snapshotEnvironment` in a Vitest config aren't read. The defaults, `escapeString: false` and `printBasicPrototype: false`, are Vitest's.

### Mock functions

opal's mocks are jest-mock's, with `settledResults` and spy names added. Where @vitest/spy behaves differently, opal behaves as jest-mock does, Jest's in every case. `compat/test-runners/corpus/known_difference/vitest/jest-mock.ts` pins the corpus tests these fail.

- **`mockReset`.** Vitest goes back to the implementation given to `vi.fn(impl)`, or to the spied original. jest-mock leaves no implementation, so the mock returns `undefined`. The `mockReset` config flag and `mockRestore` follow the same rule.
- **`getMockImplementation()`.** Vitest returns `vi.fn(impl)`'s implementation and a pending `mockImplementationOnce`. jest-mock returns only what `mockImplementation` set.
- **`mock.mock` persistence.** Vitest keeps one `mock.mock` object for a mock's life. jest-mock replaces it on `mockClear`, so a saved reference goes stale.
- **Classes.** Vitest calls a class implementation with `new` and can spy on a constructor. jest-mock calls the implementation as a function, which throws for a class.
- **Spying on an inherited method.** jest-mock puts the spy on the object as an enumerable own property. Vitest copies the prototype's descriptor, which isn't enumerable.
- **Disposal.** jest-mock's `[Symbol.dispose]` restores the mock without calling its `mockRestore` property.
- **Default name.** `vi.fn().getMockName()` is `jest.fn()` in opal and `vi.fn()` in Vitest. Both print as `[MockFunction]` in a snapshot. Failure messages name the mock `jest.fn()`.
- **Automocks.** `vi.mock(path)` without a factory or a manual mock generates the module's mock by Jest's algorithm, not Vitest's automocker. For example, Vitest 4 no longer calls an automocked getter, and it shares an automocked method's state with the prototype. jest-mock does neither.

### Modules

There is no Vite in opal test. A Vitest file's imports are resolved by Node's rules, transformed by opal's own transform (`src/transform.rs`) into CommonJS with Babel's interop, and loaded into the test's context.

- **Vite plugins. Not run.** A transform that Vite plugins provide doesn't happen: `.vue`, `.svelte`, `.astro`, CSS modules, `?raw`, `?url` and other query imports, virtual modules, `define` replacements and `.env` files. `resolve.alias` is read, as `moduleNameMapper`; `resolve.conditions`, `resolve.tsconfigPaths` (tsconfig's `paths`) and `test.alias` aren't.
- **ES modules run as CommonJS.** Imports are hoisted `require`s with live bindings, and `import()` becomes a `require` behind a promise. Only a test or setup file may await at its top level; a module it imports may not.
- **`vi.mock` factories.**
  - A factory's object is the module's namespace, as in Vitest, and `importOriginal` is provided.
  - An async factory isn't supported: modules load synchronously, so a factory that returns a promise throws.
  - `vi.mock(import('./path'))`, the typed form, isn't recognized.
- **Mocks across packages. Jest's.** `vi.mock` applies to every module that requires the mocked one, inside `node_modules` too, as `jest.mock` does. Vitest mocks only modules it processes itself, unless `server.deps.inline` lists them.
- **`vi.importActual` and `vi.importMock`.** Both return a promise, as in Vitest, of a module loaded synchronously.

### Configuration and command line

- **What a Vitest config gives opal.** The config is evaluated with `vitest/config` and `vite` stubbed, so a config that needs more of Vite falls back to loading it. These are read:
  - `test.include` and `test.exclude`;
  - `test.globals`, `test.setupFiles` and `test.testTimeout`;
  - `test.clearMocks`, `test.mockReset` and `test.restoreMocks`;
  - `test.root` (or `root`), and `resolve.alias`.

  Everything else is ignored. In particular:
  - `projects` and `workspace`;
  - `environment`: tests run in Node, whatever it says. jsdom and happy-dom are outside opal test's scope, so a DOM test fails rather than passing;
  - `globalSetup`, `pool`, `isolate`, `sequence`, `retry`, `hookTimeout` and `bail`;
  - `env`, `unstubEnvs` and `unstubGlobals`;
  - `typecheck`, `includeSource` (in-source tests, `import.meta.vitest`), `coverage` and `reporters`.
- **Isolation.** Every test file runs in its own context with its own module registry (SEMANTICS.md), as Vitest's default `isolate: true` does. `isolate: false` doesn't share modules between files.
- **Command line and output. Jest's.** `opal test` takes Jest's flags (`-t`/`--testNamePattern`, `-u`, `--ci`, `--json`, `--outputFile`), prints Jest's report, and writes Jest's JSON. `vitest run`'s flags and reporters aren't accepted.

## How this is checked

| check | result |
|---|---|
| `compat/test-runners.py --opal`, the corpus's 23 Vitest-dialect fixtures from Vitest's own suite | Vitest's verdicts and written files on 20. The other 3 differ only as their `opal` pins say: `expect-circular.ts`, `jest-mock.ts` and `jest-expect.ts`, above |
| `compat/test-expect.cjs --against vitest`, 20,000 cases | 4,157 of 100,000 verdicts differ from Vitest's, every one from a cause in the table above |
| `compat/test-snapshot-format.cjs --against vitest`, 20,058 values | every one printed byte for byte as Vitest prints it |
| `compat/test-snapshot-format.cjs`, the same values in Jest's dialect | every one printed byte for byte as Jest prints it |
| the Nest 12 scaffold's own suite (Vitest, decorators with metadata) | passes, as under `vitest run` |
