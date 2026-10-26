// Whether babel-jest, with the project's Babel config, does anything opal's
// own transform (src/transform.rs) doesn't. When it does, babel-jest runs, as
// Jest runs it (transformers.cjs); when it doesn't, opal transforms the file.
//
// Babel's config is read without loading Babel: the root babel.config.* and
// the .babelrc.* (or package.json "babel") in rootDir, the files @babel/core
// finds for babel-jest's `root: rootDir`. A JavaScript config is evaluated
// with a stand-in for Babel's config API. opal's transform covers what a
// config does when it has only:
//
//   - @babel/preset-env, without `loose` (which turns class fields into
//     assignments) or `useBuiltIns: 'usage'` (which loads core-js into the
//     test); opal doesn't lower syntax Node runs as it is, the one difference
//     left (COMPATIBILITY.md);
//   - @babel/preset-typescript, without `onlyRemoveTypeImports`,
//     `allExtensions` or `rewriteImportExtensions`;
//   - @babel/preset-react, whose runtime and import source opal then uses;
//   - plugins preset-env itself is made of, or that only parse syntax.
//
// Anything else, a config Babel would reject, or one that can't be read this
// way, means babel-jest. A .babelrc below rootDir applies only to the files
// under it, so the worker looks for one per directory (babelrcIn).
'use strict';

const fs = require('fs');
const path = require('path');
const {pathToFileURL} = require('url');

// @babel/core's names, config/files/configuration.ts.
const ROOT_CONFIG_NAMES = ['babel.config.js', 'babel.config.cjs', 'babel.config.mjs', 'babel.config.json', 'babel.config.cts'];
const RELATIVE_CONFIG_NAMES = ['.babelrc', '.babelrc.js', '.babelrc.cjs', '.babelrc.mjs', '.babelrc.json', '.babelrc.cts'];

// Options that change nothing a test can observe.
const NEUTRAL_KEYS = new Set([
  'auxiliaryCommentAfter',
  'auxiliaryCommentBefore',
  'browserslistConfigFile',
  'browserslistEnv',
  'comments',
  'compact',
  'generatorOpts',
  'highlightCode',
  'minified',
  'parserOpts',
  'retainLines',
  'shouldPrintComment',
  'sourceMaps',
  'sourceType',
  'targets',
]);

// The plugins @babel/preset-env is made of (@babel/compat-data's
// plugins.json and its bugfix and proposal names), and transform-runtime,
// which moves their helpers into @babel/runtime.
const STANDARD_PLUGINS = new RegExp(
  `^@babel/plugin-(?:syntax-.+|(?:transform|proposal)-(?:${[
    'arrow-functions',
    'async-generator-functions',
    'async-to-generator',
    'block-scoped-functions',
    'block-scoping',
    'class-properties',
    'class-static-block',
    'classes',
    'computed-properties',
    'destructuring',
    'dotall-regex',
    'duplicate-keys',
    'duplicate-named-capturing-groups-regex',
    'dynamic-import',
    'exponentiation-operator',
    'export-namespace-from',
    'for-of',
    'function-name',
    'json-strings',
    'literals',
    'logical-assignment-operators',
    'member-expression-literals',
    'modules-commonjs',
    'named-capturing-groups-regex',
    'new-target',
    'nullish-coalescing-operator',
    'numeric-separator',
    'object-rest-spread',
    'object-super',
    'optional-catch-binding',
    'optional-chaining',
    'parameters',
    'private-methods',
    'private-property-in-object',
    'property-literals',
    'regenerator',
    'regexp-modifiers',
    'reserved-words',
    'runtime',
    'shorthand-properties',
    'spread',
    'sticky-regex',
    'template-literals',
    'typeof-symbol',
    'unicode-escapes',
    'unicode-property-regex',
    'unicode-regex',
    'unicode-sets-regex',
  ].join('|')}))$`,
);

const ENV_OPTIONS_OPAL_IGNORES = new Set([
  'browserslistEnv',
  'bugfixes',
  'configPath',
  'corejs',
  'debug',
  'exclude',
  'forceAllTransforms',
  'ignoreBrowserslistConfig',
  'include',
  'modules',
  'shippedProposals',
  'spec',
  'targets',
]);
const TYPESCRIPT_OPTIONS_OPAL_IGNORES = new Set([
  'allowDeclareFields',
  'allowNamespaces',
  'disallowAmbiguousJSXLike',
  'ignoreExtensions',
  'jsxPragma',
  'jsxPragmaFrag',
  'optimizeConstEnums',
]);
const REACT_OPTIONS_OPAL_IGNORES = new Set(['development', 'pure', 'throwIfNamespace', 'useBuiltIns', 'useSpread']);

// A preset's or plugin's name as Babel resolves it: `@babel/env` is
// @babel/preset-env, `module:` is taken as written.
function normalizeName(name, kind) {
  if (name.startsWith('module:')) return name.slice('module:'.length);
  const match = name.match(/^@babel\/(?!preset-|plugin-)(.+)$/);
  return match ? `@babel/${kind}-${match[1]}` : name;
}

function entryOf(item) {
  const [value, options] = Array.isArray(item) ? item : [item, undefined];
  return {name: typeof value === 'string' ? value : null, options: options || {}};
}

const unknownKeys = (options, known) => Object.keys(options).filter(key => !known.has(key));

