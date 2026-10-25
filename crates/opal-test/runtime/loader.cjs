// The module registry a test file loads through: CommonJS, evaluated inside
// the file's context, after jest-runtime (jest/packages/jest-runtime/src/
// index.ts).
//
// Every test file gets its own registry, so a module's state never survives
// from one file to the next, which is Jest's isolation. Node's built-in
// modules are the exception, as in Jest: they come from the host, except
// `process`, which is the context's own copy.
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

const builtinModules = new Set(Module.builtinModules);
const isBuiltin = request =>
  request.startsWith('node:') || builtinModules.has(request) || builtinModules.has(request.split('/')[0]);

const scriptCache = new Map();

function compile(filename, code) {
  const key = `${filename}\0${code}`;
  let script = scriptCache.get(key);
  if (!script) {
    script = new vm.Script(
      `({"Object.<anonymous>":function(module,exports,require,__dirname,__filename,jest){${code}\n}});`,
      {filename},
    );
    scriptCache.set(key, script);
  }
  return script;
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
  constructor({config, context, global, testPath, transform}) {
    this.config = config;
    this.context = context;
    this.global = global;
    this.testPath = testPath;
    this.transform = transform;
    this.extensions = config.moduleFileExtensions.map(extension => `.${extension}`);
    this.registry = new Map();
    this.mainModule = null;
    this.createJestObject = () => undefined;
    this.moduleNameMapper = Object.entries(config.moduleNameMapper || {}).map(([pattern, target]) => [
      new RegExp(pattern),
      target,
    ]);
  }

  // --- resolution ------------------------------------------------------------

  resolve(from, request) {
    if (isBuiltin(request)) return request;
    const mapped = this.mapModuleName(from, request);
    if (mapped) return mapped;
    const dir = path.dirname(from);
    if (request.startsWith('./') || request.startsWith('../') || request === '.' || request === '..' || path.isAbsolute(request)) {
      const target = path.resolve(dir, request);
      const found = this.tryFile(target) || this.tryDirectory(target);
      if (found) return found;
    } else {
      const found = this.resolvePackage(dir, request);
      if (found) return found;
    }
    const relative = path.relative(path.dirname(this.testPath), from) || path.basename(from);
    throw new ModuleNotFoundError(
      `Cannot find module '${request}' from '${relative}'`,
      request,
    );
  }

  mapModuleName(from, request) {
    for (const [regex, target] of this.moduleNameMapper) {
      const match = request.match(regex);
      if (!match) continue;
      const targets = Array.isArray(target) ? target : [target];
      for (const candidate of targets) {
        const replaced = candidate.replaceAll(/\$(\d+)/g, (_, index) => match[Number(index)] ?? '');
        try {
          return this.resolve(from, replaced);
        } catch {}
      }
      throw new ModuleNotFoundError(
        `Configuration error:\n\nCould not locate module ${request} mapped as:\n${targets.join(', ')}.`,
        request,
      );
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
  // the `require` and `node` conditions as Jest's does, then the module
  // file extensions Jest adds on top of Node's.
  resolvePackage(dir, request) {
    try {
      return require.resolve(request, {paths: [dir]});
    } catch {}
    for (let current = dir; ; current = path.dirname(current)) {
      const base = path.join(current, 'node_modules', request);
      const found = this.tryFile(base) || this.tryDirectory(base);
      if (found) return found;
      if (path.dirname(current) === current) return null;
    }
  }

  // --- loading -----------------------------------------------------------------

  requireModule(from, request) {
    const filename = this.resolve(from, request);
    if (isBuiltin(filename)) return this.requireBuiltin(filename);
    return this.load(filename, from);
  }

  requireBuiltin(name) {
    const bare = name.startsWith('node:') ? name.slice(5) : name;
    if (bare === 'process') return this.global.process;
    return require(name);
  }

  load(filename, from) {
    const cached = this.registry.get(filename);
    if (cached) return cached.exports;
    const parent = from ? this.registry.get(from) : null;
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
    Object.defineProperty(module, 'require', {value: request => this.requireModule(filename, request)});
    if (parent) parent.children.push(module);
    if (!this.mainModule) this.mainModule = module;
    this.registry.set(filename, module);
    try {
      this.execute(module);
    } catch (error) {
      this.registry.delete(filename);
      throw error;
    }
    module.loaded = true;
    return module.exports;
  }

  execute(module) {
    const {filename} = module;
    const extension = path.extname(filename);
    if (extension === '.json') {
      const text = stripBOM(fs.readFileSync(filename, 'utf8'));
      module.exports = this.global.JSON.parse(text);
      return;
    }
    if (extension === '.node') {
      module.exports = require(filename);
      return;
    }
    let code = stripBOM(fs.readFileSync(filename, 'utf8'));
    if (this.transform) code = this.transform(filename, code);
    if (code.startsWith('#!')) code = `//${code.slice(2)}`;
    const wrapper = compile(filename, code).runInContext(this.context)['Object.<anonymous>'];
    const require = this.createRequire(module);
    const dirname = path.dirname(filename);
    wrapper.call(module.exports, module, module.exports, require, dirname, filename, this.createJestObject(filename));
  }

  createRequire(module) {
    const from = module.filename;
    const require = request => this.requireModule(from, request);
    require.resolve = (request, options) => {
      if (options && Array.isArray(options.paths)) {
        for (const base of options.paths) {
          try {
            return this.resolve(path.join(base, 'index.js'), request);
          } catch {}
        }
      }
      return this.resolve(from, request);
    };
    require.resolve.paths = request => (isBuiltin(request) ? null : nodeModulesPaths(path.dirname(from)));
    require.main = this.mainModule;
    require.extensions = Object.create(null);
    Object.defineProperty(require, 'cache', {
      get: () => {
        const cache = Object.create(null);
        for (const [key, value] of this.registry) cache[key] = value;
        return cache;
      },
    });
    return require;
  }

  requireActual(from, request) {
    return this.requireModule(from, request);
  }

  resetModules() {
    this.registry.clear();
  }

  // jest.isolateModules: what `fn` requires comes from a registry of its
  // own, thrown away afterwards.
  isolateModules(fn) {
    if (this.isolating) {
      throw new Error('isolateModules cannot be nested inside another isolateModules or isolateModulesAsync.');
    }
    const saved = this.registry;
    this.registry = new Map();
    this.isolating = true;
    try {
      fn();
    } finally {
      this.registry = saved;
      this.isolating = false;
    }
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
