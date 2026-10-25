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
const sourcemaps = require('./sourcemaps.cjs');

// Everything in this folder but the host side below runs inside each test
// file's context, loaded on first require. Compiled once per worker, run in
// each file's context.
const HOST_FILES = new Set([
  'bridge-thread.cjs',
  'bridge.cjs',
  'config.cjs',
  'environment.cjs',
  'loader.cjs',
  'runner.cjs',
  'sourcemaps.cjs',
  'worker.cjs',
]);
const frameworkScripts = new Map();

// The framework's compiled code, kept on disk as V8's code cache, the way Node
// keeps its own built-in modules', so a new worker deserializes it instead of
// compiling it again. The pool says where (OPAL_TEST_CODE_CACHE, beside the
// unpacked runtime, so it shares its content hash). A worker writes what it
// compiled after its first test file, so the cache holds the functions running
// a file compiles too. It is keyed by Node's version and architecture, so a
// cache V8 would reject is never read.
const CODE_CACHE = process.env.OPAL_TEST_CODE_CACHE
  ? path.join(process.env.OPAL_TEST_CODE_CACHE, `${process.version}-${process.arch}`)
  : null;
const uncached = [];

function frameworkScript(name) {
  let script = frameworkScripts.get(name);
  if (!script) {
    const filename = path.join(__dirname, name);
    const source = fs.readFileSync(filename, 'utf8');
    const cachePath = CODE_CACHE && path.join(CODE_CACHE, `${name}.v8`);
    let cachedData;
    try {
      if (cachePath) cachedData = fs.readFileSync(cachePath);
    } catch {}
    script = new vm.Script(`(function (exports, require, module, __filename, __dirname) {${source}\n})`, {
      cachedData,
      filename,
    });
    if (cachePath && (!cachedData || script.cachedDataRejected)) uncached.push([script, cachePath]);
    frameworkScripts.set(name, script);
  }
  return script;
}

// Writes the code cache for what this worker compiled without one. Each file
// is written whole and renamed into place, so a worker never reads half of
// one another is writing.
function saveCodeCache() {
  for (const [script, cachePath] of uncached.splice(0)) {
    try {
      fs.mkdirSync(path.dirname(cachePath), {recursive: true});
      const staging = `${cachePath}.${process.pid}`;
      fs.writeFileSync(staging, script.createCachedData());
      fs.renameSync(staging, cachePath);
    } catch {}
  }
}

// Asked once per name per worker: every context requires the same files.
const frameworkFiles = new Map();
const isFrameworkFile = name => {
  let known = frameworkFiles.get(name);
  if (known === undefined) {
    known = name.endsWith('.cjs') && !HOST_FILES.has(name) && fs.existsSync(path.join(__dirname, name));
    frameworkFiles.set(name, known);
  }
  return known;
};

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

// The transform babel-jest would apply, done by opal (src/transform.rs):
// TypeScript, JSX, ES modules to CommonJS, jest.mock hoisting. A file outside
// transformIgnorePatterns is sent when anything in it could need that; a
// config that sets `transform: {}` turns it off, as it does in Jest.
const NEEDS_TRANSFORM = /\b(?:import|export)\b|\bjest\s*\.\s*(?:mock|unmock|enableAutomock|disableAutomock|deepUnmock)\b/;
const ALWAYS = new Set(['.ts', '.tsx', '.mts', '.cts', '.jsx']);

// An ES module in node_modules is transformed whatever transformIgnorePatterns
// says: CommonJS can't load it otherwise. Jest fails on it unless told to
// transform it; opal loads it, into the test's context like everything else.
const packageTypes = new Map();
function isEsModule(filename) {
  const extension = path.extname(filename);
  if (extension === '.mjs') return true;
  if (extension !== '.js') return false;
  for (let dir = path.dirname(filename); ; dir = path.dirname(dir)) {
    let type = packageTypes.get(dir);
    if (type === undefined) {
      try {
        type = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8')).type || 'commonjs';
      } catch {
        type = null;
      }
      packageTypes.set(dir, type);
    }
    if (type) return type === 'module';
    if (path.dirname(dir) === dir) return false;
  }
}

// OPAL_TEST_PROFILE=1 prints, per file, how much went to transforms.
const profile = process.env.OPAL_TEST_PROFILE ? {transformNanos: 0n, transforms: 0} : null;

// What this worker has had transformed, by file: a module required by many
// test files is sent across once per worker, not once per test file.
const transformed = new Map();

