// Finding and normalizing a project's test config, after jest-config's
// resolveConfigPath.ts, readConfigFileAndSetRootDir.ts and normalize.ts.
//
// From the starting folder upward, the first folder holding a
// jest.config.{ts,js,mjs,cjs,cts,mts,json} or a package.json wins; a
// package.json without a "jest" key means the defaults, with that folder as
// rootDir. A config can export an object or a function returning one, sync
// or async. What comes back has every default filled in, `<rootDir>`
// replaced and every path absolute, so nothing downstream guesses.
//
// A project is in Vitest's dialect when that folder has no Jest config but a
// vitest.config.* (or a vite.config.* with a `test` key), or lists vitest
// as a dependency. Its config is read into the same shape: `include` and
// `exclude` become testMatch globs, `globals` decides injectGlobals,
// `setupFiles` run where Jest's setupFilesAfterEnv do, `resolve.alias`
// becomes moduleNameMapper. `dialect: 'vitest'` tells the runtime to serve
// the `vitest` module and read imports the way Vite does.
'use strict';

const fs = require('fs');
const path = require('path');
const {pathToFileURL} = require('url');

const CONFIG_NAMES = ['ts', 'js', 'mjs', 'cjs', 'cts', 'mts', 'json'].map(ext => `jest.config.${ext}`);

const DEFAULTS = {
  clearMocks: false,
  injectGlobals: true,
  moduleFileExtensions: ['js', 'mjs', 'cjs', 'jsx', 'ts', 'mts', 'cts', 'tsx', 'json', 'node'],
  moduleNameMapper: {},
  resetMocks: false,
  resetModules: false,
  restoreMocks: false,
  setupFiles: [],
  setupFilesAfterEnv: [],
  snapshotFormat: {},
  snapshotSerializers: [],
  testEnvironment: 'node',
  testEnvironmentOptions: {},
  testPathIgnorePatterns: ['/node_modules/'],
  testTimeout: 5000,
};

const DEFAULT_TEST_MATCH = ['**/__tests__/**/*.?([mc])[jt]s?(x)', '**/?(*.)+(spec|test).?([mc])[jt]s?(x)'];

const VITEST_CONFIG_NAMES = ['vitest.config', 'vite.config'].flatMap(base =>
  ['ts', 'mts', 'cts', 'js', 'mjs', 'cjs'].map(ext => `${base}.${ext}`),
);

function findConfig(start) {
  for (let dir = path.resolve(start); ; dir = path.dirname(dir)) {
    for (const name of CONFIG_NAMES) {
      const candidate = path.join(dir, name);
      if (fs.existsSync(candidate)) return candidate;
    }
    const manifest = path.join(dir, 'package.json');
    if (fs.existsSync(manifest)) return manifest;
    if (path.dirname(dir) === dir) return null;
  }
}

function findVitestConfig(dir) {
  for (const name of VITEST_CONFIG_NAMES) {
    const candidate = path.join(dir, name);
    if (fs.existsSync(candidate)) return candidate;
  }
  return null;
}

const usesVitest = manifest => {
  const all = {...manifest.dependencies, ...manifest.devDependencies};
  return 'vitest' in all && !('jest' in all);
};

// What a vitest config imports from vitest and vite, without loading either
// (Vite alone takes half a second): the identity defineConfig and Vitest's
// defaults. A config that needs more falls back to the real import.
const STUB_URL = 'opal-stub:vitest-config';
const STUB_SOURCE = `
const identity = config => config;
export const defineConfig = identity;
export const defineProject = identity;
export const defineWorkspace = identity;
export const mergeConfig = (a, b) => ({...a, ...b, test: {...(a && a.test), ...(b && b.test)}});
export const defaultInclude = ['**/*.{test,spec}.?(c|m)[jt]s?(x)'];
export const defaultExclude = ['**/node_modules/**', '**/.git/**'];
export const configDefaults = {include: defaultInclude, exclude: defaultExclude};
export default {defineConfig};
`;

