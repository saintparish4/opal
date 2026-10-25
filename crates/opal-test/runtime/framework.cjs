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
const snapshot = require('./jest-snapshot.cjs');
const snapfile = require('./snapfile-dialect.cjs');
const {createVitest} = require('./vitest.cjs');

const ROOT = 'ROOT_DESCRIBE_BLOCK';
const TEST_TIMEOUT_SYMBOL = Symbol.for('TEST_TIMEOUT_SYMBOL');
const RETRY_TIMES = Symbol.for('RETRY_TIMES');
const RETRY_IMMEDIATELY = Symbol.for('RETRY_IMMEDIATELY');
const WAIT_BEFORE_RETRY = Symbol.for('WAIT_BEFORE_RETRY');
const LOG_ERRORS_BEFORE_RETRY = Symbol.for('LOG_ERRORS_BEFORE_RETRY');

async function initialize({config, globalConfig, testPath, parentProcess, loader}) {
  const dialect = config.dialect || 'jest';
  const separator = dialect === 'vitest' ? ' > ' : ' ';
  snapfile.setDialect(dialect);
  const expect = createExpect({testPath});
  const circus = createCircus({expect, parentProcess, testNameSeparator: separator});

  // jestAdapterInit's snapshot setup: serializers from the config, tested
  // before the built-in ones (so added last first), then the file's state.
  for (const serializer of [...config.snapshotSerializers].reverse()) {
    snapshot.addSerializer(loader.requireModule(testPath, serializer));
  }
  const resolver = await snapshot.buildSnapshotResolver(config, request => loader.requireModule(testPath, request));
  const snapshotState = new snapshot.SnapshotState(resolver.resolveSnapshotPath(testPath), {
    expand: globalConfig.expand,
    prettierPath: null,
    rootDir: config.rootDir,
    snapshotFormat: config.snapshotFormat,
    updateSnapshot: globalConfig.updateSnapshot,
  });
  expect.setState({snapshotState, testPath});
  const testTimeout = globalConfig.testTimeout || config.testTimeout;
  if (testTimeout) circus.state.testTimeout = testTimeout;
  const moduleMocker = new ModuleMocker(globalThis);
  loader.moduleMocker = moduleMocker;

  await circus.dispatch({name: 'setup', testNamePattern: globalConfig.testNamePattern});

  // jest-runtime's JestGlobals.buildJestObject: one `jest` per module,
  // bridged to the loader for everything about modules.
  const jestObjects = new Map();
  function createJestObject(from) {
    const cached = jestObjects.get(from);
    if (cached) return cached;
    const chain = action => (...args) => {
      action(...args);
      return jest;
    };
    const mock = (moduleName, mockFactory, options) => {
      if (mockFactory !== undefined) loader.setMock(from, moduleName, mockFactory, options);
      else loader.markExplicitMock(from, moduleName);
      return jest;
    };
    const unmock = chain(moduleName => loader.unmock(from, moduleName));
    const fakeTimers = unsupported('fake timers');
    const jest = {
      advanceTimersByTime: fakeTimers,
      advanceTimersByTimeAsync: fakeTimers,
      advanceTimersToNextFrame: fakeTimers,
      advanceTimersToNextTimer: fakeTimers,
      advanceTimersToNextTimerAsync: fakeTimers,
      autoMockOff: chain(() => loader.disableAutomock()),
      autoMockOn: chain(() => loader.enableAutomock()),
      clearAllMocks: chain(() => moduleMocker.clearAllMocks()),
      clearAllTimers: fakeTimers,
      createMockFromModule: moduleName => loader.generateMock(from, moduleName),
      deepUnmock: chain(moduleName => loader.deepUnmock(from, moduleName)),
      disableAutomock: chain(() => loader.disableAutomock()),
      doMock: mock,
      dontMock: unmock,
      enableAutomock: chain(() => loader.enableAutomock()),
      fn: moduleMocker.fn.bind(moduleMocker),
      genMockFromModule: moduleName => loader.generateMock(from, moduleName),
      getRealSystemTime: fakeTimers,
      getSeed: () => 0,
      getTimerCount: fakeTimers,
      isEnvironmentTornDown: () => false,
      isMockFunction: moduleMocker.isMockFunction,
      isolateModules: chain(fn => loader.isolateModules(fn)),
      isolateModulesAsync: fn => loader.isolateModulesAsync(fn),
      mock,
      mocked: moduleMocker.mocked.bind(moduleMocker),
      now: fakeTimers,
      onGenerateMock: chain(callback => loader.addOnGenerateMock(callback)),
      replaceProperty: moduleMocker.replaceProperty.bind(moduleMocker),
      requireActual: moduleName => loader.requireActual(from, moduleName),
      requireMock: moduleName => loader.requireMock(from, moduleName),
      resetAllMocks: chain(() => moduleMocker.resetAllMocks()),
      resetModules: chain(() => loader.resetModules()),
      restoreAllMocks: chain(() => moduleMocker.restoreAllMocks()),
      retryTimes(numRetries, options) {
        globalThis[RETRY_TIMES] = numRetries;
        globalThis[LOG_ERRORS_BEFORE_RETRY] = options?.logErrorsBeforeRetry;
        globalThis[WAIT_BEFORE_RETRY] = options?.waitBeforeRetry;
        globalThis[RETRY_IMMEDIATELY] = options?.retryImmediately;
        return jest;
      },
      runAllImmediates: fakeTimers,
      runAllTicks: fakeTimers,
      runAllTimers: fakeTimers,
      runAllTimersAsync: fakeTimers,
      runOnlyPendingTimers: fakeTimers,
      runOnlyPendingTimersAsync: fakeTimers,
      setMock: (moduleName, moduleExports) => mock(moduleName, () => moduleExports),
      setSystemTime: fakeTimers,
      setTimeout(timeout) {
        globalThis[TEST_TIMEOUT_SYMBOL] = timeout;
        return jest;
      },
      spyOn: moduleMocker.spyOn.bind(moduleMocker),
      unmock,
      unstable_mockModule: unsupported('unstable_mockModule'),
      unstable_unmockModule: unsupported('unstable_unmockModule'),
      useFakeTimers: fakeTimers,
      useRealTimers: () => jest,
    };
    jestObjects.set(from, jest);
    return jest;
  }

  // jestAdapter.ts registers this before anything else, so it is the first
  // beforeEach of every test.
  circus.globals.beforeEach(() => {
    if (config.resetModules) loader.resetModules();
    if (config.clearMocks) moduleMocker.clearAllMocks();
    if (config.resetMocks) moduleMocker.resetAllMocks();
    if (config.restoreMocks) moduleMocker.restoreAllMocks();
  });

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
    moduleMocker.restoreAllMocks();
    await circus.dispatch({name: 'teardown'});
    return {
      failureMessage,
      numFailingTests,
      numPassingTests,
      numPendingTests,
      numTodoTests,
      skipped: false,
      snapshot: snapshotData(assertionResults, snapshotState, separator),
      testExecError,
      testResults: assertionResults,
    };
  }

  // jest-runtime's teardown restores every spy: one on a built-in module,
  // which the next file in this worker shares, must not outlive the file.
  async function teardown() {
    moduleMocker.restoreAllMocks();
    await circus.dispatch({name: 'teardown'});
  }

  const vitestFor = createVitest({circus, createJestObject, expect, moduleMocker});
  // What `globals` injects: Jest's globals, or with Vitest's `globals: true`
  // what Vitest puts on the global object.
  const globals =
    dialect === 'vitest'
      ? (({assert, chai, bench, vitest, ...rest}) => rest)(vitestFor(testPath))
      : {...circus.globals, expect};

  return {
    createJestObject,
    globals,
    jestGlobals: {...circus.globals, expect},
    run,
    teardown,
    vitestFor,
  };
}

