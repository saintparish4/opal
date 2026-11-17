// Jest's transformer protocol (@jest/transform's ScriptTransformer), for the
// transforms config.cjs's plan doesn't give to opal: the project's own
// transformer, loaded into the worker the way Jest loads it, called with
// Jest's TransformOptions, and its results kept on disk across runs, under
// Jest's cache key.
//
// ts-jest is run to emit only, with transpileModule file by file, which is
// what it does itself under isolatedModules. Its type-check, the slow part,
// happens once per run instead, beside the tests (checker.cjs). Its results
// are kept under a key opal works out without loading it (tsJestBasis), so
// a run whose files are in the cache doesn't load TypeScript at all.
'use strict';

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const {RELATIVE_CONFIG_NAMES} = require('./babel-config.cjs');

// @jest/transform's CACHE_VERSION, and opal's, for a change in what's kept.
const CACHE_VERSION = '1';
const OPAL_CACHE_VERSION = '1';
const CACHE_ROOT = process.env.OPAL_TEST_CACHE || null;
const CACHE_DIR = CACHE_ROOT ? path.join(CACHE_ROOT, 'test-transform-jest') : null;
const DEFAULT_IGNORE_PATTERNS = ['/node_modules/', `\\.pnp\\.[^\\${path.sep}]+$`];

// What jest-runtime tells a transformer about the module system it loads
// into, for CommonJS without --experimental-vm-modules: opal's only one.
const CALLER = {
  supportsDynamicImport: false,
  supportsExportNamespaceFrom: false,
  supportsStaticESM: false,
  supportsTopLevelAwait: false,
};
const CALLER_SUPPORT = JSON.stringify(CALLER);

const sha1 = (...parts) => {
  const hash = crypto.createHash('sha1');
  for (const part of parts) hash.update(part).update('\0', 'utf8');
  return hash.digest('hex').slice(0, 32);
};

// fast-json-stable-stringify, which makes Jest's configString.
function stableStringify(data) {
  const seen = new Set();
  const stringify = node => {
    if (node && typeof node.toJSON === 'function') node = node.toJSON();
    if (node === undefined) return undefined;
    if (typeof node === 'number') return Number.isFinite(node) ? String(node) : 'null';
    if (typeof node !== 'object') return JSON.stringify(node);
    if (node === null) return 'null';
    if (Array.isArray(node)) return `[${node.map(item => stringify(item) || 'null').join(',')}]`;
    if (seen.has(node)) throw new TypeError('Converting circular structure to JSON');
    seen.add(node);
    const fields = [];
    for (const key of Object.keys(node).sort()) {
      const value = stringify(node[key]);
      if (value) fields.push(`${JSON.stringify(key)}:${value}`);
    }
    seen.delete(node);
    return `{${fields.join(',')}}`;
  };
  return stringify(data);
}

// The ProjectConfig a transformer is given, in jest-config's shape: opal's
// config is the same but for a few fields of its own, and moduleNameMapper
// kept as an object.
function projectConfigOf(config) {
  const {babelJsx, dialect, jestMajor, manualMocks, transformRunners, ...jest} = config;
  return {
    ...jest,
    cache: true,
    cacheDirectory: config.cacheDirectory || path.join(CACHE_ROOT || os.tmpdir(), 'jest'),
    extensionsToTreatAsEsm: config.extensionsToTreatAsEsm || [],
    id: crypto.createHash('sha256').update(config.rootDir).update(config.configPath || '').digest('hex').slice(0, 32),
    moduleNameMapper: Object.entries(config.moduleNameMapper || {}),
    transformIgnorePatterns: config.transformIgnorePatterns || DEFAULT_IGNORE_PATTERNS,
  };
}

// --- the cache --------------------------------------------------------------

// One JSON file per result, {code, map}, written whole and renamed into place.
let staging = 0;

function readCache(key) {
  if (!CACHE_DIR) return null;
  let text;
  try {
    text = fs.readFileSync(path.join(CACHE_DIR, key.slice(0, 2), key), 'utf8');
  } catch {
    return null;
  }
  try {
    const entry = JSON.parse(text);
    return typeof entry.code === 'string' ? entry : null;
  } catch {
    return null;
  }
}