async function importVitestConfig(file) {
  const Module = require('module');
  let hooks = null;
  if (typeof Module.registerHooks === 'function') {
    hooks = Module.registerHooks({
      resolve(specifier, context, next) {
        if (specifier === 'vitest/config' || specifier === 'vite' || specifier === 'vitest/node') {
          return {shortCircuit: true, url: STUB_URL};
        }
        const resolved = next(specifier, context);
        // Vite bundles a config, so it imports JSON without `with {type:
        // 'json'}`; Node wants the attribute, so the hook supplies it.
        if (resolved.url.endsWith('.json') && !context.importAttributes?.type) {
          return {...resolved, importAttributes: {...context.importAttributes, type: 'json'}};
        }
        return resolved;
      },
      load(url, context, next) {
        if (url === STUB_URL) return {format: 'module', shortCircuit: true, source: STUB_SOURCE};
        return next(url, context);
      },
    });
  }
  try {
    return (await import(`${pathToFileURL(file).href}?opal=${Date.now()}`)).default;
  } catch (error) {
    if (!hooks) throw error;
    hooks.deregister();
    hooks = null;
    return (await import(pathToFileURL(file).href)).default;
  } finally {
    hooks?.deregister();
  }
}

const VITEST_DEFAULT_INCLUDE = ['**/*.{test,spec}.?(c|m)[jt]s?(x)'];
const VITEST_DEFAULT_EXCLUDE = ['**/node_modules/**', '**/.git/**'];

async function loadVitestConfig(dir, file) {
  let loaded = file ? await importVitestConfig(file) : {};
  if (typeof loaded === 'function') loaded = await loaded({command: 'serve', mode: 'test'});
  loaded = loaded || {};
  const test = loaded.test || {};
  const rootDir = path.resolve(dir, test.root || loaded.root || '.');
  const anchor = pattern => (pattern.startsWith('**/') || path.isAbsolute(pattern) ? pattern : path.join(rootDir, pattern));
  const include = (test.include || VITEST_DEFAULT_INCLUDE).map(anchor);
  const exclude = (test.exclude || VITEST_DEFAULT_EXCLUDE).map(anchor);
  const aliases = loaded.resolve && loaded.resolve.alias;
  const moduleNameMapper = {};
  for (const entry of Array.isArray(aliases) ? aliases : Object.entries(aliases || {}).map(([find, replacement]) => ({find, replacement}))) {
    const find = entry.find instanceof RegExp ? entry.find.source : `^${String(entry.find).replaceAll(/[.*+?^${}()|[\]\\]/g, '\\$&')}(/.*)?$`;
    moduleNameMapper[find] = entry.find instanceof RegExp ? entry.replacement : `${entry.replacement}$1`;
  }
  const setupFiles = [test.setupFiles || []].flat().map(entry => resolveConfigPath(rootDir, entry));
  return {
    ...DEFAULTS,
    clearMocks: Boolean(test.clearMocks),
    configPath: file,
    dialect: 'vitest',
    injectGlobals: test.globals === true,
    moduleNameMapper,
    resetMocks: Boolean(test.mockReset),
    restoreMocks: Boolean(test.restoreMocks),
    rootDir,
    roots: [rootDir],
    setupFiles: [],
    setupFilesAfterEnv: setupFiles,
    snapshotFormat: {escapeString: false, printBasicPrototype: false},
    testMatch: [...include, ...exclude.map(pattern => `!${pattern}`)],
    testPathIgnorePatterns: [],
    testRegex: [],
    testTimeout: test.testTimeout || 5000,
  };
}

async function readConfigFile(file) {
  if (path.basename(file) === 'package.json') {
    const manifest = JSON.parse(fs.readFileSync(file, 'utf8'));
    return manifest.jest || {};
  }
  if (file.endsWith('.json')) return JSON.parse(fs.readFileSync(file, 'utf8'));
  let loaded;
  if (file.endsWith('.mjs') || file.endsWith('.mts')) {
    loaded = (await import(pathToFileURL(file).href)).default;
  } else {
    // Node strips TypeScript types itself from 22.18 and 24 on, and a config
    // written with `export default` comes back as a module namespace.
    loaded = require(file);
    if (loaded && (loaded.__esModule || loaded[Symbol.toStringTag] === 'Module')) loaded = loaded.default;
  }
  if (typeof loaded === 'function') loaded = await loaded();
  return loaded || {};
}

