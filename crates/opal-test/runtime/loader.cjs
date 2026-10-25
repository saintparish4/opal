// The module registry a test file loads through: CommonJS, evaluated inside
// the file's context, with Jest's module mocking. After jest-runtime 30.5.2
// (index.ts, internals/CjsLoader.ts, internals/MockState.ts,
// internals/Resolution.ts, internals/cjsRequire.ts) and jest-resolve's
// getModuleID and getMockModule.
//
// Every test file gets its own registry, so a module's state never survives
// from one file to the next, which is Jest's isolation. Node's built-in
// modules are the exception, as in Jest: they come from the host, except
// `process`, which is the context's own copy.
//
// Mocking, as Jest decides it (SEMANTICS.md has the rules):
//   - `jest.mock(name, factory)` registers a factory; `jest.mock(name)`
//     marks the module mocked, and it is then served by a manual mock (a
//     file in a `__mocks__` folder) or else an automock generated from the
//     real module's shape;
//   - a manual mock in a root `__mocks__` folder named like a package or a
//     core module replaces it without any `jest.mock` call;
//   - `automock` mocks everything not unmocked, except core modules and
//     what `unmockedModulePathPatterns` exempts.
// ES module mocking (`unstable_mockModule`) isn't here: test files load as
// CommonJS.
//
// Each module is wrapped exactly as Jest wraps it, `jest` included as a
// module-local, so stack traces and `arguments` read the same. A compiled
// module is kept per worker and run again in the next file's context, which
// spares recompiling node_modules for every test file.
'use strict';

const fs = require('fs');
const path = require('path');
const vm = require('vm');
const Module = require('module');
const {pathToFileURL} = require('url');

const builtinModules = new Set(Module.builtinModules);
const isBuiltin = request =>
  request.startsWith('node:') || builtinModules.has(request) || builtinModules.has(request.split('/')[0]);
const normalizeCore = request => (request.startsWith('node:') ? request.slice(5) : request);

const NODE_MODULES = `${path.sep}node_modules${path.sep}`;
// Part of every module ID, as jest-resolve folds the resolve options into
// it; constant here, since opal resolves CommonJS one way.
const CJS_OPTIONS = JSON.stringify({conditions: ['require', 'node', 'default', 'node', 'node-addons']});

const scriptCache = new Map();
const IMPORT_META = Symbol.for('opal.importMeta');

// The module wrapper. A test or setup file in Vitest's dialect may await at
// its top level, as the ES module it is there; its wrapper is then an async
// function, and the plain one is padded to the same length so a column on
// the first line means the same in both (sourcemaps.cjs).
const wrap = (code, isAsync) =>
  `({"Object.<anonymous>":${isAsync ? 'async ' : '      '}function(module,exports,require,__dirname,__filename,jest){${code}\n}});`;

function compile(filename, code, isAsync = false) {
  const key = `${filename}\0${isAsync}\0${code}`;
  let script = scriptCache.get(key);
  if (!script) {
    script = new vm.Script(wrap(code, isAsync), {filename});
    scriptCache.set(key, script);
  }
  return script;
}

const awaitsAtTopLevel = error => error instanceof SyntaxError && /await is only valid/.test(error.message);

// A require anchored in `dir`, for Node's resolution of a bare name from
// there: `exports` with the require conditions, node_modules up the tree,
// and a package importing itself by its own name through its `exports`,
// which jest-resolve allows too and `require.resolve(name, {paths})` misses
// (it resolves self-references from its caller, opal's runtime).
const requiresByDir = new Map();
function requireIn(dir) {
  let anchored = requiresByDir.get(dir);
  if (!anchored) {
    anchored = Module.createRequire(path.join(dir, 'noop.js'));
    requiresByDir.set(dir, anchored);
  }
  return anchored;
}