function writeCache(key, entry) {
  if (!CACHE_DIR) return;
  try {
    const dir = path.join(CACHE_DIR, key.slice(0, 2));
    fs.mkdirSync(dir, {recursive: true});
    const temporary = path.join(dir, `.${key}.${process.pid}.${staging++}`);
    fs.writeFileSync(temporary, JSON.stringify(entry));
    fs.renameSync(temporary, path.join(dir, key));
  } catch {}
}

// --- source maps --------------------------------------------------------------

// convert-source-map's fromSource: the last inline map comment, which Jest
// reads when a transformer returns no map.
const INLINE_MAP = /^\s*?\/[/*][@#]\s+?sourceMappingURL=data:(((?:application|text)\/json)(?:;charset=([^;,]+?)?)?)?(?:;(base64))?,(.*?)$/gm;

function inlineSourceMap(code) {
  const comments = code.match(INLINE_MAP);
  if (!comments) return null;
  const [, , , , encoding, data] = new RegExp(INLINE_MAP.source, 'm').exec(comments.at(-1));
  const json = encoding === 'base64' ? Buffer.from(data, 'base64').toString('utf8') : decodeURIComponent(data);
  return JSON.parse(json);
}

function mapOf(processed) {
  let map = processed.map;
  if (map == null || map === '') {
    try {
      map = inlineSourceMap(processed.code);
    } catch {
      map = null;
    }
  }
  if (typeof map === 'string') map = JSON.parse(map);
  return map && typeof map.mappings === 'string' ? {mappings: map.mappings} : null;
}

// --- errors -------------------------------------------------------------------

function chalk() {
  return require('./chalk.cjs');
}

const DOCUMENTATION_NOTE = () => `  ${chalk().bold('Code Transformation Documentation:')}\n  https://jestjs.io/docs/code-transformation\n`;
const UPGRADE_NOTE = () =>
  `  ${chalk().bold('This error may be caused by a breaking change in Jest 28:')}\n  https://jest-archive-august-2023.netlify.app/docs/28.x/upgrading-to-jest28#transformer\n`;

// @jest/transform's runtimeErrorsAndWarnings.ts.
const invalidTransformer = transformPath =>
  chalk().red(
    [
      chalk().bold('● Invalid transformer module:'),
      `  "${transformPath}" specified in the "transform" object of Jest configuration`,
      '  must export a `process` or `processAsync` or `createTransformer` function.',
      '',
    ].join('\n') + DOCUMENTATION_NOTE(),
  );
const invalidSyncTransformer = transformPath =>
  chalk().red(
    [
      chalk().bold('● Invalid synchronous transformer module:'),
      `  "${transformPath}" specified in the "transform" object of Jest configuration`,
      '  must export a `process` function.',
      '',
    ].join('\n') + DOCUMENTATION_NOTE(),
  );
const invalidReturnValue = transformPath =>
  chalk().red(
    [
      chalk().bold('● Invalid return value:'),
      '  `process()` or/and `processAsync()` method of code transformer found at ',
      `  "${transformPath}" `,
      '  should return an object or a Promise resolving to an object. The object ',
      '  must have `code` property with a string of processed code.',
      '',
    ].join('\n') +
      UPGRADE_NOTE() +
      DOCUMENTATION_NOTE(),
  );

// @jest/transform's handlePotentialSyntaxError, which every error a
// transformer throws goes through.
function handlePotentialSyntaxError(error) {
  if (!(error instanceof Error) && !(error && typeof error.message === 'string')) return error;
  if (error.codeFrame != null) error.stack = `${error.message}\n${error.codeFrame}`;
  if (error.name === 'SyntaxError' && !error.message.includes(' expected')) {
    const c = chalk();
    const DOT = ' • ';
    error.stack = `${c.bold.red('Jest encountered an unexpected token')}

Jest failed to parse a file. This happens e.g. when your code or its dependencies use non-standard JavaScript syntax, or when Jest is not configured to support such syntax.

Out of the box Jest supports Babel, which will be used to transform your files into valid JS based on your Babel configuration.

By default "node_modules" folder is ignored by transformers.

Here's what you can do:
${DOT}If you are trying to use TypeScript, see ${c.underline('https://jestjs.io/docs/getting-started#using-typescript')}
${DOT}To have some of your "node_modules" files transformed, you can specify a custom ${c.bold('"transformIgnorePatterns"')} in your config.
${DOT}If you need a custom transformation, specify a ${c.bold('"transform"')} option in your config.
${DOT}If you simply want to mock your non-JS modules (e.g. binary assets) you can stub them out with the ${c.bold('"moduleNameMapper"')} config option.

You'll find more details and examples of these config options in the docs:
${c.cyan('https://jestjs.io/docs/configuration')}
For information about custom transformations, see:
${c.cyan('https://jestjs.io/docs/code-transformation')}

${c.bold.red('Details:')}

${error.stack ?? ''}`.trimEnd();
  }
  return error;
}

// --- ts-jest ------------------------------------------------------------------

// ts-jest picks, per ConfigSet, between a LanguageService that type-checks
// each file it emits and transpileModule: `isolatedModules`. opal sets it on
// the ConfigSet after ts-jest has read the config, so ts-jest emits file by
// file and prints nothing about it (its own option for this is deprecated,
// and warns), and turns off what transpileModule would report: the
// LanguageService's diagnostics come from checker.cjs instead. A project
// that sets isolatedModules itself is left as it is, and so is a ts-jest
// without the hook, which checks types itself.
function emitOnly(transformer) {
  const create = transformer._createConfigSet;
  if (typeof create !== 'function' || typeof transformer._createCompiler !== 'function') return transformer;
  transformer._createConfigSet = function createConfigSet(config) {
    const configSet = create.call(this, config);
    if (!configSet.isolatedModules) {
      configSet.isolatedModules = true;
      configSet.shouldReportDiagnostics = () => false;
    }
    return configSet;
  };
  return transformer;
}

// ts-jest's own TSError, so what's thrown is what ts-jest throws: its
// message the formatted diagnostics, its stack empty.
function tsError(entry, verdict) {
  try {
    const manifest = packageJsonOf(entry.transformPath);
    const {TSError} = require(path.join(path.dirname(manifest), 'dist', 'utils', 'ts-error.js'));
    return new TSError(verdict.diagnosticText, verdict.codes);
  } catch {
    const error = new Error(verdict.message);
    error.name = 'TSError';
    Object.defineProperty(error, 'stack', {value: ''});
    return error;
  }
}

// What ts-jest does with a check's diagnostics under `warnOnly`: logs them.
function warnOf(entry, error) {
  try {
    const manifest = packageJsonOf(entry.transformPath);
    const {rootLogger} = require(path.join(path.dirname(manifest), 'dist', 'utils', 'logger.js'));
    rootLogger.child({namespace: 'ts-compiler'}).warn({error}, error.message);
  } catch {
    process.stderr.write(`ts-jest[ts-compiler] (WARN) ${error.message}\n`);
  }
}

const packageJsonOf = file => {
  for (let dir = path.dirname(file); path.dirname(dir) !== dir; dir = path.dirname(dir)) {
    const manifest = path.join(dir, 'package.json');
    if (fs.existsSync(manifest)) return manifest;
  }
  return null;
};

// tsconfig.json is JSON with comments and trailing commas.
function parseJsonc(text) {
  let out = '';
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      out += text.slice(i, j + 1);
      i = j;
    } else if (c === '/' && text[i + 1] === '/') {
      while (i < text.length && text[i] !== '\n') i++;
      out += '\n';
    } else if (c === '/' && text[i + 1] === '*') {
      i = text.indexOf('*/', i + 2);
      if (i < 0) break;
      i++;
    } else {
      out += c;
    }
  }
  return JSON.parse(out.replace(/,(\s*[}\]])/g, '$1'));
}

