// The `vitest` module: Vitest's API as an alias layer over opal's Jest
// semantics. What a Vitest test file imports maps onto the Jest framework the
// rest of the runtime implements; where Vitest behaves differently from Jest,
// opal behaves as Jest does, and crates/opal-test/COMPATIBILITY.md lists each
// such place. What Vitest has and Jest has no counterpart for either maps onto
// the nearest Jest behavior (test.concurrent runs in order) or throws saying
// it isn't supported, rather than passing.
//
// Some things differ in shape, not just in name, and follow @vitest/runner:
//   - Vitest passes a test and its beforeEach and afterEach hooks one
//     context object; Jest would read a function that declares a parameter
//     as waiting for `done`. Every test and hook is wrapped so the context
//     is passed and nothing waits.
//   - A beforeEach or beforeAll may return a cleanup function, and a test may
//     register onTestFinished and onTestFailed callbacks. They run after the
//     test's afterEach hooks (or the block's afterAll hooks), through
//     circus's teardowns.
//   - context.skip() skips the test whatever fails after it.
//   - test() or describe() inside a running test throws, where Jest fails
//     the running test.
//   - Every variant chains (test.skip.each, describe.todo.each, ...), and a
//     test or describe without a function is a todo.
'use strict';

const display = require('./vitest-display.cjs');

const DYNAMIC_SKIP = Symbol.for('opal.dynamicSkip');

// @vitest/runner's messages for a test or suite defined inside a running test.
const NESTED_TEST =
  'Calling the test function inside another test function is not allowed. Please put it inside "describe" or "suite" so it can be properly collected.';
const NESTED_SUITE =
  'Calling the suite function inside test function is not allowed. It can be only called at the top level or inside another suite function.';

// What `globals: true` puts on the global object (globalApis in
// vitest/src/constants.ts).
const GLOBAL_APIS = [
  'suite',
  'test',
  'describe',
  'it',
  'chai',
  'expect',
  'assert',
  'expectTypeOf',
  'assertType',
  'vitest',
  'vi',
  'beforeAll',
  'afterAll',
  'beforeEach',
  'afterEach',
  'onTestFinished',
  'onTestFailed',
];

const TEST_FLAGS = ['concurrent', 'fails', 'only', 'sequential', 'skip', 'todo'];
const SUITE_FLAGS = ['concurrent', 'only', 'sequential', 'shuffle', 'skip', 'todo'];

function unsupported(name) {
  return () => {
    throw new Error(`opal test: ${name} is not supported.`);
  };
}

// A chain that accepts anything, for type-only assertions.
function noopChain() {
  const chain = new Proxy(function () {}, {
    apply: () => chain,
    get: (_target, name) => (name === 'then' ? undefined : chain),
  });
  return chain;
}

