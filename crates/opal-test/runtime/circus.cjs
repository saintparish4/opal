// The test lifecycle: a port of jest-circus 30.5.2 (index.ts, state.ts,
// eventHandler.ts, run.ts, utils.ts, testExecutionContext.ts,
// globalErrorHandlers.ts, unhandledRejectionHandler.ts) and the part of
// legacy-code-todo-rewrite/jestAdapterInit.ts that turns a run into Jest's
// result. Every rule about which hooks run, in what order, which tests are
// skipped, and where an error is charged is Jest's, kept in Jest's order so
// a reader can hold the two side by side; SEMANTICS.md lists them.
//
// Left out: describe-level retries (`jest.retryTimes` on a describe block),
// randomized order, test location in results, and custom environments'
// handleTestEvent. `jest.retryTimes` for tests is here.
//
// One instance per test file, created inside that file's context.
'use strict';

const {AsyncLocalStorage} = require('async_hooks');
const {format: prettyFormat} = require('./format.cjs');
const each = require('./each.cjs');
const {
  ErrorWithStack,
  convertDescriptorToString,
  flattenErrorStack,
  formatTime,
  isPromise,
} = require('./errors.cjs');

const ROOT_DESCRIBE_BLOCK_NAME = 'ROOT_DESCRIBE_BLOCK';
const RETRY_TIMES = Symbol.for('RETRY_TIMES');
const RETRY_IMMEDIATELY = Symbol.for('RETRY_IMMEDIATELY');
const WAIT_BEFORE_RETRY = Symbol.for('WAIT_BEFORE_RETRY');
const LOG_ERRORS_BEFORE_RETRY = Symbol.for('LOG_ERRORS_BEFORE_RETRY');
const TEST_TIMEOUT_SYMBOL = Symbol.for('TEST_TIMEOUT_SYMBOL');

// Captured before any test file runs, so fake timers or a test that replaces
// them can't stop a timeout from firing.
const {setTimeout, clearTimeout} = globalThis;

const DYNAMIC_SKIP = Symbol.for('opal.dynamicSkip');