// TypeScript's resolution of `extends`: a path, with .json added when
// missing, or a package's tsconfig.json.
function resolveExtends(name, dir) {
  if (name.startsWith('.') || path.isAbsolute(name)) {
    const file = path.resolve(dir, name);
    if (fs.existsSync(file) && fs.statSync(file).isFile()) return file;
    if (fs.existsSync(`${file}.json`)) return `${file}.json`;
    return null;
  }
  for (const request of name.endsWith('.json') ? [name] : [`${name}/tsconfig.json`, `${name}.json`, name]) {
    try {
      return require.resolve(request, {paths: [dir]});
    } catch {}
  }
  return null;
}

// The tsconfig files ts-jest reads, each with its text: the one its
// `tsconfig` option names (relative to cwd), or else the nearest
// tsconfig.json from rootDir up, which compiler options given inline go on
// top of; then each one it extends. null when one can't be read this way.
function tsconfigChain(projectConfig, options) {
  const chain = [];
  let file;
  if (typeof options.tsconfig === 'string') {
    file = options.tsconfig.startsWith('<rootDir>')
      ? path.resolve(path.join(projectConfig.rootDir, options.tsconfig.slice(9)))
      : path.resolve(projectConfig.cwd || process.cwd(), options.tsconfig);
  } else {
    for (let dir = projectConfig.rootDir; ; dir = path.dirname(dir)) {
      const candidate = path.join(dir, 'tsconfig.json');
      if (fs.existsSync(candidate)) {
        file = candidate;
        break;
      }
      if (path.dirname(dir) === dir) return chain;
    }
  }
  const pending = [file];
  while (pending.length > 0) {
    const next = pending.shift();
    let text;
    let parsed;
    try {
      text = fs.readFileSync(next, 'utf8');
      parsed = parseJsonc(text);
    } catch {
      return null;
    }
    chain.push(next, text);
    for (const name of [parsed.extends || []].flat()) {
      const resolved = resolveExtends(name, path.dirname(next));
      if (!resolved) return null;
      pending.push(resolved);
    }
  }
  return chain;
}

