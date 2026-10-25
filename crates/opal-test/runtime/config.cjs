// Finding and normalizing a Jest config, after jest-config's
// resolveConfigPath.ts, readConfigFileAndSetRootDir.ts and normalize.ts.
//
// From the starting folder upward, the first folder holding a
// jest.config.{ts,js,mjs,cjs,cts,mts,json} or a package.json wins; a
// package.json without a "jest" key means the defaults, with that folder as
// rootDir. A config can export an object or a function returning one, sync
// or async. What comes back has every default filled in, `<rootDir>`
// replaced and every path absolute, so nothing downstream guesses.
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
  return config;
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