const stripBOM = text => (text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
const isFile = file => {
  try {
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
};
const isDirectory = dir => {
  try {
    return fs.statSync(dir).isDirectory();
  } catch {
    return false;
  }
};

class ModuleNotFoundError extends Error {
  constructor(message, moduleName) {
    super(message);
    this.code = 'MODULE_NOT_FOUND';
    this.moduleName = moduleName;
    this.name = 'Error';
  }
}

class Loader {
  constructor({config, context, global, testPath, mocks, transform}) {
    this.config = config;
    this.context = context;
    this.global = global;
    this.testPath = testPath;
    this.transform = transform;
    this.extensions = config.moduleFileExtensions.map(extension => `.${extension}`);
    this.moduleNameMapper = Object.entries(config.moduleNameMapper || {}).map(([pattern, target]) => [
      new RegExp(pattern),
      target,
    ]);
    // jest-haste-map's mocks: every file in a `__mocks__` folder under the
    // roots, by its path inside that folder, without extension.
    this.manualMocks = new Map(Object.entries(mocks || {}));
    this.createJestObject = () => undefined;
    this.moduleMocker = null;
    this.mainModule = null;

    this.moduleRegistry = new Map();
    this.mockRegistry = new Map();
    this.isolated = null;

    // MockState
    this.automock = Boolean(config.automock);
    this.unmockList =
      config.unmockedModulePathPatterns && config.unmockedModulePathPatterns.length > 0
        ? new RegExp(config.unmockedModulePathPatterns.join('|'))
        : undefined;
    this.explicitMock = new Map();
    this.factories = new Map();
    this.virtualMocks = new Map();
    this.shouldMockCache = new Map();
    this.unmockTransitiveCache = new Map();
    this.transitiveShouldMock = new Map();
    this.mockMetadata = new Map();
    this.onGenerateMockCallbacks = new Set();
    this.currentlyExecuting = null;
    this.moduleIdCache = new Map();
  }

  // --- registries ------------------------------------------------------------

  modules() {
    return this.isolated ? this.isolated.modules : this.moduleRegistry;
  }

  mocks() {
    return this.isolated ? this.isolated.mocks : this.mockRegistry;
  }

  // jest.isolateModules: what `fn` requires comes from registries of its own,
  // thrown away afterwards.
  isolateModules(fn) {
    this.enterIsolated('isolateModules');
    try {
      fn();
    } finally {
      this.isolated = null;
    }
  }

  async isolateModulesAsync(fn) {
    this.enterIsolated('isolateModulesAsync');
    try {
      await fn();
    } finally {
      this.isolated = null;
    }
  }

  enterIsolated(name) {
    if (this.isolated) {
      throw new Error(
        `${name} cannot be nested inside another isolateModules or isolateModulesAsync.`,
      );
    }
    this.isolated = {mocks: new Map(), modules: new Map()};
  }

  // Generating an automock loads the real module; that must not leave it,
  // or anything it required, in the test's registries.
  withScratchRegistries(fn) {
    const saved = this.isolated;
    this.isolated = {mocks: new Map(), modules: new Map()};
    try {
      return fn();
    } finally {
      this.isolated = saved;
    }
  }

  // resetModules keeps the mock state: factories, explicit mocks and
  // virtual marks survive it.
  resetModules() {
    this.isolated = null;
    this.moduleRegistry.clear();
    this.mockRegistry.clear();
  }

  // --- resolution ------------------------------------------------------------

  resolve(from, request) {
    if (isBuiltin(request)) return request;
    const mapped = this.resolveStub(from, request);
    if (mapped) return mapped;
    const dir = path.dirname(from);
    if (request.startsWith('./') || request.startsWith('../') || request === '.' || request === '..' || path.isAbsolute(request)) {
      const target = path.resolve(dir, request);
      const found = this.tryFile(target) || this.tryDirectory(target) || this.tryTypeScriptSibling(target);
      if (found) return found;
    } else {
      const found = this.resolvePackage(dir, request);
      if (found) return found;
    }
    const relative = path.relative(path.dirname(this.testPath), from) || path.basename(from);
    throw new ModuleNotFoundError(`Cannot find module '${request}' from '${relative}'`, request);
  }

  // moduleNameMapper, which jest-resolve calls resolving a "stub".
  resolveStub(from, request) {
    for (const [regex, target] of this.moduleNameMapper) {
      const match = request.match(regex);
      if (!match) continue;
      const targets = Array.isArray(target) ? target : [target];
      for (const candidate of targets) {
        const replaced = candidate.replaceAll(/\$(\d+)/g, (_, index) => match[Number(index)] ?? '');
        try {
          return this.resolveUnmapped(from, replaced);
        } catch {}
      }
      throw new ModuleNotFoundError(
        `Configuration error:\n\nCould not locate module ${request} mapped as:\n${targets.join(', ')}.`,
        request,
      );
    }
    return null;
  }

  resolveUnmapped(from, request) {
    const saved = this.moduleNameMapper;
    this.moduleNameMapper = [];
    try {
      return this.resolve(from, request);
    } finally {
      this.moduleNameMapper = saved;
    }
  }

  // Vite reads `./a.js` as `./a.ts` when only the TypeScript file exists,
  // as TypeScript's NodeNext resolution writes imports; Jest doesn't.
  tryTypeScriptSibling(target) {
    if (this.config.dialect !== 'vitest') return null;
    const swaps = {'.cjs': ['.cts'], '.js': ['.ts', '.tsx'], '.jsx': ['.tsx'], '.mjs': ['.mts']};
    const extension = path.extname(target);
    for (const replacement of swaps[extension] || []) {
      const candidate = target.slice(0, -extension.length) + replacement;
      if (isFile(candidate)) return candidate;
    }
    return null;
  }

  tryFile(target) {
    if (isFile(target)) return target;
    for (const extension of this.extensions) {
      if (isFile(target + extension)) return target + extension;
    }
    return null;
  }

  tryDirectory(target) {
    if (!isDirectory(target)) return null;
    const manifest = path.join(target, 'package.json');
    if (isFile(manifest)) {
      try {
        const main = JSON.parse(fs.readFileSync(manifest, 'utf8')).main;
        if (typeof main === 'string') {
          const resolved = path.resolve(target, main);
          const found = this.tryFile(resolved) || this.tryIndex(resolved);
          if (found) return found;
        }
      } catch {}
    }
    return this.tryIndex(target);
  }

  tryIndex(target) {
    for (const extension of this.extensions) {
      const index = path.join(target, `index${extension}`);
      if (isFile(index)) return index;
    }
    return null;
  }

  // A bare specifier: Node's own resolver, which reads package exports with
  // the `require` and `node` conditions as Jest's does, then the module file
  // extensions Jest adds on top of Node's.
  resolvePackage(dir, request) {
    try {
      return requireIn(dir).resolve(request);
    } catch {}
    for (let current = dir; ; current = path.dirname(current)) {
      const base = path.join(current, 'node_modules', request);
      const found = this.tryFile(base) || this.tryDirectory(base);
      if (found) return found;
      if (path.dirname(current) === current) return null;
    }
  }

  // jest-resolve's getModulePath: where a virtual mock of `request` lives.
  getModulePath(from, request) {
    if (request[0] !== '.' || path.isAbsolute(request)) return request;
    return path.normalize(`${path.dirname(from)}/${request}`);
  }

  lookupManualMock(name) {
    const mock = this.manualMocks.get(name) || this.manualMocks.get(`${name}/index`);
    if (mock) return mock;
    if (!isBuiltin(name)) return null;
    const normalized = normalizeCore(name);
    return normalized === name ? null : this.manualMocks.get(normalized) || null;
  }

  // jest-resolve's getMockModule: a root manual mock for the name, or for
  // what moduleNameMapper maps it to.
  getMockModule(from, name) {
    const mock = this.lookupManualMock(name);
    if (mock) return mock;
    const stub = this.resolveStub(from, name);
    return stub ? this.lookupManualMock(stub) : null;
  }

  // The manual mock `jest.mock(name)` uses: a root mock by name, or a
  // `__mocks__` folder beside the real file.
  findManualMock(from, name) {
    if (isBuiltin(name)) return this.getMockModule(from, normalizeCore(name));
    const rootMock = this.getMockModule(from, name);
    if (rootMock) return rootMock;
    const modulePath = this.resolve(from, name);
    const sibling = path.join(path.dirname(modulePath), '__mocks__', path.basename(modulePath));
    return fs.existsSync(sibling) ? sibling : null;
  }

  // jest-resolve's getModuleID: what explicit mocks and factories are keyed
  // by, so two spellings of one module share a mock.
  getModuleId(from, name = '') {
    const isVirtual = this.virtualMocks.size > 0 && this.virtualMocks.get(this.getModulePath(from, name)) === true;
    const key = `${from}\0${name}\0${isVirtual}`;
    const cached = this.moduleIdCache.get(key);
    if (cached) return cached;
    const core = isBuiltin(name);
    let absolutePath;
    if (core) {
      absolutePath = normalizeCore(name);
    } else if (this.getMockModule(from, name)) {
      absolutePath = null;
    } else {
      const virtualPath = this.getModulePath(from, name);
      absolutePath = this.virtualMocks.get(virtualPath) ? virtualPath : name ? this.resolve(from, name) : from;
    }
    const mockPath = core ? null : this.getMockModule(from, name);
    const sep = path.delimiter;
    const id = `${core ? 'node' : 'user'}${sep}${absolutePath ? absolutePath + sep : ''}${mockPath ? mockPath + sep : ''}${CJS_OPTIONS}${sep}`;
    this.moduleIdCache.set(key, id);
    return id;
  }

  // --- the mock decision (MockState.decideSync) --------------------------------

  shouldMock(from, name) {
    if (!this.automock && this.explicitMock.size === 0) return {moduleID: '', shouldMock: false};
    const moduleID = this.getModuleId(from, name);
    const explicit = this.explicitMock.get(moduleID);
    if (explicit !== undefined) return {moduleID, shouldMock: explicit};
    if (!this.automock || isBuiltin(name)) return {moduleID, shouldMock: false};
    const key = `${from}\0${moduleID}`;
    if (this.unmockTransitiveCache.get(key)) return {moduleID, shouldMock: false};
    const cached = this.shouldMockCache.get(moduleID);
    if (cached !== undefined) return {moduleID, shouldMock: cached};
    let modulePath;
    try {
      modulePath = this.resolve(from, name);
    } catch (error) {
      if (this.getMockModule(from, name)) {
        this.shouldMockCache.set(moduleID, true);
        return {moduleID, shouldMock: true};
      }
      throw error;
    }
    if (this.unmockList?.test(modulePath)) {
      this.shouldMockCache.set(moduleID, false);
      return {moduleID, shouldMock: false};
    }
    const currentModuleID = this.getModuleId(from);
    if (
      this.transitiveShouldMock.get(currentModuleID) === false ||
      (from.includes(NODE_MODULES) &&
        modulePath.includes(NODE_MODULES) &&
        ((this.unmockList && this.unmockList.test(from)) || this.explicitMock.get(currentModuleID) === false))
    ) {
      this.transitiveShouldMock.set(moduleID, false);
      this.unmockTransitiveCache.set(key, true);
      return {moduleID, shouldMock: false};
    }
    this.shouldMockCache.set(moduleID, true);
    return {moduleID, shouldMock: true};
  }

  setMock(from, name, factory, options) {
    if (options?.virtual) this.virtualMocks.set(this.getModulePath(from, name), true);
    let moduleID;
    try {
      moduleID = this.getModuleId(from, name);
    } catch (error) {
      if (error.code === 'MODULE_NOT_FOUND') {
        error.message +=
          '\n\nTo mock a module that does not exist on disk, pass `{virtual: true}` in the mock options. See https://jestjs.io/docs/jest-object#jestmockmodulename-factory-options';
      }
      throw error;
    }
    this.explicitMock.set(moduleID, true);
    this.factories.set(moduleID, factory);
  }

  markExplicitMock(from, name) {
    this.explicitMock.set(this.getModuleId(from, name), true);
  }

  unmock(from, name) {
    this.explicitMock.set(this.getModuleId(from, name), false);
  }

  deepUnmock(from, name) {
    const moduleID = this.getModuleId(from, name);
    this.explicitMock.set(moduleID, false);
    this.transitiveShouldMock.set(moduleID, false);
  }

  enableAutomock() {
    this.automock = true;
  }

  disableAutomock() {
    this.automock = false;
  }

  addOnGenerateMock(callback) {
    this.onGenerateMockCallbacks.add(callback);
  }

  // --- loading -----------------------------------------------------------------

  // What `require` in a test file or module does.
  requireModuleOrMock(from, name) {
    if (name === '@jest/globals') return this.jestGlobals(from);
    if (name === 'vitest' && this.config.dialect === 'vitest') return this.vitestFor(from);
    const {shouldMock, moduleID} = this.shouldMock(from, name);
    if (shouldMock) return this.requireMockWithId(from, name, moduleID);
    return this.requireModule(from, name);
  }

  requireActual(from, name) {
    return this.requireModule(from, name, true);
  }

  requireModule(from, name, isRequireActual = false) {
    if (name && isBuiltin(name)) return this.requireBuiltin(name);
    let modulePath;
    // A root manual mock named like the request stands in for it with no
    // jest.mock call ("some old tests rely on this", says Jest's source).
    // Vitest uses one only when vi.mock asks for it.
    if (name && !isRequireActual && this.config.dialect !== 'vitest') {
      const manualMock = this.getMockModule(from, name);
      if (
        manualMock &&
        manualMock !== this.currentlyExecuting &&
        this.explicitMock.get(this.getModuleId(from, name)) !== false
      ) {
        modulePath = manualMock;
      }
    }
    if (!modulePath) modulePath = name ? this.resolve(from, name) : from;
    return this.loadInto(this.modules(), modulePath, from);
  }

  requireMock(from, name) {
    return this.requireMockWithId(from, name, this.getModuleId(from, name));
  }

  requireMockWithId(from, name, moduleID) {
    const registry = this.mocks();
    if (registry.has(moduleID)) return registry.get(moduleID);
    const factory = this.factories.get(moduleID);
    if (factory) {
      const value = factory();
      registry.set(moduleID, value);
      return value;
    }
    const manualMockPath = this.findManualMock(from, name);
    if (manualMockPath) {
      const module = this.createModule(manualMockPath, from);
      this.executeFile(module);
      registry.set(moduleID, module.exports);
    } else {
      registry.set(moduleID, this.generateMock(from, name));
    }
    return registry.get(moduleID);
  }

  // jest.createMockFromModule and automocking: the real module's shape,
  // made of mock functions.
  generateMock(from, name) {
    const modulePath = this.resolveStub(from, name) || this.resolve(from, name);
    if (!this.mockMetadata.has(modulePath)) {
      // Seeded first, so a module that requires itself through the cycle
      // finds something.
      this.mockMetadata.set(modulePath, this.moduleMocker.getMetadata({}) || {});
      const exports = this.withScratchRegistries(() => this.requireModule(from, name));
      const metadata = this.moduleMocker.getMetadata(exports);
      if (metadata == null) {
        throw new Error(
          `Failed to get mock metadata: ${modulePath}\n\nSee: https://jestjs.io/docs/manual-mocks#content`,
        );
      }
      this.mockMetadata.set(modulePath, metadata);
    }
    let mock = this.moduleMocker.generateFromMetadata(this.mockMetadata.get(modulePath));
    for (const callback of this.onGenerateMockCallbacks) mock = callback(modulePath, mock);
    return mock;
  }

  requireBuiltin(name) {
    const bare = normalizeCore(name);
    if (bare === 'process') return this.global.process;
    return require(name);
  }

  createModule(filename, from) {
    const parent = from ? this.modules().get(from) : null;
    const module = {
      children: [],
      exports: {},
      filename,
      id: filename,
      loaded: false,
      parent: parent || null,
      path: path.dirname(filename),
      paths: nodeModulesPaths(path.dirname(filename)),
    };
    Object.defineProperty(module, 'require', {value: request => this.requireModuleOrMock(filename, request)});
    return module;
  }

  loadInto(registry, filename, from) {
    const cached = registry.get(filename);
    if (cached) return cached.exports;
    const module = this.createModule(filename, from);
    const parent = from ? registry.get(from) : null;
    if (parent && !parent.children.includes(module)) parent.children.push(module);
    if (!this.mainModule) this.mainModule = module;
    registry.set(filename, module);
    try {
      this.executeFile(module);
    } catch (error) {
      registry.delete(filename);
      if (parent) parent.children.splice(parent.children.indexOf(module), 1);
      throw error;
    }
    module.loaded = true;
    return module.exports;
  }

  executeFile(module) {
    const {filename} = module;
    const extension = path.extname(filename);
    if (extension === '.json') {
      module.exports = this.global.JSON.parse(stripBOM(fs.readFileSync(filename, 'utf8')));
      return;
    }
    if (extension === '.node') {
      module.exports = require(filename);
      return;
    }
    let code = stripBOM(fs.readFileSync(filename, 'utf8'));
    if (this.transform) code = this.transform(filename, code);
    if (code.startsWith('#!')) code = `//${code.slice(2)}`;
    let script;
    let isAsync = false;
    try {
      script = compile(filename, code);
    } catch (error) {
      if (filename !== this.asyncEntry || !awaitsAtTopLevel(error)) throw error;
      script = compile(filename, code, true);
      isAsync = true;
    }
    const wrapper = script.runInContext(this.context)['Object.<anonymous>'];
    const require = this.createRequire(module);
    const saved = this.currentlyExecuting;
    this.currentlyExecuting = filename;
    try {
      const evaluation = wrapper.call(module.exports, module, module.exports, require, path.dirname(filename), filename, this.createJestObject(filename));
      if (isAsync) module.evaluation = evaluation;
    } finally {
      this.currentlyExecuting = saved;
    }
  }

  importMeta(filename, require) {
    const meta = new this.global.Object();
    meta.dirname = path.dirname(filename);
    meta.filename = filename;
    meta.resolve = request => pathToFileURL(require.resolve(request)).href;
    meta.url = pathToFileURL(filename).href;
    if (this.config.dialect === 'vitest') meta.env = this.importMetaEnv();
    return meta;
  }

  // Vitest's import.meta.env: the test's process.env, with DEV, PROD and SSR
  // read as booleans (createImportMetaEnvProxy in vitest's worker). Vitest
  // puts Vite's values in process.env first, where they aren't set already.
  importMetaEnv() {
    if (this.metaEnv) return this.metaEnv;
    const env = this.global.process.env;
    for (const [name, value] of Object.entries({BASE_URL: '/', DEV: '1', MODE: 'test', PROD: '', SSR: '1'})) {
      if (env[name] === undefined) env[name] = value;
    }
    const booleans = ['DEV', 'PROD', 'SSR'];
    this.metaEnv = new Proxy(env, {
      get: (_target, key) => {
        if (typeof key !== 'string') return undefined;
        return booleans.includes(key) ? Boolean(env[key]) : env[key];
      },
      set: (_target, key, value) => {
        if (typeof key === 'string') env[key] = booleans.includes(key) ? (value ? '1' : '') : value;
        return true;
      },
    });
    return this.metaEnv;
  }

  // A test or setup file. In Vitest's dialect it may await at its top level,
  // and is then done when `evaluation` settles. Otherwise nothing is awaited:
  // a test defined in a promise callback the file started has to find the
  // run begun, as in Jest.
  requireEntry(filename) {
    this.asyncEntry = this.config.dialect === 'vitest' ? filename : null;
    try {
      const exports = this.requireModule(filename);
      return {evaluation: this.modules().get(filename)?.evaluation, exports};
    } finally {
      this.asyncEntry = null;
    }
  }

  createRequire(module) {
    const from = module.filename;
    const require = request => this.requireModuleOrMock(from, request);
    // What `import.meta` becomes (transform.rs): one object per module.
    let meta;
    Object.defineProperty(require, IMPORT_META, {get: () => (meta ??= this.importMeta(from, require))});
    require.resolve = (request, options) => {
      if (request == null) {
        throw new Error('The first argument to require.resolve must be a string. Received null or undefined.');
      }
      if (options && Array.isArray(options.paths)) {
        for (const base of options.paths) {
          try {
            return this.resolve(path.join(path.resolve(path.dirname(from), base), 'index.js'), request);
          } catch {}
        }
        throw new ModuleNotFoundError(
          `Cannot resolve module '${request}' from paths ['${options.paths.join("', '")}'] from ${from}`,
          request,
        );
      }
      try {
        return this.resolve(from, request);
      } catch (error) {
        const mock = this.getMockModule(from, request);
        if (mock) return mock;
        throw error;
      }
    };
    require.resolve.paths = request => {
      if (request[0] === '.') return [path.dirname(from)];
      return isBuiltin(request) ? null : nodeModulesPaths(path.dirname(from));
    };
    Object.defineProperty(require, 'main', {enumerable: true, get: () => this.mainModule});
    require.extensions = Object.create(null);
    Object.defineProperty(require, 'cache', {
      get: () => {
        const cache = Object.create(null);
        for (const [key, value] of this.modules()) cache[key] = value;
        return cache;
      },
    });
    return require;
  }
}

function nodeModulesPaths(dir) {
  const paths = [];
  for (let current = dir; ; current = path.dirname(current)) {
    if (path.basename(current) !== 'node_modules') paths.push(path.join(current, 'node_modules'));
    if (path.dirname(current) === current) return paths;
  }
}

module.exports = {Loader, isBuiltin};
