// The `vitest` module: Vitest's API as an alias layer over opal's Jest
// semantics. What a Vitest test file imports maps onto the Jest framework the
// rest of the runtime implements; where Vitest behaves differently from Jest,
// opal behaves as Jest does, and crates/opal-test/COMPATIBILITY.md lists each
// such place. What Vitest has and Jest has no counterpart for either maps onto
// the nearest Jest behavior (test.concurrent runs in order) or throws saying
// it isn't supported, rather than passing.
//
// Two things differ in shape, not just in name:
//   - Vitest passes a test or hook a context object; Jest would read a
//     function that declares a parameter as waiting for `done`. Every test
//     and hook is wrapped so the context is passed and nothing waits.
//   - A Vitest beforeEach or beforeAll may return a cleanup function, run
//     after the test or the block.
'use strict';

const DYNAMIC_SKIP = Symbol.for('opal.dynamicSkip');

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

// What a test function receives in place of `done`.
function createContext(name, expect) {
  return {
    expect,
    onTestFailed: unsupported('onTestFailed'),
    onTestFinished: unsupported('onTestFinished'),
    signal: new AbortController().signal,
    skip(condition = true, note) {
      if (condition === false) return;
      const signal = new Error(note || 'skipped');
      signal[DYNAMIC_SKIP] = true;
      throw signal;
    },
    task: {name},
  };
}

function createVitest({circus, expect, createJestObject, moduleMocker}) {
  const {globals} = circus;

  const wrapTest = (name, fn) =>
    fn === undefined
      ? undefined
      : function () {
          return fn.call(this, createContext(name, expect));
        };

  // test(name, fn, timeout) or test(name, options, fn).
  function normalize(name, second, third) {
    if (second && typeof second === 'object') return {fn: third, options: second, timeout: second.timeout};
    return {fn: second, options: typeof third === 'object' ? third || {} : {}, timeout: typeof third === 'number' ? third : undefined};
  }

  function makeTest(base) {
    const test = (name, second, third) => {
      const {fn, options, timeout} = normalize(name, second, third);
      if (options.todo) return globals.test.todo(name);
      let target = base;
      if (options.skip) target = globals.test.skip;
      else if (options.only) target = globals.test.only;
      if (options.fails) target = target === globals.test.only ? globals.test.only.failing : globals.test.failing;
      return target(name, wrapTest(name, fn), timeout);
    };
    test.skip = (name, second, third) => {
      const {fn, timeout} = normalize(name, second, third);
      return globals.test.skip(name, wrapTest(name, fn) || (() => {}), timeout);
    };
    test.only = (name, second, third) => {
      const {fn, timeout} = normalize(name, second, third);
      return globals.test.only(name, wrapTest(name, fn), timeout);
    };
    test.todo = name => globals.test.todo(name);
    test.fails = (name, second, third) => {
      const {fn, timeout} = normalize(name, second, third);
      return globals.test.failing(name, wrapTest(name, fn), timeout);
    };
    test.each = (...table) => {
      const bound = globals.test.each(...table);
      return (name, second, third) => {
        const {fn, timeout} = normalize(name, second, third);
        return bound(name, (...args) => fn(...args), timeout);
      };
    };
    test.for = table => (name, second, third) => {
      const {fn, timeout} = normalize(name, second, third);
      return globals.test.each(table.map(row => [row]))(name, row => fn(row, createContext(name, expect)), timeout);
    };
    test.skipIf = condition => (condition ? test.skip : test);
    test.runIf = condition => (condition ? test : test.skip);
    // Run in declaration order, as everything in opal test does.
    test.concurrent = test;
    test.sequential = test;
    test.extend = unsupported('test.extend');
    test.scoped = unsupported('test.scoped');
    return test;
  }

  function makeDescribe() {
    const describe = (name, second, third) => {
      const fn = typeof second === 'function' ? second : third;
      const options = typeof second === 'object' && second ? second : {};
      if (options.skip || options.todo) return globals.describe.skip(name, fn || (() => {}));
      if (options.only) return globals.describe.only(name, fn);
      return globals.describe(name, fn);
    };
    describe.skip = (name, fn) => globals.describe.skip(name, fn || (() => {}));
    describe.only = (name, fn) => globals.describe.only(name, fn);
    describe.todo = name => globals.describe.skip(name, () => {});
    describe.each = (...table) => globals.describe.each(...table);
    describe.for = table => (name, fn) => globals.describe.each(table.map(row => [row]))(name, row => fn(row));
    describe.skipIf = condition => (condition ? describe.skip : describe);
    describe.runIf = condition => (condition ? describe : describe.skip);
    describe.concurrent = describe;
    describe.sequential = describe;
    describe.shuffle = describe;
    return describe;
  }

  // A hook may return a cleanup, run after the test (beforeEach) or after
  // the block (beforeAll), most recent first.
  function makeHook(register, cleanupHook) {
    return (fn, timeout) => {
      const cleanups = [];
      register(async function () {
        const result = await fn.call(this, createContext('', expect));
        if (typeof result === 'function') cleanups.push(result);
      }, timeout);
      if (cleanupHook) {
        cleanupHook(async () => {
          while (cleanups.length > 0) await cleanups.pop()();
        });
      }
    };
  }

  const test = makeTest(globals.test);
  const describe = makeDescribe();
  const hooks = {
    afterAll: makeHook(globals.afterAll),
    afterEach: makeHook(globals.afterEach),
    beforeAll: makeHook(globals.beforeAll, globals.afterAll),
    beforeEach: makeHook(globals.beforeEach, globals.afterEach),
  };

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
      assert: unsupported('chai assert'),
      assertType: () => {},
      bench: unsupported('bench'),
      chai: unsupported('chai'),
      describe,
      expect,
      expectTypeOf: noopChain,
      it: test,
      onTestFailed: unsupported('onTestFailed'),
      onTestFinished: unsupported('onTestFinished'),
      suite: describe,
      test,
      vi,
      vitest: vi,
    };
    Object.defineProperty(module, '__esModule', {value: true});
    modules.set(from, module);
    return module;
  };
}

module.exports = {DYNAMIC_SKIP, createVitest};