// `testNameSeparator` joins describe and test titles into the name expect
// keys snapshots by: ' ' in Jest's dialect, ' > ' in Vitest's.
function createCircus({expect, parentProcess, testNameSeparator = ' '}) {
  // --- state ---------------------------------------------------------------

  const makeDescribe = (name, parent, mode) => {
    let _mode = mode;
    if (parent && !mode) _mode = parent.mode;
    return {
      type: 'describeBlock',
      children: [],
      hooks: [],
      mode: _mode,
      name: convertDescriptorToString(name),
      parent,
      tests: [],
    };
  };

  const makeTest = (fn, mode, concurrent, name, parent, timeout, asyncError, failing) => ({
    type: 'test',
    asyncError,
    concurrent,
    duration: null,
    errors: [],
    failing,
    fn,
    invocations: 0,
    mode,
    name: convertDescriptorToString(name),
    numPassingAsserts: 0,
    parent,
    retryReasons: [],
    seenDone: false,
    startedAt: null,
    status: null,
    timeout,
    unhandledRejectionErrorByPromise: new Map(),
  });

  const root = makeDescribe(ROOT_DESCRIBE_BLOCK_NAME);
  const state = {
    currentDescribeBlock: root,
    currentlyRunningTest: null,
    hasFocusedTests: false,
    hasStarted: false,
    includeTestLocationInResult: false,
    maxConcurrency: 5,
    originalGlobalErrorHandlers: null,
    rootDescribeBlock: root,
    testNamePattern: null,
    testTimeout: 5000,
    unhandledErrors: [],
    unhandledRejectionErrorByPromise: new Map(),
    unhandledRejectionErrorByPromiseByHook: new WeakMap(),
    unhandledRejectionErrorByPromiseTarget: new WeakMap(),
  };

  const executionContext = new AsyncLocalStorage();
  const getTestExecutionContext = () => executionContext.getStore();
  const runInTestExecutionContext = (context, callback) =>
    executionContext.run({...executionContext.getStore(), ...context}, callback);

  // --- events --------------------------------------------------------------

  const handlers = [eventHandler, adapterEventHandler, unhandledRejectionHandler];

  async function dispatch(event) {
    for (const handler of handlers) await handler(event, state);
  }

  function dispatchSync(event) {
    for (const handler of handlers) handler(event, state);
  }

  function eventHandler(event, state) {
    switch (event.name) {
      case 'include_test_location_in_result':
        state.includeTestLocationInResult = true;
        break;
      case 'hook_start':
        event.hook.seenDone = false;
        break;
      case 'start_describe_definition': {
        const {blockName, mode} = event;
        const {currentDescribeBlock, currentlyRunningTest} = state;
        if (currentlyRunningTest) {
          currentlyRunningTest.errors.push(
            new Error(
              `Cannot nest a describe inside a test. Describe block "${blockName}" cannot run because it is nested within "${currentlyRunningTest.name}".`,
            ),
          );
          break;
        }
        const describeBlock = makeDescribe(blockName, currentDescribeBlock, mode);
        currentDescribeBlock.children.push(describeBlock);
        state.currentDescribeBlock = describeBlock;
        break;
      }
      case 'finish_describe_definition': {
        const {currentDescribeBlock} = state;
        if (!describeBlockHasTests(currentDescribeBlock)) {
          for (const hook of currentDescribeBlock.hooks) {
            hook.asyncError.message = `Invalid: ${hook.type}() may not be used in a describe block containing no tests.`;
            state.unhandledErrors.push(hook.asyncError);
          }
        }
        // Pass the block's mode to its tests, unless it is an `only` block
        // that already holds an `only` test.
        const shouldPassMode = !(
          currentDescribeBlock.mode === 'only' &&
          currentDescribeBlock.children.some(child => child.type === 'test' && child.mode === 'only')
        );
        if (shouldPassMode) {
          for (const child of currentDescribeBlock.children) {
            if (child.type === 'test' && !child.mode) child.mode = currentDescribeBlock.mode;
          }
        }
        if (
          !state.hasFocusedTests &&
          currentDescribeBlock.mode !== 'skip' &&
          currentDescribeBlock.children.some(child => child.type === 'test' && child.mode === 'only')
        ) {
          state.hasFocusedTests = true;
        }
        if (currentDescribeBlock.parent) state.currentDescribeBlock = currentDescribeBlock.parent;
        break;
      }
      case 'add_hook': {
        const {currentDescribeBlock, currentlyRunningTest, hasStarted} = state;
        const {asyncError, fn, hookType: type, timeout} = event;
        if (currentlyRunningTest) {
          currentlyRunningTest.errors.push(
            new Error(
              `Hooks cannot be defined inside tests. Hook of type "${type}" is nested within "${currentlyRunningTest.name}".`,
            ),
          );
          break;
        } else if (hasStarted) {
          state.unhandledErrors.push(
            new Error('Cannot add a hook after tests have started running. Hooks must be defined synchronously.'),
          );
          break;
        }
        currentDescribeBlock.hooks.push({
          asyncError,
          fn,
          parent: currentDescribeBlock,
          seenDone: false,
          timeout,
          type,
        });
        break;
      }
      case 'add_test': {
        const {currentDescribeBlock, currentlyRunningTest, hasStarted} = state;
        const {asyncError, fn, mode, testName: name, timeout, concurrent, failing} = event;
        if (currentlyRunningTest) {
          currentlyRunningTest.errors.push(
            new Error(
              `Tests cannot be nested. Test "${name}" cannot run because it is nested within "${currentlyRunningTest.name}".`,
            ),
          );
          break;
        } else if (hasStarted) {
          state.unhandledErrors.push(
            new Error('Cannot add a test after tests have started running. Tests must be defined synchronously.'),
          );
          break;
        }
        const test = makeTest(fn, mode, concurrent, name, currentDescribeBlock, timeout, asyncError, failing);
        if (currentDescribeBlock.mode !== 'skip' && test.mode === 'only') state.hasFocusedTests = true;
        currentDescribeBlock.children.push(test);
        currentDescribeBlock.tests.push(test);
        break;
      }
      case 'hook_failure': {
        const {test, describeBlock, error, hook} = event;
        const {asyncError, type} = hook;
        if (type === 'beforeAll') {
          addErrorToEachTestUnderDescribe(describeBlock, error, asyncError);
        } else if (type === 'afterAll') {
          // Charging an afterAll error to each test would make a global
          // afterAll hold every test's status until it ran.
          state.unhandledErrors.push([error, asyncError]);
        } else {
          test.errors.push([error, asyncError]);
        }
        break;
      }
      case 'test_skip':
        event.test.status = 'skip';
        state.currentlyRunningTest = null;
        break;
      case 'test_todo':
        event.test.status = 'todo';
        state.currentlyRunningTest = null;
        break;
      case 'test_done':
        event.test.duration = typeof event.test.startedAt === 'number' ? Date.now() - event.test.startedAt : null;
        event.test.status = 'done';
        state.currentlyRunningTest = null;
        break;
      case 'test_start':
        state.currentlyRunningTest = event.test;
        event.test.startedAt = Date.now();
        event.test.invocations += 1;
        break;
      case 'test_fn_start':
        event.test.seenDone = false;
        break;
      case 'test_fn_failure':
        event.test.errors.push([event.error, event.test.asyncError]);
        break;
      case 'test_retry':
        if (globalThis[LOG_ERRORS_BEFORE_RETRY]) event.test.retryReasons.push(...event.test.errors);
        event.test.errors = [];
        break;
      case 'run_start':
        state.hasStarted = true;
        if (globalThis[TEST_TIMEOUT_SYMBOL]) state.testTimeout = globalThis[TEST_TIMEOUT_SYMBOL];
        break;
      case 'setup':
        state.originalGlobalErrorHandlers = injectGlobalErrorHandlers(parentProcess);
        if (event.testNamePattern) state.testNamePattern = new RegExp(event.testNamePattern, 'i');
        break;
      case 'teardown':
        restoreGlobalErrorHandlers(parentProcess, state.originalGlobalErrorHandlers);
        break;
      case 'error': {
        // An error thrown or rejected outside the test's own promise chain:
        // charge it to whatever was running when it was raised.
        const context = getTestExecutionContext();
        if (event.promise) {
          let target = state.unhandledRejectionErrorByPromise;
          if (context?.test) {
            target = context.test.unhandledRejectionErrorByPromise;
          } else if (context?.hook) {
            let hookTarget = state.unhandledRejectionErrorByPromiseByHook.get(context.hook);
            if (!hookTarget) {
              hookTarget = new Map();
              state.unhandledRejectionErrorByPromiseByHook.set(context.hook, hookTarget);
            }
            target = hookTarget;
          }
          target.set(event.promise, event.error);
          state.unhandledRejectionErrorByPromiseTarget.set(event.promise, target);
        } else if (context?.test) {
          context.test.errors.push(event.error);
        } else {
          state.unhandledErrors.push(event.error);
        }
        break;
      }
      case 'error_handled': {
        const target = state.unhandledRejectionErrorByPromiseTarget.get(event.promise);
        target?.delete(event.promise);
        state.unhandledRejectionErrorByPromiseTarget.delete(event.promise);
        break;
      }
    }
  }

  // jestAdapterInit's handler: expect's per-test state.
  function adapterEventHandler(event) {
    if (!expect) return;
    switch (event.name) {
      case 'test_start':
        expect.setState({
          currentTestName: getTestNamesPath(event.test).slice(1).join(testNameSeparator),
          testFailing: event.test.failing,
        });
        break;
      case 'test_done': {
        event.test.numPassingAsserts = expect.getState().numPassingAsserts;
        const {suppressedErrors} = expect.getState();
        expect.setState({suppressedErrors: []});
        if (suppressedErrors.length > 0) event.test.errors.push(...suppressedErrors);
        const {isExpectingAssertions} = expect.getState();
        const failures = expect.extractExpectedAssertionsErrors();
        // expect.hasAssertions() only speaks when nothing else failed.
        if (!(isExpectingAssertions && event.test.errors.length > 0)) {
          event.test.errors.push(...failures.map(failure => failure.error));
        }
        break;
      }
    }
  }

  // --- unhandled rejections --------------------------------------------------

  function takeUnhandledRejectionErrors(errorsByPromise) {
    const errors = [...errorsByPromise.values()];
    for (const promise of errorsByPromise.keys()) state.unhandledRejectionErrorByPromiseTarget.delete(promise);
    errorsByPromise.clear();
    return errors;
  }

  function getTestsUnder(describeBlock) {
    const tests = [];
    for (const child of describeBlock.children) {
      if (child.type === 'describeBlock') tests.push(...getTestsUnder(child));
      else tests.push(child);
    }
    return tests;
  }

  function getHooksUnder(describeBlock) {
    const hooks = [...describeBlock.hooks];
    for (const child of describeBlock.children) {
      if (child.type === 'describeBlock') hooks.push(...getHooksUnder(child));
    }
    return hooks;
  }

  function unhandledRejectionHandler(event, state) {
    if (event.name === 'hook_success' || event.name === 'hook_failure') {
      const {test, describeBlock, hook} = event;
      const {asyncError, type} = hook;
      const errorsByPromise = state.unhandledRejectionErrorByPromiseByHook.get(hook) ?? new Map();
      const errors = takeUnhandledRejectionErrors(errorsByPromise);
      state.unhandledRejectionErrorByPromiseByHook.delete(hook);
      if (type === 'beforeAll') {
        for (const error of errors) addErrorToEachTestUnderDescribe(describeBlock, error, asyncError);
      } else if (type === 'afterAll') {
        for (const error of errors) state.unhandledErrors.push([error, asyncError]);
      } else {
        for (const error of takeUnhandledRejectionErrors(test.unhandledRejectionErrorByPromise)) {
          test.errors.push([error, asyncError]);
        }
      }
    } else if (event.name === 'test_fn_success' || event.name === 'test_fn_failure') {
      const {test} = event;
      for (const error of takeUnhandledRejectionErrors(test.unhandledRejectionErrorByPromise)) {
        test.errors.push([error, test.asyncError]);
      }
    } else if (event.name === 'run_describe_finish') {
      for (const hook of getHooksUnder(event.describeBlock)) {
        const errorsByPromise = state.unhandledRejectionErrorByPromiseByHook.get(hook);
        if (!errorsByPromise) continue;
        const errors = takeUnhandledRejectionErrors(errorsByPromise);
        state.unhandledRejectionErrorByPromiseByHook.delete(hook);
        for (const error of errors) {
          if (hook.type === 'beforeAll') addErrorToEachTestUnderDescribe(hook.parent, error, hook.asyncError);
          else if (hook.type === 'afterAll') state.unhandledErrors.push([error, hook.asyncError]);
        }
      }
      for (const test of getTestsUnder(event.describeBlock)) {
        for (const error of takeUnhandledRejectionErrors(test.unhandledRejectionErrorByPromise)) {
          test.errors.push([error, test.asyncError]);
        }
      }
    } else if (event.name === 'run_finish' || event.name === 'teardown') {
      state.unhandledErrors.push(...takeUnhandledRejectionErrors(state.unhandledRejectionErrorByPromise));
    }
  }

  // --- global error handlers -------------------------------------------------

  const uncaughtExceptionListener = error => dispatchSync({error, name: 'error'});
  const unhandledRejectionListener = (error, promise) => dispatchSync({error, name: 'error', promise});
  const rejectionHandledListener = promise => dispatchSync({name: 'error_handled', promise});

  function injectGlobalErrorHandlers(parent) {
    const uncaughtException = [...parent.rawListeners('uncaughtException')];
    const unhandledRejection = [...parent.rawListeners('unhandledRejection')];
    const rejectionHandled = [...parent.rawListeners('rejectionHandled')];
    parent.removeAllListeners('uncaughtException');
    parent.removeAllListeners('unhandledRejection');
    parent.removeAllListeners('rejectionHandled');
    parent.on('uncaughtException', uncaughtExceptionListener);
    parent.on('unhandledRejection', unhandledRejectionListener);
    parent.on('rejectionHandled', rejectionHandledListener);
    return {rejectionHandled, uncaughtException, unhandledRejection};
  }

  function restoreGlobalErrorHandlers(parent, original) {
    parent.removeListener('uncaughtException', uncaughtExceptionListener);
    parent.removeListener('unhandledRejection', unhandledRejectionListener);
    parent.removeListener('rejectionHandled', rejectionHandledListener);
    if (!original) return;
    for (const listener of original.uncaughtException) parent.on('uncaughtException', listener);
    for (const listener of original.unhandledRejection) parent.on('unhandledRejection', listener);
    for (const listener of original.rejectionHandled) parent.on('rejectionHandled', listener);
  }

  // --- tree queries ----------------------------------------------------------

  function getTestNamesPath(test) {
    const titles = [];
    let parent = test;
    do {
      titles.unshift(parent.name);
    } while ((parent = parent.parent));
    return titles;
  }

  const testIds = new WeakMap();
  function getTestID(test) {
    const cached = testIds.get(test);
    if (cached !== undefined) return cached;
    const testNamesPath = getTestNamesPath(test);
    testNamesPath.shift();
    const testId = testNamesPath.join(' ');
    testIds.set(test, testId);
    return testId;
  }

  // Whether at least one test under the block will run; a block without one
  // runs none of its beforeAll or afterAll hooks.
  function hasEnabledTest(describeBlock) {
    const {hasFocusedTests, testNamePattern} = state;
    return describeBlock.children.some(child =>
      child.type === 'describeBlock'
        ? hasEnabledTest(child)
        : !(
            child.mode === 'skip' ||
            (hasFocusedTests && child.mode !== 'only') ||
            (testNamePattern && !testNamePattern.test(getTestID(child)))
          ),
    );
  }

  function getAllHooksForDescribe(describe) {
    const result = {afterAll: [], beforeAll: []};
    if (hasEnabledTest(describe)) {
      for (const hook of describe.hooks) {
        if (hook.type === 'beforeAll') result.beforeAll.push(hook);
        else if (hook.type === 'afterAll') result.afterAll.push(hook);
      }
    }
    return result;
  }

  // beforeEach from the outermost block in; afterEach from the innermost out.
  // Within one block, both run in the order they were declared.
  function getEachHooksForTest(test) {
    const result = {afterEach: [], beforeEach: []};
    if (test.concurrent) return result;
    let block = test.parent;
    do {
      const beforeEachForCurrentBlock = [];
      for (const hook of block.hooks) {
        if (hook.type === 'beforeEach') beforeEachForCurrentBlock.push(hook);
        else if (hook.type === 'afterEach') result.afterEach.push(hook);
      }
      result.beforeEach.unshift(...beforeEachForCurrentBlock);
    } while ((block = block.parent));
    return result;
  }

  const describeBlockHasTests = describe =>
    describe.children.some(child => child.type === 'test' || describeBlockHasTests(child));

  function addErrorToEachTestUnderDescribe(describeBlock, error, asyncError) {
    for (const child of describeBlock.children) {
      if (child.type === 'describeBlock') addErrorToEachTestUnderDescribe(child, error, asyncError);
      else child.errors.push(asyncError ? [error, asyncError] : error);
    }
  }

  // --- calling a test or hook ------------------------------------------------

  const makeTimeoutMessage = (timeout, isHook, takesDoneCallback) =>
    `Exceeded timeout of ${formatTime(timeout)} for a ${isHook ? 'hook' : 'test'}${
      takesDoneCallback ? ' while waiting for `done()` to be called' : ''
    }.\nAdd a timeout value to this test to increase the timeout, if this is a long-running test. See https://jestjs.io/docs/api#testname-fn-timeout.`;

  const checkIsError = error => !!(error && error.message && error.stack);

  function callAsyncCircusFn(testOrHook, testContext, {isHook, timeout}) {
    let timeoutID;
    let completed = false;
    const {fn, asyncError} = testOrHook;
    const doneCallback = fn.length > 0;

    return new Promise((resolve, reject) => {
      timeoutID = setTimeout(() => reject(makeTimeoutMessage(timeout, isHook, doneCallback)), timeout);

      if (doneCallback) {
        let returnedValue = undefined;
        const done = reason => {
          // Keep a stack from before the promise tick.
          const errorAtDone = new ErrorWithStack(undefined, done);
          if (!completed && testOrHook.seenDone) {
            errorAtDone.message = 'Expected done to be called once, but it was called multiple times.';
            if (reason) {
              errorAtDone.message += ` Reason: ${prettyFormat(reason, {maxDepth: 3})}`;
            }
            reject(errorAtDone);
            throw errorAtDone;
          } else if (!completed) {
            testOrHook.seenDone = true;
          }
          // One tick, in case done was called synchronously.
          Promise.resolve().then(() => {
            if (returnedValue !== undefined) {
              asyncError.message =
                "Test functions cannot both take a 'done' callback and return something. Either use a 'done' callback, or return a promise.\n" +
                `Returned value: ${prettyFormat(returnedValue, {maxDepth: 3})}`;
              return reject(asyncError);
            }
            let errorAsErrorObject;
            if (checkIsError(reason)) {
              errorAsErrorObject = reason;
            } else {
              errorAsErrorObject = errorAtDone;
              errorAtDone.message = `Failed: ${prettyFormat(reason, {maxDepth: 3})}`;
            }
            if (completed && reason) {
              errorAsErrorObject.message = `Caught error after test environment was torn down\n\n${errorAsErrorObject.message}`;
              throw errorAsErrorObject;
            }
            return reason ? reject(errorAsErrorObject) : resolve();
          });
        };
        returnedValue = fn.call(testContext, done);
        return;
      }

      let returnedValue;
      if (isGeneratorFunction(fn)) {
        returnedValue = co(fn.call(testContext));
      } else {
        try {
          returnedValue = fn.call(testContext);
        } catch (error) {
          reject(error);
          return;
        }
      }
      if (isPromise(returnedValue)) {
        returnedValue.then(() => resolve(), reject);
        return;
      }
      if (!isHook && returnedValue !== undefined) {
        reject(
          new Error(
            'test functions can only return Promise or undefined.\n' +
              `Returned value: ${prettyFormat(returnedValue, {maxDepth: 3})}`,
          ),
        );
        return;
      }
      resolve();
    }).finally(() => {
      completed = true;
      timeoutID.unref?.();
      clearTimeout(timeoutID);
    });
  }

  // --- running ---------------------------------------------------------------

  async function run() {
    await dispatch({name: 'run_start'});
    await runTestsForDescribeBlock(state.rootDescribeBlock);
    await dispatch({name: 'run_finish'});
    return makeRunResult();
  }

  function* regroupConcurrentChildren(children) {
    const concurrentTests = children.filter(child => child.type === 'test' && child.concurrent);
    if (concurrentTests.length === 0) {
      yield* children;
      return;
    }
    let collected = false;
    for (const child of children) {
      if (child.type === 'test' && child.concurrent) {
        if (!collected) {
          collected = true;
          yield {tests: concurrentTests, type: 'test-concurrent'};
        }
      } else {
        yield child;
      }
    }
  }

  async function runTestsForDescribeBlock(describeBlock) {
    await dispatch({describeBlock, name: 'run_describe_start'});
    const {beforeAll, afterAll} = getAllHooksForDescribe(describeBlock);
    const isSkipped = describeBlock.mode === 'skip';

    if (!isSkipped) {
      for (const hook of beforeAll) await callCircusHook({describeBlock, hook});
    }

    // Tests that fail and are retried run after the block's other tests.
    const retryTimes = Number.parseInt(globalThis[RETRY_TIMES], 10) || 0;
    const waitBeforeRetry = Number.parseInt(globalThis[WAIT_BEFORE_RETRY], 10) || 0;
    const retryImmediately = globalThis[RETRY_IMMEDIATELY] || false;
    const deferredRetryTests = [];

    const rerunTest = async test => {
      let numRetriesAvailable = retryTimes;
      while (numRetriesAvailable > 0 && test.errors.length > 0) {
        await dispatch({name: 'test_retry', test});
        if (waitBeforeRetry > 0) await new Promise(resolve => setTimeout(resolve, waitBeforeRetry));
        await runTest(test, isSkipped);
        numRetriesAvailable--;
      }
    };

    const runTestWithContext = async child => {
      const hasErrorsBeforeTestRun = child.errors.length > 0;
      await runTest(child, isSkipped);
      if (child.errors.length === 0 || hasErrorsBeforeTestRun || retryTimes <= 0) return;
      if (!retryImmediately) {
        deferredRetryTests.push(child);
        return;
      }
      await rerunTest(child);
    };

    for (const child of regroupConcurrentChildren(describeBlock.children)) {
      if (child.type === 'describeBlock') {
        await runTestsForDescribeBlock(child);
      } else if (child.type === 'test') {
        await runTestWithContext(child);
      } else {
        await runConcurrently(child.tests, runTestWithContext, state.maxConcurrency);
      }
    }

    for (const test of deferredRetryTests) await rerunTest(test);

    if (!isSkipped) {
      for (const hook of afterAll) await callCircusHook({describeBlock, hook});
    }
    await dispatch({describeBlock, name: 'run_describe_finish'});
  }

  // p-limit: at most `limit` running at once, started in order.
  function runConcurrently(tests, runOne, limit) {
    return new Promise(resolve => {
      let next = 0;
      let running = 0;
      let finished = 0;
      if (tests.length === 0) return resolve();
      const startNext = () => {
        while (running < limit && next < tests.length) {
          const test = tests[next++];
          running++;
          Promise.resolve()
            .then(() => runOne(test))
            .finally(() => {
              running--;
              finished++;
              if (finished === tests.length) resolve();
              else startNext();
            });
        }
      };
      startNext();
    });
  }

  const runTest = (test, parentSkipped) =>
    runInTestExecutionContext({test}, () => runTestInContext(test, parentSkipped));

  async function runTestInContext(test, parentSkipped) {
    await dispatch({name: 'test_start', test});
    const testContext = Object.create(null);
    const {hasFocusedTests, testNamePattern} = state;
    const isSkipped =
      parentSkipped ||
      test.mode === 'skip' ||
      (hasFocusedTests && test.mode === undefined) ||
      (testNamePattern && !testNamePattern.test(getTestID(test)));

    if (isSkipped) {
      await dispatch({name: 'test_skip', test});
      return;
    }
    if (test.mode === 'todo') {
      await dispatch({name: 'test_todo', test});
      return;
    }
    await dispatch({name: 'test_started', test});

    const {afterEach, beforeEach} = getEachHooksForTest(test);
    for (const hook of beforeEach) {
      // Once a beforeEach has failed, none of the later ones run.
      if (test.errors.length > 0) break;
      await callCircusHook({hook, test, testContext});
    }
    await callCircusTest(test, testContext);
    for (const hook of afterEach) await callCircusHook({hook, test, testContext});
    // afterAll doesn't decide a test's status, so the test is done here.
    await dispatch({name: 'test_done', test});
  }

  const callCircusHook = ({hook, test, describeBlock, testContext = {}}) =>
    runInTestExecutionContext({describeBlock, hook, test}, async () => {
      await dispatch({hook, name: 'hook_start'});
      const timeout = hook.timeout || state.testTimeout;
      try {
        await callAsyncCircusFn(hook, testContext, {isHook: true, timeout});
        await dispatch({describeBlock, hook, name: 'hook_success', test});
      } catch (error) {
        await dispatch({describeBlock, error, hook, name: 'hook_failure', test});
      }
    });

  async function callCircusTest(test, testContext) {
    await dispatch({name: 'test_fn_start', test});
    const timeout = test.timeout || state.testTimeout;
    // A failed beforeEach means the test itself doesn't run.
    if (test.errors.length > 0) return;
    try {
      await callAsyncCircusFn(test, testContext, {isHook: false, timeout});
      if (test.failing) {
        test.asyncError.message =
          'Failing test passed even though it was supposed to fail. Remove `.failing` to remove error.';
        await dispatch({error: test.asyncError, name: 'test_fn_failure', test});
      } else {
        await dispatch({name: 'test_fn_success', test});
      }
    } catch (error) {
      // Vitest's context.skip(): the test reports as skipped (vitest.cjs).
      if (error && error[DYNAMIC_SKIP]) {
        test.dynamicSkip = true;
        await dispatch({name: 'test_fn_success', test});
      } else if (test.failing) await dispatch({name: 'test_fn_success', test});
      else await dispatch({error, name: 'test_fn_failure', test});
    }
  }

  // --- results ---------------------------------------------------------------

  function getError(errors) {
    let error;
    let asyncError;
    if (Array.isArray(errors)) {
      error = errors[0];
      asyncError = errors[1];
    } else {
      error = errors;
      asyncError = new Error();
    }
    if (error && (typeof error.stack === 'string' || error.message)) return error;
    if (asyncError) return new Error(`thrown: ${prettyFormat(error, {maxDepth: 3})}`, {cause: asyncError});
    return new Error(`thrown: ${prettyFormat(error, {maxDepth: 3})}`);
  }

  function makeRunResult() {
    const testResults = [];
    const stack = [[state.rootDescribeBlock, 0]];
    while (stack.length > 0) {
      const [currentBlock, childIndex] = stack.pop();
      for (let i = childIndex; i < currentBlock.children.length; i++) {
        const child = currentBlock.children[i];
        if (child.type === 'describeBlock') {
          stack.push([currentBlock, i + 1], [child, 0]);
          break;
        }
        const errorsDetailed = child.errors.map(getError);
        testResults.push({
          duration: child.duration,
          errors: errorsDetailed.map(error => flattenErrorStack(error)),
          errorsDetailed,
          failing: child.failing,
          invocations: child.invocations,
          location: null,
          numPassingAsserts: child.numPassingAsserts,
          retryReasons: child.retryReasons.map(getError).map(error => flattenErrorStack(error)),
          startedAt: child.startedAt,
          status: child.dynamicSkip && child.errors.length === 0 ? 'skip' : child.status,
          testPath: getTestNamesPath(child),
        });
      }
    }
    const unhandledErrorsDetailed = state.unhandledErrors.map(getError);
    return {
      testResults,
      unhandledErrors: unhandledErrorsDetailed.map(error => flattenErrorStack(error)),
      unhandledErrorsDetailed,
    };
  }

  // --- the globals -----------------------------------------------------------

  const _dispatchDescribe = (blockFn, blockName, describeFn, mode) => {
    const asyncError = new ErrorWithStack(undefined, describeFn);
    if (blockFn === undefined) {
      asyncError.message = 'Missing second argument. It must be a callback function.';
      throw asyncError;
    }
    if (typeof blockFn !== 'function') {
      asyncError.message = `Invalid second argument, ${blockFn}. It must be a callback function.`;
      throw asyncError;
    }
    try {
      blockName = convertDescriptorToString(blockName);
    } catch (error) {
      asyncError.message = error.message;
      throw asyncError;
    }
    dispatchSync({asyncError, blockName, mode, name: 'start_describe_definition'});
    const describeReturn = blockFn();
    if (isPromise(describeReturn)) {
      throw new ErrorWithStack(
        'Returning a Promise from "describe" is not supported. Tests must be defined synchronously.',
        describeFn,
      );
    } else if (describeReturn !== undefined) {
      throw new ErrorWithStack('A "describe" callback must not return a value.', describeFn);
    }
    dispatchSync({blockName, mode, name: 'finish_describe_definition'});
  };

  const describe = (() => {
    const describe = (blockName, blockFn) => _dispatchDescribe(blockFn, blockName, describe);
    const only = (blockName, blockFn) => _dispatchDescribe(blockFn, blockName, only, 'only');
    const skip = (blockName, blockFn) => _dispatchDescribe(blockFn, blockName, skip, 'skip');
    describe.each = each.bind(describe, false);
    only.each = each.bind(only, false);
    skip.each = each.bind(skip, false);
    describe.only = only;
    describe.skip = skip;
    return describe;
  })();

  const _addHook = (fn, hookType, hookFn, timeout) => {
    const asyncError = new ErrorWithStack(undefined, hookFn);
    if (typeof fn !== 'function') {
      asyncError.message = 'Invalid first argument. It must be a callback function.';
      throw asyncError;
    }
    dispatchSync({asyncError, fn, hookType, name: 'add_hook', timeout});
  };

  const beforeEach = (fn, timeout) => _addHook(fn, 'beforeEach', beforeEach, timeout);
  const beforeAll = (fn, timeout) => _addHook(fn, 'beforeAll', beforeAll, timeout);
  const afterEach = (fn, timeout) => _addHook(fn, 'afterEach', afterEach, timeout);
  const afterAll = (fn, timeout) => _addHook(fn, 'afterAll', afterAll, timeout);

  const test = (() => {
    const test = (testName, fn, timeout) => _addTest(testName, undefined, false, fn, test, timeout);
    const skip = (testName, fn, timeout) => _addTest(testName, 'skip', false, fn, skip, timeout);
    const only = (testName, fn, timeout) => _addTest(testName, 'only', false, fn, test.only, timeout);
    const concurrentTest = (testName, fn, timeout) => _addTest(testName, undefined, true, fn, concurrentTest, timeout);
    const concurrentOnly = (testName, fn, timeout) => _addTest(testName, 'only', true, fn, concurrentOnly, timeout);

    const bindFailing = (concurrent, mode) => {
      const failing = (testName, fn, timeout, eachError) =>
        _addTest(testName, mode, concurrent, fn, failing, timeout, true, eachError);
      failing.each = each.bind(failing, false, true);
      return failing;
    };

    test.todo = (testName, ...rest) => {
      if (rest.length > 0 || typeof testName !== 'string') {
        throw new ErrorWithStack('Todo must be called with only a description.', test.todo);
      }
      return _addTest(testName, 'todo', false, () => {}, test.todo);
    };

    const _addTest = (testName, mode, concurrent, fn, testFn, timeout, failing, asyncError = new ErrorWithStack(undefined, testFn)) => {
      try {
        testName = convertDescriptorToString(testName);
      } catch (error) {
        asyncError.message = error.message;
        throw asyncError;
      }
      if (fn === undefined) {
        asyncError.message =
          'Missing second argument. It must be a callback function. Perhaps you want to use `test.todo` for a test placeholder.';
        throw asyncError;
      }
      if (typeof fn !== 'function') {
        asyncError.message = `Invalid second argument, ${fn}. It must be a callback function.`;
        throw asyncError;
      }
      return dispatchSync({
        asyncError,
        concurrent,
        failing: failing === undefined ? false : failing,
        fn,
        mode,
        name: 'add_test',
        testName,
        timeout,
      });
    };

    test.each = each.bind(test);
    only.each = each.bind(only);
    skip.each = each.bind(skip);
    concurrentTest.each = each.bind(concurrentTest, false);
    concurrentOnly.each = each.bind(concurrentOnly, false);
    only.failing = bindFailing(false, 'only');
    skip.failing = bindFailing(false, 'skip');
    test.failing = bindFailing(false);
    test.only = only;
    test.skip = skip;
    test.concurrent = concurrentTest;
    concurrentTest.only = concurrentOnly;
    concurrentTest.skip = skip;
    concurrentTest.failing = bindFailing(true);
    concurrentOnly.failing = bindFailing(true, 'only');
    return test;
  })();

  const globals = {
    afterAll,
    afterEach,
    beforeAll,
    beforeEach,
    describe,
    fdescribe: describe.only,
    fit: test.only,
    it: test,
    test,
    xdescribe: describe.skip,
    xit: test.skip,
    xtest: test.skip,
  };

  return {dispatch, getTestID, globals, run, state};
}