// {jsx} when opal's transform covers the config, else the reason it doesn't.
function judge(options) {
  const plan = {jsx: null};
  for (const key of Object.keys(options)) {
    if (key === 'presets' || key === 'plugins' || NEUTRAL_KEYS.has(key)) continue;
    return {custom: `the option "${key}"`};
  }
  for (const item of options.presets || []) {
    const {name, options: presetOptions} = entryOf(item);
    const preset = name && normalizeName(name, 'preset');
    if (preset === '@babel/preset-env') {
      const unknown = unknownKeys(presetOptions, ENV_OPTIONS_OPAL_IGNORES).filter(key => !(key === 'loose' && !presetOptions.loose) && !(key === 'useBuiltIns' && presetOptions.useBuiltIns !== 'usage'));
      if (unknown.length) return {custom: `${preset}'s ${unknown[0]}`};
    } else if (preset === '@babel/preset-typescript') {
      const unknown = unknownKeys(presetOptions, TYPESCRIPT_OPTIONS_OPAL_IGNORES).filter(key => !(key === 'onlyRemoveTypeImports' && !presetOptions.onlyRemoveTypeImports));
      if (unknown.length) return {custom: `${preset}'s ${unknown[0]}`};
    } else if (preset === '@babel/preset-react') {
      const {runtime = 'classic', importSource, ...rest} = presetOptions;
      const unknown = unknownKeys(rest, REACT_OPTIONS_OPAL_IGNORES);
      if (unknown.length) return {custom: `${preset}'s ${unknown[0]}`};
      if (runtime !== 'classic' && runtime !== 'automatic') return {custom: `${preset}'s runtime ${runtime}`};
      plan.jsx = {classic: runtime === 'classic', importSource: importSource || null};
    } else if (preset !== 'babel-preset-jest') {
      return {custom: `the preset ${name || '(inline)'}`};
    }
  }
  for (const item of options.plugins || []) {
    const {name, options: pluginOptions} = entryOf(item);
    const plugin = name && normalizeName(name, 'plugin');
    if (!plugin || !STANDARD_PLUGINS.test(plugin)) return {custom: `the plugin ${name || '(inline)'}`};
    if (pluginOptions.loose) return {custom: `${plugin}'s loose`};
  }
  return plan;
}

// What @babel/core's config API answers, for babel-jest's caller.
function configApi(envName) {
  const cache = () => {};
  cache.forever = () => {};
  cache.never = () => {};
  cache.using = fn => fn();
  cache.invalidate = fn => fn();
  return {
    addExternalDependency() {},
    assertVersion() {},
    assumption: () => undefined,
    async: () => false,
    cache,
    caller: fn =>
      fn({
        name: 'babel-jest',
        supportsDynamicImport: false,
        supportsExportNamespaceFrom: false,
        supportsStaticESM: false,
        supportsTopLevelAwait: false,
      }),
    env(value) {
      if (value === undefined) return envName;
      if (typeof value === 'function') return value(envName);
      return [value].flat().includes(envName);
    },
    targets: () => ({}),
    version: '7.0.0',
  };
}

async function readOptions(file, envName) {
  const name = path.basename(file);
  let options;
  if (name === 'package.json') {
    options = JSON.parse(fs.readFileSync(file, 'utf8')).babel;
  } else if (name === '.babelrc' || name.endsWith('.json')) {
    options = JSON.parse(fs.readFileSync(file, 'utf8'));
  } else if (name.endsWith('.mjs')) {
    options = (await import(pathToFileURL(file).href)).default;
  } else if (name.endsWith('.cts')) {
    throw new Error('a TypeScript config');
  } else {
    delete require.cache[file];
    options = require(file);
    if (options && options.__esModule) options = options.default;
  }
  if (typeof options === 'function') options = options(configApi(envName));
  if (!options || typeof options !== 'object' || typeof options.then === 'function') {
    throw new Error('not an object');
  }
  // `env` applies the options under the current environment's name on top.
  const {env, ...base} = options;
  const extra = env && env[envName];
  if (!extra) return base;
  return {...base, ...extra, plugins: [...(base.plugins || []), ...(extra.plugins || [])], presets: [...(base.presets || []), ...(extra.presets || [])]};
}

function relativeConfigIn(dir) {
  const found = RELATIVE_CONFIG_NAMES.map(name => path.join(dir, name)).filter(file => fs.existsSync(file));
  const manifest = path.join(dir, 'package.json');
  if (fs.existsSync(manifest)) {
    try {
      if (JSON.parse(fs.readFileSync(manifest, 'utf8')).babel) found.push(manifest);
    } catch {}
  }
  return found;
}

// The plan for babel-jest in a project: {jsx, files} when opal's transform
// does what it would, {custom} when only babel-jest does. `files` are the
// config files read, for the worker to know which ones were seen.
async function babelPlan(rootDir, transformerConfig) {
  if (transformerConfig && Object.keys(transformerConfig).length > 0) return {custom: "babel-jest's own options"};
  // jest-cli puts NODE_ENV=test in the environment when it isn't set.
  const envName = process.env.BABEL_ENV || process.env.NODE_ENV || 'test';
  const files = [
    ...ROOT_CONFIG_NAMES.map(name => path.join(rootDir, name)).filter(file => fs.existsSync(file)),
    ...relativeConfigIn(rootDir),
  ];
  if (fs.existsSync(path.join(rootDir, '.babelignore'))) return {custom: '.babelignore'};
  const plan = {files, jsx: null};
  for (const file of files) {
    let options;
    try {
      options = await readOptions(file, envName);
    } catch (error) {
      return {custom: `${path.relative(rootDir, file)} (${error.message})`};
    }
    const verdict = judge(options);
    if (verdict.custom) return {custom: `${verdict.custom} in ${path.relative(rootDir, file)}`};
    if (verdict.jsx) plan.jsx = verdict.jsx;
  }
  return plan;
}

module.exports = {RELATIVE_CONFIG_NAMES, babelPlan, relativeConfigIn};
