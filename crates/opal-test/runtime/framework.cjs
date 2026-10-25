// What a test file's context gets: the circus globals, `expect`, and a
// `jest` object per module; then the run, turned into Jest's per-file
// result (jestAdapterInit.ts's runAndTransformResultsToJestFormat, and
// jest-message-util's formatResultsErrors in outline).
//
// Evaluated inside the file's context; the runner reaches it through
// `initialize`.
'use strict';

const {createCircus} = require('./circus.cjs');
const {createExpect} = require('./expect.cjs');
const {ModuleMocker} = require('./mock.cjs');
const {format: prettyFormat} = require('./format.cjs');

const ROOT = 'ROOT_DESCRIBE_BLOCK';
const TEST_TIMEOUT_SYMBOL = Symbol.for('TEST_TIMEOUT_SYMBOL');
const RETRY_TIMES = Symbol.for('RETRY_TIMES');
const RETRY_IMMEDIATELY = Symbol.for('RETRY_IMMEDIATELY');
const WAIT_BEFORE_RETRY = Symbol.for('WAIT_BEFORE_RETRY');
const LOG_ERRORS_BEFORE_RETRY = Symbol.for('LOG_ERRORS_BEFORE_RETRY');

async function initialize({config, globalConfig, testPath, parentProcess, loader}) {
  const expect = createExpect({testPath});
  const circus = createCircus({expect, parentProcess});
  const testTimeout = globalConfig.testTimeout || config.testTimeout;
  if (testTimeout) circus.state.testTimeout = testTimeout;
  const moduleMocker = new ModuleMocker(globalThis);

  await circus.dispatch({name: 'setup', testNamePattern: globalConfig.testNamePattern});

  function createJestObject(from) {
    const jest = {
      advanceTimersByTime: unsupported('advanceTimersByTime'),
      clearAllMocks() {
        moduleMocker.clearAllMocks();
        return jest;
      },
      clearAllTimers: unsupported('clearAllTimers'),
      fn: implementation => moduleMocker.fn(implementation),
      getRealSystemTime: () => Date.now(),
      getSeed: () => 0,
      isEnvironmentTornDown: () => false,
      isMockFunction: fn => moduleMocker.isMockFunction(fn),
      isolateModules(fn) {
        loader.isolateModules(fn);
        return jest;
      },
      mocked: source => source,
      requireActual: request => loader.requireActual(from, request),
      resetAllMocks() {
        moduleMocker.resetAllMocks();
        return jest;
      },
      resetModules() {
        loader.resetModules();
        return jest;
      },
      restoreAllMocks() {
        moduleMocker.restoreAllMocks();
        return jest;
      },
      retryTimes(numRetries, options) {
        globalThis[RETRY_TIMES] = numRetries;
        globalThis[LOG_ERRORS_BEFORE_RETRY] = options?.logErrorsBeforeRetry;
        globalThis[WAIT_BEFORE_RETRY] = options?.waitBeforeRetry;
        globalThis[RETRY_IMMEDIATELY] = options?.retryImmediately;
        return jest;
      },
      runAllTimers: unsupported('runAllTimers'),
      setTimeout(timeout) {
        globalThis[TEST_TIMEOUT_SYMBOL] = timeout;
        return jest;
      },
      spyOn: (object, methodKey, accessType) => moduleMocker.spyOn(object, methodKey, accessType),
      useFakeTimers: unsupported('useFakeTimers'),
      useRealTimers() {
        return jest;
      },
    };
    return jest;
  }

  async function run() {
    const runResult = await circus.run();
    let numFailingTests = 0;
    let numPassingTests = 0;
    let numPendingTests = 0;
    let numTodoTests = 0;
    const assertionResults = runResult.testResults.map(testResult => {
      let status;
      if (testResult.status === 'skip') status = 'pending';
      else if (testResult.status === 'todo') status = 'todo';
      else if (testResult.errors.length > 0) status = 'failed';
      else status = 'passed';
      if (status === 'pending') numPendingTests += 1;
      else if (status === 'todo') numTodoTests += 1;
      else if (status === 'failed') numFailingTests += 1;
      else numPassingTests += 1;
      const ancestorTitles = testResult.testPath.filter(name => name !== ROOT);
      const fullName = ancestorTitles.join(' ');
      const title = testResult.testPath.at(-1);
      ancestorTitles.pop();
      return {
        ancestorTitles,
        duration: testResult.duration,
        failing: testResult.failing,
        failureDetails: testResult.errorsDetailed.map(detail),
        failureMessages: [...testResult.errors],
        fullName,
        invocations: testResult.invocations,
        location: testResult.location,
        numPassingAsserts: testResult.numPassingAsserts,
        retryReasons: [...testResult.retryReasons],
        startAt: testResult.startedAt,
        status,
        title,
      };
    });
    let failureMessage = formatResultsErrors(assertionResults);
    let testExecError;
    if (runResult.unhandledErrors.length > 0) {
      testExecError = {message: '', stack: runResult.unhandledErrors.join('\n')};
      failureMessage = `${failureMessage || ''}\n\n${runResult.unhandledErrors
        .map(error => `  ● Test suite failed to run\n\n${indent(error, '    ')}`)
        .join('\n')}`;
    }
    await circus.dispatch({name: 'teardown'});
    return {
      failureMessage,
      numFailingTests,
      numPassingTests,
      numPendingTests,
      numTodoTests,
      skipped: false,
      snapshot: expect.snapshotSummary ? expect.snapshotSummary() : emptySnapshot(),
      testExecError,
      testResults: assertionResults,
    };
  }

  async function teardown() {
    await circus.dispatch({name: 'teardown'});
  }

  return {
    createJestObject,
    globals: {...circus.globals, expect},
    run,
    teardown,
  };
}

function unsupported(name) {
  return () => {
    throw new Error(`opal test: jest.${name}() is not supported yet.`);
  };
}

const emptySnapshot = () => ({
  added: 0,
  fileDeleted: false,
  matched: 0,
  unchecked: 0,
  uncheckedKeys: [],
  unmatched: 0,
  updated: 0,
});

// An error's own enumerable fields, as JSON would carry them across.
function detail(error) {
  if (error === null || typeof error !== 'object') return error;
  const out = {};
  for (const key of Object.keys(error)) {
    try {
      out[key] = JSON.parse(JSON.stringify(error[key]));
    } catch {
      out[key] = prettyFormat(error[key], {maxDepth: 2});
    }
  }
  return out;
}

const indent = (text, prefix) =>
  String(text)
    .split('\n')
    .map(line => (line ? prefix + line : line))
    .join('\n');

function formatResultsErrors(assertionResults) {
  const failed = assertionResults.filter(result => result.status === 'failed');
  if (failed.length === 0) return null;
  return failed
    .map(result => {
      const title = [...result.ancestorTitles, result.title].join(' › ');
      const body = result.failureMessages.map(message => indent(message, '    ')).join('\n\n');
      return `  ● ${title}\n\n${body}\n`;
    })
    .join('\n');
}

module.exports = {initialize};