function unsupported(name) {
  return () => {
    throw new Error(`opal test: ${name} ${name.endsWith('s') ? 'are' : 'is'} not supported yet.`);
  };
}

// jestAdapter.ts's _addSnapshotData: a skipped or failed test's snapshots
// aren't obsolete, nor are those of a `failing` test that passed (it threw);
// whatever else went unchecked is obsolete, and removed with -u.
function snapshotData(assertionResults, snapshotState, separator) {
  for (const {ancestorTitles, title, status, failing} of assertionResults) {
    if (status === 'pending' || status === 'failed' || (failing && status === 'passed')) {
      snapshotState.markSnapshotsAsCheckedForTest([...ancestorTitles, title].join(separator));
    }
  }
  const uncheckedCount = snapshotState.getUncheckedCount();
  const uncheckedKeys = snapshotState.getUncheckedKeys();
  if (uncheckedCount) snapshotState.removeUncheckedKeys();
  const status = snapshotState.save();
  return {
    added: snapshotState.added,
    fileDeleted: status.deleted,
    matched: snapshotState.matched,
    unchecked: status.deleted ? 0 : uncheckedCount,
    uncheckedKeys: [...uncheckedKeys],
    unmatched: snapshotState.unmatched,
    updated: snapshotState.updated,
  };
}

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