function versionOf(request, from) {
  try {
    const file = require.resolve(`${request}/package.json`, {paths: [from]});
    return `${file}@${JSON.parse(fs.readFileSync(file, 'utf8')).version}`;
  } catch {
    return null;
  }
}

// --- the transformers -----------------------------------------------------------

// One per config a worker is given, as Jest keeps one cache per configString.
const projects = new Map();

class ProjectTransforms {
  constructor(config) {
    this.config = config;
    this.projectConfig = projectConfigOf(config);
    this.configString = stableStringify(this.projectConfig);
    // Jest's cacheFS, one per runtime: what a transformer has read.
    this.cacheFS = new Map();
    this.entries = config.transform.map(([pattern, transformPath, transformerConfig], index) => ({
      index,
      pattern: new RegExp(pattern),
      runner: config.transformRunners[index].runner,
      tsJestBasis: undefined,
      transformPath,
      transformer: null,
      transformerConfig: transformerConfig === null ? undefined : transformerConfig,
    }));
  }

  // The first entry whose pattern matches, as ScriptTransformer's
  // _getTransformPatternAndPath picks.
  entryFor(filename) {
    return this.entries.find(entry => entry.pattern.test(filename)) || null;
  }

  // babel-jest itself, for a file opal would transform in its place but for
  // a .babelrc of its own (babelrcBetween).
  babelJest(entry) {
    if (!entry.babelJest) entry.babelJest = {...entry, runner: 'jest', transformer: null};
    return entry.babelJest;
  }

  // loadTransformers, for one entry, when a file first needs it.
  load(entry) {
    if (entry.transformer) return entry.transformer;
    let transformer = require(entry.transformPath);
    if (transformer && transformer.__esModule && transformer.default) transformer = transformer.default;
    if (transformer == null) throw new Error(invalidTransformer(entry.transformPath));
    if (typeof transformer.createTransformer === 'function') {
      transformer = transformer.createTransformer(entry.transformerConfig);
      if (transformer && typeof transformer.then === 'function') {
        throw new Error(`opal test can't wait for ${entry.transformPath}'s createTransformer, which returns a promise.`);
      }
    }
    if (typeof transformer.process !== 'function' && typeof transformer.processAsync !== 'function') {
      throw new TypeError(invalidTransformer(entry.transformPath));
    }
    if (entry.runner === 'ts-jest') emitOnly(transformer);
    entry.transformer = transformer;
    return transformer;
  }

  transformOptions(entry) {
    return {
      ...CALLER,
      cacheFS: this.cacheFS,
      config: this.projectConfig,
      configString: this.configString,
      instrument: false,
      transformerConfig: entry.transformerConfig === undefined ? {} : entry.transformerConfig,
    };
  }

