//! The JavaScript the bundle carries: namespace objects, lazy modules,
//! CommonJS loading, and the small helpers rewritten code calls.
//!
//! Every helper is written against builtins it captures before any user code
//! runs, so a program that replaces `Object.defineProperty` or
//! `Array.prototype[Symbol.iterator]` cannot change how its own modules
//! link. `$$` is replaced by the bundle's prefix, which no identifier in the
//! program starts with.

/// Captured builtins. Always emitted first.
pub const CAPTURE: &str = r#"const $$Object = Object, $$Reflect = Reflect, $$Proxy = Proxy, $$Promise = Promise, $$TypeError = TypeError, $$URL = URL;
const $$create = $$Object.create, $$define = $$Object.defineProperty, $$getOwn = $$Object.getOwnPropertyDescriptor, $$hasOwn = $$Object.hasOwn, $$is = $$Object.is, $$apply = $$Reflect.apply, $$then = $$Promise.prototype.then, $$tag = Symbol.toStringTag, $$JSONparse = JSON.parse;
"#;

/// A module namespace exotic object (ECMA-262 §10.4.6): `keys` sorted by
/// code unit, `getters` a null-prototype object of functions reading each
/// binding, so a binding in its TDZ throws as §10.4.6.8 requires.
pub const NAMESPACE: &str = r#"function $$ns(keys, getters) {
  const target = $$create(null);
  const ownKeys = [];
  for (let i = 0; i < keys.length; i++) {
    $$define(target, keys[i], { __proto__: null, value: undefined, writable: true, enumerable: true, configurable: false });
    ownKeys[i] = keys[i];
  }
  ownKeys[keys.length] = $$tag;
  $$define(target, $$tag, { __proto__: null, value: "Module", writable: false, enumerable: false, configurable: false });
  $$Object.preventExtensions(target);
  const has = (key) => typeof key === "string" && $$hasOwn(getters, key);
  const describe = (key) => has(key) ? { __proto__: null, value: getters[key](), writable: true, enumerable: true, configurable: false } : undefined;
  return new $$Proxy(target, {
    __proto__: null,
    get: (t, key) => typeof key === "string" ? (has(key) ? getters[key]() : undefined) : $$Reflect.get(t, key),
    has: (t, key) => typeof key === "string" ? has(key) : $$Reflect.has(t, key),
    getOwnPropertyDescriptor: (t, key) => typeof key === "string" ? describe(key) : $$Reflect.getOwnPropertyDescriptor(t, key),
    defineProperty(t, key, desc) {
      if (typeof key !== "string") return $$Reflect.defineProperty(t, key, desc);
      const current = describe(key);
      if (current === undefined) return false;
      if ($$hasOwn(desc, "configurable") && desc.configurable) return false;
      if ($$hasOwn(desc, "enumerable") && !desc.enumerable) return false;
      if ($$hasOwn(desc, "get") || $$hasOwn(desc, "set")) return false;
      if ($$hasOwn(desc, "writable") && !desc.writable) return false;
      if ($$hasOwn(desc, "value")) return $$is(desc.value, current.value);
      return true;
    },
    deleteProperty: (t, key) => typeof key === "string" ? !has(key) : $$Reflect.deleteProperty(t, key),
    ownKeys: () => { const copy = []; for (let i = 0; i < ownKeys.length; i++) copy[i] = ownKeys[i]; return copy; },
    set: () => false,
    getPrototypeOf: () => null,
    setPrototypeOf: (t, proto) => proto === null,
    isExtensible: () => false,
    preventExtensions: () => true,
  });
}
"#;

/// A module reached only through `import()`. `body` runs once; an error is
/// recorded on every module on the evaluation stack, which is what
/// InnerModuleEvaluation does (§16.2.1.6.1.3.1), and a later import of any
/// of them throws it again. A module completes only when the first module
/// of its cycle entered in this evaluation does.
pub const LAZY: &str = r#"const $$stack = [];
function $$lazy(cycle, body) {
  const record = { __proto__: null, state: 0, error: undefined, cycle };
  return () => {
    if (record.state === 3) throw record.error;
    if (record.state !== 0) return;
    record.state = 1;
    const depth = $$stack.length;
    $$stack[depth] = record;
    try {
      body();
    } catch (error) {
      for (let i = 0; i < $$stack.length; i++) { $$stack[i].state = 3; $$stack[i].error = error; }
      $$stack.length = 0;
      throw error;
    }
    record.state = 2;
    for (let i = 0; i < depth; i++) if ($$stack[i].cycle === cycle) return;
    for (let i = depth; i < $$stack.length; i++) $$stack[i].state = 4;
    $$stack.length = depth;
  };
}
"#;