function replaceRootDir(rootDir, value) {
  if (typeof value === 'string') return value.replaceAll('<rootDir>', rootDir);
  if (Array.isArray(value)) return value.map(item => replaceRootDir(rootDir, item));
  if (value && typeof value === 'object' && !(value instanceof RegExp)) {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, replaceRootDir(rootDir, item)]));
  }
  return value;
}

const PATH_LISTS = ['roots', 'setupFiles', 'setupFilesAfterEnv', 'snapshotSerializers'];

async function loadConfig({rootDir: start, configPath}) {
  let file = configPath ? path.resolve(start, configPath) : findConfig(start);
  if (!configPath && file && path.basename(file) === 'package.json') {
    const dir = path.dirname(file);
    const manifest = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (!manifest.jest) {
      const vitestConfig = findVitestConfig(dir);
      if (vitestConfig || usesVitest(manifest)) return loadVitestConfig(dir, vitestConfig);
    }
  }
  // vitest.config.ts, and the variants projects name for a second suite:
  // vitest.config.e2e.ts, vitest.e2e.config.mts.
  if (configPath && /^vite(st)?\..*\.[cm]?[jt]s$/.test(path.basename(file))) {
    return loadVitestConfig(path.dirname(file), file);
  }
  let raw = file ? await readConfigFile(file) : {};
  if (Array.isArray(raw.projects) && raw.projects.length > 0) {
    throw new Error('opal test does not support Jest `projects` yet.');
  }
  const base = file ? path.dirname(file) : path.resolve(start);
  const rootDir = raw.rootDir ? path.resolve(base, raw.rootDir) : base;
  const config = {...DEFAULTS, ...replaceRootDir(rootDir, raw)};
  config.rootDir = rootDir;
  config.configPath = file;
  config.roots = (config.roots || [rootDir]).map(root => path.resolve(rootDir, root));
  if (raw.testRegex) {
    config.testRegex = Array.isArray(raw.testRegex) ? raw.testRegex.map(String) : [String(raw.testRegex)];
    config.testMatch = raw.testMatch || [];
  } else {
    config.testRegex = [];
    config.testMatch = config.testMatch || DEFAULT_TEST_MATCH;
  }
  for (const key of PATH_LISTS) {
    if (Array.isArray(config[key])) {
      config[key] = config[key].map(entry => resolveConfigPath(rootDir, entry));
    }
  }
  config.snapshotFormat = {escapeString: false, printBasicPrototype: false, ...config.snapshotFormat};
  config.dialect = 'jest';
  config.jestMajor = installedJestMajor(rootDir);
  return config;
}

// The major version of the Jest the project has installed, or null. opal's
// semantics are Jest 30's; a project on Jest 29 also gets the matchers Jest
// 30 removed (framework.cjs).
function installedJestMajor(rootDir) {
  for (const name of ['jest', 'jest-cli', '@jest/core', 'expect']) {
    try {
      const manifest = JSON.parse(fs.readFileSync(require.resolve(`${name}/package.json`, {paths: [rootDir]}), 'utf8'));
      const major = Number.parseInt(manifest.version, 10);
      if (Number.isFinite(major)) return major;
    } catch {}
  }
  return null;
}

// A path is relative to rootDir; a bare name is a package.
function resolveConfigPath(rootDir, entry) {
  if (typeof entry !== 'string') return entry;
  if (entry.startsWith('.') || path.isAbsolute(entry)) return path.resolve(rootDir, entry);
  try {
    return require.resolve(entry, {paths: [rootDir]});
  } catch {
    return path.resolve(rootDir, entry);
  }
}

module.exports = {loadConfig};