function createVitest({circus, expect, createJestObject, moduleMocker, requireChai}) {
  const {globals, state} = circus;

  // --- a test's context and what runs after it ---------------------------

  // One record per run of a test (a retry starts a new one): the context its
  // hooks and body share, and what @vitest/runner's runTest runs after the
  // afterEach hooks: the beforeEach cleanups, then onTestFinished, each most
  // recent first, then onTestFailed if the test failed.
  const runs = new WeakMap();
  function runOf(test) {
    let run = runs.get(test);
    if (run && run.invocation === test.invocations) return run;
    run = {cleanups: [], context: null, failed: [], finished: [], invocation: test.invocations, scheduled: false};
    run.context = createContext(test, run);
    runs.set(test, run);
    return run;
  }

  function schedule(test, list, fn) {
    const run = runOf(test);
    if (!run.scheduled) {
      run.scheduled = true;
      const drain = (callbacks, withContext) => async () => {
        for (const callback of callbacks.splice(0).reverse()) {
          await (withContext ? callback(run.context) : callback());
        }
      };
      (test.teardowns ??= []).push(drain(run.cleanups, false), drain(run.finished, true), async () => {
        if (test.errors.length > 0 && !test.dynamicSkip) await drain(run.failed, true)();
      });
    }
    run[list].push(fn);
  }

  // @vitest/runner's createTestHook: only while a test runs.
  const testHook = (name, list) => fn => {
    if (typeof fn !== 'function') throw new TypeError(`"${name}" callback value must be function, received "${typeof fn}"`);
    const test = state.currentlyRunningTest;
    if (!test) throw new Error(`Hook ${name}() can only be called inside a test`);
    schedule(test, list, fn);
  };

  function createContext(test) {
    return {
      expect,
      onTestFailed: fn => schedule(test, 'failed', fn),
      onTestFinished: fn => schedule(test, 'finished', fn),
      signal: new AbortController().signal,
      skip(condition, note) {
        if (condition === false) return;
        test.dynamicSkip = true;
        const signal = new Error(typeof condition === 'string' ? condition : note || 'skipped');
        signal[DYNAMIC_SKIP] = true;
        throw signal;
      },
      task: {name: test.name},
    };
  }

  const contextOfRunningTest = () => (state.currentlyRunningTest ? runOf(state.currentlyRunningTest).context : undefined);

  const wrapTest = fn =>
    function () {
      return fn.call(this, contextOfRunningTest());
    };

  // --- test and describe ---------------------------------------------------

  // @vitest/runner's chainable API: every modifier is a property that adds a
  // flag, and every combination has each, for, skipIf and runIf. Arguments
  // are read, and each and for format titles, with Vitest's own code
  // (vitest-display.cjs).
  function chainable(flagNames, define, flags = {}) {
    const fn = (name, optionsOrFn, timeoutOrFn) => {
      const {options, handler} = display.parseArguments(optionsOrFn, timeoutOrFn);
      return define({...flags, ...options}, display.formatName(name), handler, options.timeout);
    };
    for (const flag of flagNames) {
      Object.defineProperty(fn, flag, {get: () => chainable(flagNames, define, {...flags, [flag]: true})});
    }
    // A row's items are the arguments, unless some row isn't an array.
    fn.each = (cases, ...args) => {
      if (Array.isArray(cases) && args.length > 0) cases = display.formatTemplateString(cases, args);
      return (name, optionsOrFn, timeoutOrFn) => {
        const title = display.formatName(name);
        const arrayOnly = cases.every(Array.isArray);
        const {options, handler} = display.parseArguments(optionsOrFn, timeoutOrFn);
        cases.forEach((row, index) => {
          const items = Array.isArray(row) ? row : [row];
          const call = handler && (arrayOnly ? () => handler(...items) : () => handler(row));
          define({...flags, ...options}, display.formatTitle(title, items, index), call, options.timeout);
        });
      };
    };
    // The row whole, then the test's context.
    fn.for = (cases, ...args) => {
      if (Array.isArray(cases) && args.length > 0) cases = display.formatTemplateString(cases, args);
      return (name, optionsOrFn, timeoutOrFn) => {
        const title = display.formatName(name);
        const {options, handler} = display.parseArguments(optionsOrFn, timeoutOrFn);
        cases.forEach((row, index) => {
          const call = handler && (context => handler(row, context));
          define({...flags, ...options}, display.formatTitle(title, display.toArray(row), index), call, options.timeout);
        });
      };
    };
    fn.skipIf = condition => (condition ? fn.skip : fn);
    fn.runIf = condition => (condition ? fn : fn.skip);
    return fn;
  }

  // Vitest's modes: only, then skip, then todo; without a function, todo.
  function defineTest(flags, name, fn, timeout) {
    if (state.currentlyRunningTest) throw new Error(NESTED_TEST);
    if (flags.only && fn) return (flags.fails ? globals.test.only.failing : globals.test.only)(name, wrapTest(fn), timeout);
    if (flags.skip) return globals.test.skip(name, fn ? wrapTest(fn) : () => {}, timeout);
    if (flags.todo || !fn) return globals.test.todo(name);
    // Run in declaration order, as everything in opal test does.
    return (flags.fails ? globals.test.failing : globals.test)(name, wrapTest(fn), timeout);
  }

  function defineSuite(flags, name, fn) {
    if (state.currentlyRunningTest) throw new Error(NESTED_SUITE);
    let mode;
    if (flags.only) mode = 'only';
    else if (flags.skip) mode = 'skip';
    else if (flags.todo || !fn) mode = 'todo';
    return circus.describeWithMode(name, fn || (() => {}), mode);
  }

  const test = chainable(TEST_FLAGS, defineTest);
  test.extend = unsupported('test.extend');
  test.scoped = unsupported('test.scoped');
  const describe = chainable(SUITE_FLAGS, defineSuite);

  // --- hooks ---------------------------------------------------------------

  // beforeEach and afterEach get the test's context; beforeAll and afterAll
  // the block. A beforeEach's cleanup runs after the test's afterEach hooks,
  // a beforeAll's after the block's afterAll hooks, most recent first.
  function makeHook(register, kind) {
    return (fn, timeout) => {
      const block = state.currentDescribeBlock;
      register(async function () {
        const test = state.currentlyRunningTest;
        const argument = kind === 'each' && test ? runOf(test).context : {name: block.name, type: 'suite'};
        const result = await fn.call(this, argument);
        if (typeof result !== 'function') return;
        if (kind === 'each' && test) schedule(test, 'cleanups', result);
        else if (kind === 'all') (block.teardowns ??= []).unshift(result);
      }, timeout);
    };
  }

  const hooks = {
    afterAll: makeHook(globals.afterAll),
    afterEach: makeHook(globals.afterEach, 'each'),
    beforeAll: makeHook(globals.beforeAll, 'all'),
    beforeEach: makeHook(globals.beforeEach, 'each'),
  };

  // --- vi ------------------------------------------------------------------

  const stubbedGlobals = new Map();
  const stubbedEnv = new Map();

  // A factory's object is the module's namespace: its `default` is the
  // default export, as it is in Vitest.
  const asNamespace = value => {
    if (value && typeof value === 'object' && !('__esModule' in value)) {
      Object.defineProperty(value, '__esModule', {value: true});
    }
    return value;
  };
  const namespaceOf = exports =>
    exports && exports.__esModule ? exports : {...exports, default: exports};

  function createVi(from) {
    const jest = createJestObject(from);
    const mockFactory = (path, factory) => () => {
      const result = factory(() => Promise.resolve(namespaceOf(jest.requireActual(path))));
      if (result && typeof result.then === 'function') {
        throw new Error(
          `opal test: the vi.mock factory for '${path}' returned a Promise; opal test loads modules synchronously and supports synchronous factories only.`,
        );
      }
      return asNamespace(result);
    };
    const poll = async (callback, options, accept) => {
      const {interval = 50, timeout = 1000} = typeof options === 'number' ? {timeout: options} : options || {};
      const start = Date.now();
      for (;;) {
        try {
          const value = await callback();
          if (accept(value)) return value;
          if (Date.now() - start > timeout) throw new Error('Timed out in waitUntil!');
        } catch (error) {
          if (Date.now() - start > timeout) throw error;
        }
        await new Promise(resolve => setTimeout(resolve, interval));
      }
    };
    const fakeTimers = unsupported('fake timers');
    const vi = {
      advanceTimersByTime: fakeTimers,
      advanceTimersByTimeAsync: fakeTimers,
      advanceTimersToNextTimer: fakeTimers,
      clearAllMocks() {
        moduleMocker.clearAllMocks();
        return vi;
      },
      clearAllTimers: fakeTimers,
      doMock(path, factory) {
        if (factory) jest.doMock(path, mockFactory(path, factory));
        else jest.doMock(path);
        return vi;
      },
      doUnmock(path) {
        jest.dontMock(path);
        return vi;
      },
      dynamicImportSettled: async () => {},
      fn: implementation => moduleMocker.fn(implementation),
      getMockedSystemTime: () => null,
      getRealSystemTime: () => Date.now(),
      hoisted: factory => factory(),
      importActual: async path => namespaceOf(jest.requireActual(path)),
      importMock: async path => namespaceOf(jest.requireMock(path)),
      isFakeTimers: () => false,
      isMockFunction: fn => moduleMocker.isMockFunction(fn),
      mock(path, factory) {
        if (typeof factory === 'function') jest.mock(path, mockFactory(path, factory));
        else jest.mock(path);
        return vi;
      },
      mocked: source => source,
      resetAllMocks() {
        moduleMocker.resetAllMocks();
        return vi;
      },
      resetConfig() {},
      resetModules() {
        jest.resetModules();
        return vi;
      },
      restoreAllMocks() {
        moduleMocker.restoreAllMocks();
        return vi;
      },
      runAllTicks: fakeTimers,
      runAllTimers: fakeTimers,
      runAllTimersAsync: fakeTimers,
      runOnlyPendingTimers: fakeTimers,
      setConfig() {},
      setSystemTime: fakeTimers,
      spyOn: (object, method, accessType) => moduleMocker.spyOn(object, method, accessType),
      stubEnv(name, value) {
        if (!stubbedEnv.has(name)) stubbedEnv.set(name, process.env[name]);
        process.env[name] = value;
        return vi;
      },
      stubGlobal(name, value) {
        if (!stubbedGlobals.has(name)) stubbedGlobals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
        Object.defineProperty(globalThis, name, {configurable: true, enumerable: true, value, writable: true});
        return vi;
      },
      unmock(path) {
        jest.unmock(path);
        return vi;
      },
      unstubAllEnvs() {
        for (const [name, value] of stubbedEnv) {
          if (value === undefined) delete process.env[name];
          else process.env[name] = value;
        }
        stubbedEnv.clear();
        return vi;
      },
      unstubAllGlobals() {
        for (const [name, descriptor] of stubbedGlobals) {
          if (descriptor) Object.defineProperty(globalThis, name, descriptor);
          else delete globalThis[name];
        }
        stubbedGlobals.clear();
        return vi;
      },
      useFakeTimers: fakeTimers,
      useRealTimers: () => vi,
      waitFor: (callback, options) => poll(callback, options, () => true),
      waitUntil: (callback, options) => poll(callback, options, Boolean),
    };
    return vi;
  }

  if (!expect.unreachable) {
    expect.unreachable = message => {
      throw new Error(message ? `expected not to reach this point: ${message}` : 'expected not to reach this point');
    };
  }

  // One `vitest` module per requiring file, since `vi.mock` resolves paths
  // from the file that calls it.
  const modules = new Map();
  return function vitestFor(from) {
    let module = modules.get(from);
    if (module) return module;
    const vi = createVi(from);
    module = {
      ...hooks,
      assertType: () => {},
      bench: unsupported('bench'),
      describe,
      expect,
      expectTypeOf: noopChain,
      it: test,
      onTestFailed: testHook('onTestFailed', 'failed'),
      onTestFinished: testHook('onTestFinished', 'finished'),
      suite: describe,
      test,
      vi,
      vitest: vi,
    };
    // assert, chai and should are chai's own, loaded when first used.
    for (const name of ['assert', 'chai', 'should']) {
      Object.defineProperty(module, name, {
        configurable: true,
        enumerable: true,
        get() {
          const chai = requireChai(from);
          return name === 'chai' ? chai : name === 'should' ? chai.should : chai.assert;
        },
      });
    }
    Object.defineProperty(module, '__esModule', {value: true});
    modules.set(from, module);
    return module;
  };
}

// The globals of `globals: true`, as descriptors, so chai still loads only
// when it's used.
const globalsOf = module =>
  Object.fromEntries(GLOBAL_APIS.map(name => [name, Object.getOwnPropertyDescriptor(module, name)]));

module.exports = {DYNAMIC_SKIP, createVitest, globalsOf};