/// `import()` of a bundled module: settles after the current synchronous
/// code, with the namespace `thunk` returns or the error it throws.
pub const IMPORT: &str = r#"function $$import(thunk) {
  return $$apply($$then, new $$Promise((resolve) => resolve()), [thunk]);
}
"#;

pub const SET_NAME: &str = r#"function $$setName(f, name) {
  $$define(f, "name", { __proto__: null, value: name });
}
function $$setClassName(c, name) {
  const d = $$getOwn(c, "name");
  if (d !== undefined && $$hasOwn(d, "value") && d.writable === false) $$define(c, "name", { __proto__: null, value: name });
}
"#;

/// Writing to an import binding (§6.2.5.6 PutValue on an immutable binding).
pub const CONST_ASSIGN: &str = r#"function $$constAssign() {
  throw new $$TypeError("Assignment to constant variable.");
}
function $$toNumeric(v) {
  v++;
}
"#;

/// Paths of the original files, from the bundle's own location, for
/// `--target node`.
pub const PATHS_NODE: &str = r#"const $$base = import.meta.url;
function $$abs(rel) {
  return $$fileURLToPath(new $$URL(rel, $$base));
}
function $$fileURL(rel) {
  return $$pathToFileURL($$abs(rel)).href;
}
"#;

pub const PATHS_BROWSER: &str = r#"const $$base = import.meta.url;
function $$fileURL(rel) {
  return new $$URL(rel, $$base).href;
}
"#;

/// CommonJS for `--target node`: Node's loader (`Module._load`,
/// `Module.prototype._compile`, lib/internal/modules/cjs/loader.js at
/// v24.19.0) for the modules in the bundle, and Node's own `require` for
/// everything else.
pub const CJS_NODE: &str = r#"const $$byPath = { __proto__: null };
let $$main;
const $$root = $$abs("%ROOT%");
const $$wrapper = ["exports", "require", "module", "__filename", "__dirname"];
class $$Module {
  constructor(id, filename, parent) {
    this.id = id;
    this.path = $$dirname(filename);
    this.exports = {};
    this.filename = filename;
    this.loaded = false;
    this.children = [];
    this.paths = $$NodeModule._nodeModulePaths(this.path);
    $$define(this, $$parentKey, { __proto__: null, value: parent, writable: true, configurable: true });
    if (parent !== undefined) $$addChild(parent, this);
  }
  get parent() {
    return this[$$parentKey];
  }
  require(id) {
    return this[$$requireKey](id);
  }
}
const $$parentKey = Symbol("parent"), $$requireKey = Symbol("require");
$$setName($$Module, "Module");
function $$addChild(parent, child) {
  const children = parent.children;
  for (let i = 0; i < children.length; i++) if (children[i] === child) return;
  children[children.length] = child;
}
function $$cjs(rel, key, deps, factory, sloppy) {
  return ($$byPath[key] = { __proto__: null, kind: 0, rel, deps, table: undefined, factory, sloppy, module: undefined, snapshot: undefined, ns: undefined });
}
function $$json(rel, key, text) {
  return ($$byPath[key] = { __proto__: null, kind: 1, rel, text, module: undefined, snapshot: undefined, ns: undefined });
}
function $$load(record, parent, isMain) {
  let module = record.module;
  if (module !== undefined) {
    if (parent !== undefined) $$addChild(parent, module);
    return module.exports;
  }
  const filename = $$abs(record.rel);
  module = new $$Module(isMain ? "." : filename, filename, parent);
  record.module = module;
  if (isMain) {
    process.mainModule = module;
    $$main = module;
  }
  let threw = true;
  try {
    if (record.kind === 1) {
      try {
        module.exports = $$JSONparse(record.text);
      } catch (error) {
        error.message = filename + ": " + error.message;
        throw error;
      }
    } else {
      const require = $$makeRequire(record, module);
      $$define(module, $$requireKey, { __proto__: null, value: require });
      let factory = record.factory;
      if (factory === undefined) factory = record.factory = $$compileFunction(record.sloppy, $$wrapper, { filename });
      $$apply(factory, module.exports, [module.exports, require, module, filename, $$dirname(filename)]);
    }
    threw = false;
  } finally {
    if (threw) {
      record.module = undefined;
      if (parent !== undefined) {
        const children = parent.children;
        for (let i = 0; i < children.length; i++) if (children[i] === module) { children.splice(i, 1); break; }
      }
    } else {
      module.loaded = true;
    }
  }
  return module.exports;
}
function $$makeRequire(record, module) {
  let real;
  const realRequire = () => (real === undefined ? (real = $$createRequireReal(module.filename)) : real);
  function require(id) {
    if (typeof id === "string") {
      const table = record.table === undefined ? (record.table = record.deps()) : record.table;
      const target = table[id];
      if (target !== undefined && target !== 0) return $$load(target, module, false);
      if (target === 0) return realRequire()(id);
    }
    const filename = realRequire().resolve(id);
    const found = $$byPath[$$relative($$root, filename).split($$sep).join("/")];
    return found !== undefined ? $$load(found, module, false) : realRequire()(id);
  }
  require.resolve = function resolve(request, options) {
    return realRequire().resolve(request, options);
  };
  require.resolve.paths = function paths(request) {
    return realRequire().resolve.paths(request);
  };
  require.main = $$main;
  return require;
}
"#;