// --- co ------------------------------------------------------------------------

// What Jest runs a generator function as a test through: each yielded value
// is awaited and sent back in.
const isGeneratorFunction = fn => Object.prototype.toString.call(fn) === '[object GeneratorFunction]';
const isGenerator = obj => obj && typeof obj.next === 'function' && typeof obj.throw === 'function';

function co(gen) {
  return new Promise((resolve, reject) => {
    function onFulfilled(res) {
      let ret;
      try {
        ret = gen.next(res);
      } catch (error) {
        return reject(error);
      }
      next(ret);
    }
    function onRejected(err) {
      let ret;
      try {
        ret = gen.throw(err);
      } catch (error) {
        return reject(error);
      }
      next(ret);
    }
    function next(ret) {
      if (ret.done) return resolve(ret.value);
      const value = toPromise(ret.value);
      if (value && isPromise(value)) return value.then(onFulfilled, onRejected);
      return onRejected(
        new TypeError(
          'You may only yield a function, promise, generator, array, or object, ' +
            `but the following object was passed: "${String(ret.value)}"`,
        ),
      );
    }
    onFulfilled();
  });
}

function toPromise(obj) {
  if (!obj) return obj;
  if (isPromise(obj)) return obj;
  if (isGeneratorFunction(obj) || isGenerator(obj)) return co(isGenerator(obj) ? obj : obj.call(this));
  if (typeof obj === 'function') {
    return new Promise((resolve, reject) => {
      obj((err, ...res) => (err ? reject(err) : resolve(res.length > 1 ? res : res[0])));
    });
  }
  if (Array.isArray(obj)) return Promise.all(obj.map(toPromise));
  if (Object === obj.constructor) {
    const results = new obj.constructor();
    const promises = [];
    for (const key of Object.keys(obj)) {
      const promise = toPromise(obj[key]);
      if (promise && isPromise(promise)) {
        results[key] = undefined;
        promises.push(promise.then(res => (results[key] = res)));
      } else {
        results[key] = obj[key];
      }
    }
    return Promise.all(promises).then(() => results);
  }
  return obj;
}

module.exports = {ROOT_DESCRIBE_BLOCK_NAME, createCircus};
