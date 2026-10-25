// CommonJS, as Node's lib/internal/modules/cjs/loader.js runs it: each file is
// compiled as the body of
//   function (exports, require, module, __filename, __dirname)
// (with V8's CompileFunction, so line 1 column 1 of the file is line 1
// column 1 in stack traces), and called with `this` set to module.exports,
// which is why a top-level frame reads `Object.<anonymous>`.
(function (internals, ops, primordials) {
  "use strict";

  const { ObjectDefineProperty } = primordials;
  const requireBuiltin = internals.requireBuiltin;
  const errors = () => requireBuiltin("internal/errors").codes;

  // Built-in modules, by the name require() takes, with or without `node:`.
  const builtins = new Map();
  function defineBuiltin(name, load) {
    let cached;
    builtins.set(name, () => (cached === undefined ? (cached = load()) : cached));
  }
  // Loading events makes process an EventEmitter (process.js), so that
  // `process instanceof EventEmitter` holds for anyone who can ask.
  defineBuiltin("events", () => {
    internals.processEmitter.become();
    return requireBuiltin("events");
  });
  defineBuiltin("path", () => requireBuiltin("path"));
  defineBuiltin("path/posix", () => requireBuiltin("path").posix);
  defineBuiltin("path/win32", () => requireBuiltin("path").win32);
  defineBuiltin("util", () => requireBuiltin("util"));
  defineBuiltin("util/types", () => requireBuiltin("util/types"));
  defineBuiltin("buffer", () => requireBuiltin("buffer"));
  defineBuiltin("console", () => internals.console);
  defineBuiltin("process", () => internals.process);
  defineBuiltin("timers", () => {
    const t = internals.timers;
    return {
      setTimeout: t.setTimeout,
      setInterval: t.setInterval,
      setImmediate: t.setImmediate,
      clearTimeout: t.clearTimeout,
      clearInterval: t.clearInterval,
      clearImmediate: t.clearImmediate,
    };
  });

  function builtinFor(id) {
    const name = id.startsWith("node:") ? id.slice(5) : id;
    return builtins.get(name);
  }

  function dirname(path) {
    const index = path.lastIndexOf("/");
    if (index === -1) return ".";
    if (index === 0) return "/";
    return path.slice(0, index);
  }

  function Module(id = "", parent) {
    this.id = id;
    this.path = dirname(id);
    this.exports = {};
    this.filename = null;
    this.loaded = false;
    this.children = [];
    this.paths = [];
    if (parent) parent.children.push(this);
  }
  Module._cache = { __proto__: null };
  Module.wrapper = [
    "(function (exports, require, module, __filename, __dirname) { ",
    "\n});",
  ];

  function moduleNotFound(request, parent) {
    const requireStack = [];
    for (let cursor = parent; cursor; cursor = cursor.parent) {
      if (cursor.filename) requireStack.push(cursor.filename);
    }
    let message = `Cannot find module '${request}'`;
    if (requireStack.length > 0) {
      message += `\nRequire stack:\n- ${requireStack.join("\n- ")}`;
    }
    const error = new Error(message);
    error.code = "MODULE_NOT_FOUND";
    error.requireStack = requireStack;
    return error;
  }

  function makeRequire(module) {
    const require = function require(id) {
      if (typeof id !== "string") {
        throw new (errors().ERR_INVALID_ARG_TYPE)("id", "string", id);
      }
      if (id === "") {
        throw new (errors().ERR_INVALID_ARG_VALUE)("id", id, "must be a non-empty string");
      }
      const builtin = builtinFor(id);
      if (builtin !== undefined) return builtin();
      throw moduleNotFound(id, module);
    };
    require.main = mainModule;
    require.cache = Module._cache;
    require.resolve = function resolve(request) {
      if (builtinFor(request) !== undefined) return request;
      throw moduleNotFound(request, module);
    };
    return require;
  }

  let mainModule;

  Module.prototype._compile = function _compile(content, filename) {
    // A shebang line is not JavaScript; V8 accepts one only at the start of
    // a script, and this is a function body.
    if (content.startsWith("#!")) {
      const end = content.indexOf("\n");
      content = end === -1 ? "" : `//${content.slice(2, end)}${content.slice(end)}`;
    }
    const compiled = ops.compile_function(content, filename, [
      "exports",
      "require",
      "module",
      "__filename",
      "__dirname",
    ]);
    const require = makeRequire(this);
    return compiled.call(this.exports, this.exports, require, this, filename, dirname(filename));
  };

  function runMain(filename) {
    const module = new Module(".", null);
    module.id = ".";
    module.filename = filename;
    module.path = dirname(filename);
    mainModule = module;
    Module._cache[filename] = module;
    module._compile(ops.read_file(filename), filename);
    module.loaded = true;
  }

  ObjectDefineProperty(Module, "Module", { value: Module });
  defineBuiltin("module", () => Module);
  Module.builtinModules = [...builtins.keys()];

  internals.loader = { Module, runMain, builtinFor };
})