/// `createRequire` from `node:module`, for `--target node`: Node's
/// resolver from the given location, and the bundled instance whenever the
/// file it finds is in the bundle, as the original program's would be.
pub const CREATE_REQUIRE: &str = r#"function $$createRequire(path) {
  const real = $$createRequireReal(path);
  function require(id) {
    const filename = real.resolve(id);
    const found = typeof $$byPath === "undefined" ? undefined : $$byPath[$$relative($$root, filename).split($$sep).join("/")];
    return found !== undefined ? $$load(found, undefined, false) : real(id);
  }
  require.resolve = real.resolve;
  require.main = real.main;
  require.cache = real.cache;
  require.extensions = real.extensions;
  return require;
}
"#;

/// CommonJS for `--target browser`: the same loader without Node. A
/// specifier the bundle does not hold throws `MODULE_NOT_FOUND`, as Node's
/// require does for a module that is not there.
pub const CJS_BROWSER: &str = r#"const $$byPath = { __proto__: null };
let $$main;
const $$wrapper = ["exports", "require", "module", "__filename", "__dirname"];
class $$Module {
  constructor(id, filename, parent) {
    this.id = id;
    this.path = filename.slice(0, filename.lastIndexOf("/")) || "/";
    this.exports = {};
    this.filename = filename;
    this.loaded = false;
    this.children = [];
    this.paths = [];
    $$define(this, $$parentKey, { __proto__: null, value: parent, writable: true, configurable: true });
    if (parent !== undefined) $$addChild(parent, this);
  }
  get parent() {
    return this[$$parentKey];
  }
  require(id) {
    return this[$$requireKey](id);
  }
}
const $$parentKey = Symbol("parent"), $$requireKey = Symbol("require");
$$setName($$Module, "Module");
function $$addChild(parent, child) {
  const children = parent.children;
  for (let i = 0; i < children.length; i++) if (children[i] === child) return;
  children[children.length] = child;
}
function $$cjs(rel, key, deps, factory, sloppy) {
  return ($$byPath[key] = { __proto__: null, kind: 0, rel: "/" + key, deps, table: undefined, factory, sloppy, module: undefined, snapshot: undefined, ns: undefined });
}
function $$json(rel, key, text) {
  return ($$byPath[key] = { __proto__: null, kind: 1, rel: "/" + key, text, module: undefined, snapshot: undefined, ns: undefined });
}
function $$load(record, parent, isMain) {
  let module = record.module;
  if (module !== undefined) {
    if (parent !== undefined) $$addChild(parent, module);
    return module.exports;
  }
  const filename = record.rel;
  module = new $$Module(isMain ? "." : filename, filename, parent);
  record.module = module;
  if (isMain) $$main = module;
  let threw = true;
  try {
    if (record.kind === 1) {
      try {
        module.exports = $$JSONparse(record.text);
      } catch (error) {
        error.message = filename + ": " + error.message;
        throw error;
      }
    } else {
      const require = $$makeRequire(record, module);
      $$define(module, $$requireKey, { __proto__: null, value: require });
      let factory = record.factory;
      if (factory === undefined) factory = record.factory = Function(...$$wrapper, record.sloppy);
      $$apply(factory, module.exports, [module.exports, require, module, filename, module.path]);
    }
    threw = false;
  } finally {
    if (threw) {
      record.module = undefined;
      if (parent !== undefined) {
        const children = parent.children;
        for (let i = 0; i < children.length; i++) if (children[i] === module) { children.splice(i, 1); break; }
      }
    } else {
      module.loaded = true;
    }
  }
  return module.exports;
}
function $$makeRequire(record, module) {
  function require(id) {
    if (typeof id === "string") {
      const table = record.table === undefined ? (record.table = record.deps()) : record.table;
      const target = table[id];
      if (target !== undefined && target !== 0) return $$load(target, module, false);
    }
    const error = new Error("Cannot find module '" + id + "'");
    error.code = "MODULE_NOT_FOUND";
    throw error;
  }
  require.resolve = function resolve(request) {
    const error = new Error("Cannot find module '" + request + "'");
    error.code = "MODULE_NOT_FOUND";
    throw error;
  };
  require.main = $$main;
  return require;
}
"#;