  // What ts-jest's emit depends on, read without loading it: its version
  // and TypeScript's, its options, the tsconfig files and the config. null
  // when it depends on more (a Babel pass, AST transformers, another
  // compiler, TS_JEST_HOOKS), and the key is then ts-jest's own.
  tsJestBasis(entry) {
    if (entry.tsJestBasis !== undefined) return entry.tsJestBasis;
    const options = {...(this.config.globals && this.config.globals['ts-jest']), ...entry.transformerConfig};
    let basis = null;
    if (!options.babelConfig && !options.astTransformers && !options.compiler && !process.env.TS_JEST_HOOKS) {
      const manifest = packageJsonOf(entry.transformPath);
      let tsJest = null;
      try {
        tsJest = `${manifest}@${JSON.parse(fs.readFileSync(manifest, 'utf8')).version}`;
      } catch {}
      const typescript = tsJest && versionOf('typescript', path.dirname(manifest));
      const chain = typescript && tsconfigChain(this.projectConfig, options);
      if (chain) {
        basis = sha1('ts-jest emit', OPAL_CACHE_VERSION, tsJest, typescript, this.configString, stableStringify(options) || '', ...chain);
      }
    }
    entry.tsJestBasis = basis;
    return basis;
  }

  // ScriptTransformer's _getCacheKey, namespaced by the transformer, which
  // Jest's cache directory does for it.
  cacheKey(entry, filename, content, options) {
    if (entry.runner === 'ts-jest') {
      const basis = this.tsJestBasis(entry);
      if (basis) return sha1(basis, CALLER_SUPPORT, filename, content);
    }
    const transformer = this.load(entry);
    let key;
    if (typeof transformer.getCacheKey === 'function') {
      const transformerKey = transformer.getCacheKey(content, filename, options);
      if (transformerKey != null) key = sha1(transformerKey, CALLER_SUPPORT, CACHE_VERSION);
    }
    if (!key) key = sha1(content, options.configString, '', CALLER_SUPPORT, filename, '', CACHE_VERSION);
    return sha1(OPAL_CACHE_VERSION, entry.transformPath, entry.runner, key);
  }

  // transformSource: {code, map}, map in source map v3's shape or null.
  // For ts-jest, then what the run's type-check says about the file, as
  // ts-jest says it: a TSError thrown, or with warnOnly a warning.
  transform(entry, filename, source, bridge) {
    const result = this.emit(entry, filename, source);
    if (entry.runner === 'ts-jest' && bridge) {
      const {verdict} = bridge.request({check: filename, entry: entry.index});
      if (verdict) {
        const error = tsError(entry, verdict);
        if (verdict.throws) throw error;
        warnOf(entry, error);
      }
    }
    return result;
  }

  emit(entry, filename, source) {
    const content = source.startsWith('#!') ? source.replace(/^#!.*/, '') : source;
    try {
      const options = this.transformOptions(entry);
      const key = this.cacheKey(entry, filename, content, options);
      const cached = readCache(key);
      if (cached) return cached;
      const transformer = this.load(entry);
      if (typeof transformer.process !== 'function') throw new Error(invalidSyncTransformer(entry.transformPath));
      const processed = transformer.process(content, filename, options);
      if (processed == null || typeof processed.code !== 'string') throw new Error(invalidReturnValue(entry.transformPath));
      const result = {code: processed.code, map: mapOf(processed)};
      writeCache(key, result);
      return result;
    } catch (error) {
      throw handlePotentialSyntaxError(error);
    }
  }
}

// A .babelrc that applies to `filename` and isn't rootDir's, which
// babel-config.cjs read: @babel/core looks from the file's folder up to its
// package's, and uses the first one it finds. A file in a package of its own
// below rootDir gets none, babelrcRoots being rootDir's package alone.
const babelrcs = new Map();
function babelrcBetween(filename, rootDir) {
  const dirs = [];
  let found = null;
  for (let dir = path.dirname(filename); dir !== rootDir && dir.startsWith(rootDir + path.sep); dir = path.dirname(dir)) {
    const known = babelrcs.get(dir);
    if (known !== undefined) {
      found = known;
      break;
    }
    dirs.push(dir);
    if (fs.existsSync(path.join(dir, 'package.json'))) {
      found = false;
      break;
    }
    const config = RELATIVE_CONFIG_NAMES.map(name => path.join(dir, name)).find(file => fs.existsSync(file));
    if (config) {
      found = config;
      break;
    }
  }
  for (const dir of dirs) babelrcs.set(dir, found || false);
  return found || null;
}

function projectTransforms(config) {
  const key = stableStringify(config.transform) + stableStringify(projectConfigOf(config));
  let project = projects.get(key);
  if (!project) {
    project = new ProjectTransforms(config);
    projects.set(key, project);
  }
  return project;
}

module.exports = {babelrcBetween, projectConfigOf, projectTransforms, stableStringify, tsconfigChain};