function createTransform(config, bridge) {
  if (!bridge || (config.transform && Object.keys(config.transform).length === 0)) return undefined;
  const ignore = (config.transformIgnorePatterns || ['/node_modules/', '\\.pnp\\.[^\\/]+$']).map(
    pattern => new RegExp(pattern),
  );
  return (filename, code) => {
    if (ignore.some(regex => regex.test(filename))) {
      if (!isEsModule(filename)) return code;
    } else if (!ALWAYS.has(path.extname(filename)) && !NEEDS_TRANSFORM.test(code)) {
      return code;
    }
    const known = transformed.get(filename);
    if (known && known.source === code) return known.code;
    const started = profile ? process.hrtime.bigint() : 0n;
    const reply = bridge.request({path: filename});
    if (profile) {
      profile.transforms += 1;
      profile.transformNanos += process.hrtime.bigint() - started;
    }
    if (reply.error) {
      const error = new SyntaxError(reply.error);
      error.stack = `SyntaxError: ${reply.error}`;
      throw error;
    }
    sourcemaps.register(filename, reply.mappings);
    transformed.set(filename, {code: reply.code, source: code});
    return reply.code;
  };
}

// A test file that imports from vitest is in Vitest's dialect whatever the
// project's config is: a Jest project couldn't run it otherwise.
const IMPORTS_VITEST = /\bfrom\s*['"]vitest['"]|\brequire\(\s*['"]vitest['"]\s*\)|\bimport\s*['"]vitest['"]/;

function dialectOf(testPath, config) {
  if (config.dialect === 'vitest') return config;
  try {
    if (IMPORTS_VITEST.test(fs.readFileSync(testPath, 'utf8'))) return {...config, dialect: 'vitest'};
  } catch {}
  return config;
}

async function runTestFile(testPath, context, bridge) {
  const globalConfig = context.globalConfig;
  const config = dialectOf(testPath, context.config);
  const start = Date.now();
  const consoleEntries = [];
  let result;
  let framework = null;
  try {
    const {context, global} = createEnvironment(config);
    // Vitest's variables in place of jest-worker's (cli-api.ts and its pool
    // workers set them).
    if (config.dialect === 'vitest') {
      const env = global.process.env;
      const id = env.JEST_WORKER_ID;
      Object.assign(env, {TEST: 'true', VITEST: 'true', VITEST_MODE: 'RUN', VITEST_POOL_ID: id, VITEST_WORKER_ID: id});
      delete env.JEST_WORKER_ID;
    }
    global.console = createBufferedConsole(consoleEntries);
    global.Error.prepareStackTrace = sourcemaps.prepareStackTrace;
    const transform = createTransform(config, bridge);
    const loader = new Loader({config, context, global, mocks: config.manualMocks, testPath, transform});
    framework = await loadFramework(context).initialize({
      config,
      globalConfig,
      loader,
      parentProcess: process,
      testPath,
    });
    loader.createJestObject = from => framework.createJestObject(from);
    loader.jestGlobals = from => ({...framework.jestGlobals, jest: framework.createJestObject(from)});
    loader.vitestFor = from => framework.vitestFor(from);
    for (const file of config.setupFiles) {
      const {evaluation} = loader.requireEntry(file);
      if (evaluation) await evaluation;
    }
    if (config.injectGlobals) Object.defineProperties(global, framework.globals);
    for (const file of config.setupFilesAfterEnv) {
      const {evaluation, exports} = loader.requireEntry(file);
      if (evaluation) await evaluation;
      if (typeof exports === 'function') await exports();
    }
    const {evaluation} = loader.requireEntry(testPath);
    if (evaluation) await evaluation;
  } catch (error) {
    if (framework) await framework.teardown();
    result = failure(testPath, error);
  }
  if (!result) result = await framework.run();
  const end = Date.now();
  if (profile) {
    process.stderr.write(
      `opal test profile: ${path.basename(testPath)} ${end - start} ms, ${profile.transforms} transforms in ${Number(profile.transformNanos / 1000000n)} ms\n`,
    );
    profile.transforms = 0;
    profile.transformNanos = 0n;
  }
  result.testFilePath = testPath;
  result.console = consoleEntries.length > 0 ? consoleEntries : undefined;
  result.perfStats = {end, runtime: end - start, slow: false, start};
  return result;
}

module.exports = {frameworkRequire, runTestFile, saveCodeCache};