/// An ES module importing CommonJS: the module record Node builds for it
/// (translators.js:203-275). The module runs if no `require()` has run it,
/// and each named export is read once, after it ran.
pub const CJS_IMPORT: &str = r#"function $$importCjs(record, names) {
  if (record.snapshot !== undefined) return record.snapshot;
  const exports = $$load(record, undefined, false);
  const snapshot = $$create(null);
  for (let i = 0; i < names.length; i++) {
    const name = names[i];
    if (name === "default" || name === "module.exports") continue;
    let value;
    if ($$hasOwn(exports, name)) {
      try {
        value = exports[name];
      } catch {}
    }
    snapshot[name] = value;
  }
  snapshot["default"] = exports;
  snapshot["module.exports"] = exports;
  return (record.snapshot = snapshot);
}
function $$cjsNs(record, names, sorted) {
  if (record.ns !== undefined) return record.ns;
  const snapshot = $$importCjs(record, names);
  const getters = $$create(null);
  for (let i = 0; i < sorted.length; i++) {
    const value = snapshot[sorted[i]];
    getters[sorted[i]] = () => value;
  }
  return (record.ns = $$ns(sorted, getters));
}
"#;

/// The entry module when it is CommonJS. Node loads it synchronously, so a
/// throw at its top level is an uncaught exception at once: monitors see it,
/// then a handler if there is one, and otherwise the process exits with 1
/// before any pending microtask runs (lib/internal/process/execution.js,
/// createOnGlobalUncaughtException). The bundle is an ES module, where the
/// same throw would become a rejection a microtask later, so the entry's
/// load reports it the way Node would.
pub const CJS_MAIN: &str = r#"function $$runMain(record) {
  try {
    $$load(record, undefined, true);
  } catch (error) {
    process.emit("uncaughtExceptionMonitor", error, "uncaughtException");
    if (process.listenerCount("uncaughtException") > 0) {
      process.emit("uncaughtException", error, "uncaughtException");
      return;
    }
    const text = error !== null && typeof error === "object" && typeof error.stack === "string" ? error.stack : "Uncaught " + String(error);
    process.stderr.write(text + "\n\nNode.js " + process.version + "\n");
    process.exit(1);
  }
}
"#;

/// Helpers the TypeScript transform imports for legacy decorators, which
/// are TypeScript's own (src/compiler/factory/emitHelpers.ts), as
/// @oxc-project/runtime ships them.
pub const VIRTUAL_MODULES: &[(&str, &str)] = &[
    (
        "decorate",
        r#"function __decorate(decorators, target, key, desc) {
  var c = arguments.length, r = c < 3 ? target : desc === null ? (desc = Object.getOwnPropertyDescriptor(target, key)) : desc, d;
  if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
  else for (var i = decorators.length - 1; i >= 0; i--) if ((d = decorators[i])) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
  return c > 3 && r && Object.defineProperty(target, key, r), r;
}
export { __decorate as default };
"#,
    ),
    (
        "decorateMetadata",
        r#"function __decorateMetadata(k, v) {
  if (typeof Reflect === "object" && typeof Reflect.metadata === "function") return Reflect.metadata(k, v);
}
export { __decorateMetadata as default };
"#,
    ),
    (
        "decorateParam",
        r#"function __decorateParam(paramIndex, decorator) {
  return function (target, key) {
    decorator(target, key, paramIndex);
  };
}
export { __decorateParam as default };
"#,
    ),
];

pub fn with_prefix(text: &str, prefix: &str) -> String {
    text.replace("$$", prefix)
}
