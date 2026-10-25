// One test file, start to finish, in the order jest-runner's runTest.ts and
// jestAdapter.ts use: a fresh environment, the framework loaded into it,
// setupFiles, the globals, setupFilesAfterEnv, the test file, the run.
// Anything thrown before the run starts fails the file as a whole, as
// "Test suite failed to run".
'use strict';

const fs = require('fs');
const path = require('path');
const util = require('util');
const vm = require('vm');
const {createEnvironment} = require('./environment.cjs');
const {Loader} = require('./loader.cjs');

// Everything in this folder but the host side below runs inside each test
// file's context, loaded on first require. Compiled once per worker, run in
// each file's context.
const HOST_FILES = new Set(['worker.cjs', 'config.cjs', 'environment.cjs', 'loader.cjs', 'runner.cjs']);
const frameworkScripts = new Map();

function frameworkScript(name) {
  let script = frameworkScripts.get(name);
  if (!script) {
    const filename = path.join(__dirname, name);
    const source = fs.readFileSync(filename, 'utf8');
    script = new vm.Script(`(function (exports, require, module, __filename, __dirname) {${source}\n})`, {filename});
    frameworkScripts.set(name, script);
  }
  return script;
}

const isFrameworkFile = name =>
  name.endsWith('.cjs') && !HOST_FILES.has(name) && fs.existsSync(path.join(__dirname, name));

// A `require` for the runtime's in-context modules, one registry per context.
function frameworkRequire(context) {
  const loaded = new Map();
  const requireInternal = request => {
    const name = request.startsWith('./') ? request.slice(2) : request;
    if (!isFrameworkFile(name)) return require(request);
    const cached = loaded.get(name);
    if (cached) return cached.exports;
    const module = {exports: {}};
    loaded.set(name, module);
    frameworkScript(name)
      .runInContext(context)
      .call(module.exports, module.exports, requireInternal, module, path.join(__dirname, name), __dirname);
    return module.exports;
  };
  return requireInternal;
}

const loadFramework = context => frameworkRequire(context)('framework.cjs');

// Jest's BufferedConsole, in outline: what a test logs is kept with the file
// and printed under it, not interleaved with other workers' output.
function createBufferedConsole(entries) {
  const counters = new Map();
  const timers = new Map();
  let groupDepth = 0;
  const write = (type, message) => {
    entries.push({message: '  '.repeat(groupDepth) + message, origin: '', type});
  };
  const log = type => (...args) => write(type, util.format(...args));
  return {
    assert(value, ...args) {
      if (!value) write('assert', `Assertion failed${args.length ? `: ${util.format(...args)}` : ''}`);
    },
    count(label = 'default') {
      counters.set(label, (counters.get(label) || 0) + 1);
      write('count', `${label}: ${counters.get(label)}`);
    },
    countReset(label = 'default') {
      counters.set(label, 0);
    },
    debug: log('debug'),
    dir: (value, options) => write('dir', util.inspect(value, options)),
    dirxml: log('dirxml'),
    error: log('error'),
    group(...args) {
      if (args.length) write('group', util.format(...args));
      groupDepth++;
    },
    groupCollapsed(...args) {
      if (args.length) write('groupCollapsed', util.format(...args));
      groupDepth++;
    },
    groupEnd() {
      if (groupDepth > 0) groupDepth--;
    },
    info: log('info'),
    log: log('log'),
    table: (data, properties) => write('table', util.inspect(data, {depth: 2, ...(properties ? {} : {})})),
    time(label = 'default') {
      timers.set(label, process.hrtime.bigint());
    },
    timeEnd(label = 'default') {
      const start = timers.get(label);
      if (start !== undefined) {
        timers.delete(label);
        write('time', `${label}: ${Number(process.hrtime.bigint() - start) / 1e6} ms`);
      }
    },
    timeLog(label = 'default', ...args) {
      const start = timers.get(label);
      if (start !== undefined) {
        write('time', `${label}: ${Number(process.hrtime.bigint() - start) / 1e6} ms ${util.format(...args)}`.trimEnd());
      }
    },
    trace: (...args) => write('trace', `Trace: ${util.format(...args)}\n${new Error().stack.split('\n').slice(2).join('\n')}`),
    warn: log('warn'),
  };
}

function failure(testPath, error) {
  const stack = error && error.stack ? String(error.stack) : String(error);
  const message = error && error.message !== undefined ? String(error.message) : String(error);
  return {
    failureMessage: `  ● Test suite failed to run\n\n${stack
      .split('\n')
      .map(line => (line ? `    ${line}` : line))
      .join('\n')}\n`,
    numFailingTests: 0,
    numPassingTests: 0,
    numPendingTests: 0,
    numTodoTests: 0,
    skipped: false,
    snapshot: {added: 0, fileDeleted: false, matched: 0, unchecked: 0, uncheckedKeys: [], unmatched: 0, updated: 0},
    testExecError: {message, stack},
    testResults: [],
  };
}

async function runTestFile(testPath, {config, globalConfig}) {
  const start = Date.now();
  const consoleEntries = [];
  let result;
  let framework = null;
  try {
    const {context, global} = createEnvironment(config);
    global.console = createBufferedConsole(consoleEntries);
    const loader = new Loader({config, context, global, testPath});
    framework = await loadFramework(context).initialize({
      config,
      globalConfig,
      loader,
      parentProcess: process,
      testPath,
    });
    loader.createJestObject = from => framework.createJestObject(from);
    for (const file of config.setupFiles) loader.requireModule(file, file);
    if (config.injectGlobals) Object.assign(global, framework.globals);
    for (const file of config.setupFilesAfterEnv) {
      const exported = loader.requireModule(file, file);
      if (typeof exported === 'function') await exported();
    }
    loader.requireModule(testPath, testPath);
  } catch (error) {
    if (framework) await framework.teardown();
    result = failure(testPath, error);
  }
  if (!result) result = await framework.run();
  const end = Date.now();
  result.testFilePath = testPath;
  result.console = consoleEntries.length > 0 ? consoleEntries : undefined;
  result.perfStats = {end, runtime: end - start, slow: false, start};
  return result;
}

module.exports = {frameworkRequire, runTestFile};
