// CommonJS, as Node's lib/internal/modules/cjs/loader.js runs it. Each file
// is compiled as the body of
//   function (exports, require, module, __filename, __dirname)
// (with V8's CompileFunction, so line 1 column 1 of the file is line 1
// column 1 in stack traces), and called with `this` set to module.exports,
// which is why a top-level frame reads `Object.<anonymous>`. What a
// specifier names is decided by Node's rules in Rust
// (opal_core::graph::node_resolution, through the resolve op), and every
// failure becomes the error Node throws for it.
//
// A TypeScript file (.ts, .cts) has its types stripped first, as Node does
// by default since v23.6: replaced by whitespace, so lines and columns are
// the source's. Node refuses to strip files under node_modules, and so does
// this. A file that is an ES module (.mjs, .mts, "type": "module", or a
// typeless .js or .ts that only parses as one) is run by esm.js, also when
// it is required.
(function (internals, ops, primordials) {
  "use strict";

  const {
    ArrayPrototypeIncludes,
    ArrayPrototypeJoin,
    ArrayPrototypeMap,
    ArrayPrototypePush,
    ArrayPrototypeSlice,
    ArrayPrototypeSplice,
    ObjectDefineProperty,
    ReflectApply,
    ReflectConstruct,
    RegExpPrototypeExec,
    SafeMap,
    StringPrototypeCharCodeAt,
    StringPrototypeEndsWith,
    StringPrototypeIncludes,
    StringPrototypeIndexOf,
    StringPrototypeLastIndexOf,
    StringPrototypeSlice,
    StringPrototypeStartsWith,
  } = primordials;
  const requireBuiltin = internals.requireBuiltin;
  const errors = () => requireBuiltin("internal/errors").codes;

  // Built-in modules, by the name require() takes, with or without `node:`.
  const builtins = new SafeMap();
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
  defineBuiltin("fs", () => requireBuiltin("fs"));
  defineBuiltin("assert", () => requireBuiltin("assert"));
  defineBuiltin("assert/strict", () => requireBuiltin("assert").strict);
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

  // Node's other built-ins, which require() must not look for on disk.
  const unimplemented = [
    "async_hooks", "child_process", "cluster", "constants", "crypto", "dgram",
    "diagnostics_channel", "dns", "dns/promises", "domain", "fs/promises", "http", "http2", "https",
    "inspector", "inspector/promises", "net", "os", "perf_hooks", "punycode", "querystring", "readline",
    "readline/promises", "repl", "stream", "stream/consumers", "stream/promises", "stream/web",
    "string_decoder", "sys", "timers/promises", "tls", "trace_events", "tty", "url", "v8", "vm", "wasi",
    "worker_threads", "zlib",
  ];
  const schemeOnly = ["sea", "sqlite", "test", "test/reporters"];

  // What a request names as a built-in: a function giving its exports, or
  // undefined when it isn't one. Throws for one Opal doesn't have yet, and
  // for an unknown `node:` name, as Node does.
  function loadBuiltin(request) {
    const scheme = StringPrototypeStartsWith(request, "node:");
    const name = scheme ? StringPrototypeSlice(request, 5) : request;
    const builtin = builtins.get(name);
    if (builtin !== undefined) return builtin;
    if (ArrayPrototypeIncludes(unimplemented, name) || (scheme && ArrayPrototypeIncludes(schemeOnly, name))) {
      const error = new Error(`Opal does not implement the built-in module node:${name} yet`);
      error.code = "ERR_OPAL_UNIMPLEMENTED_BUILTIN";
      throw error;
    }
    if (scheme) throw new (errors().ERR_UNKNOWN_BUILTIN_MODULE)(request);
    return undefined;
  }

  function isBuiltin(request) {
    if (typeof request !== "string") return false;
    const scheme = StringPrototypeStartsWith(request, "node:");
    const name = scheme ? StringPrototypeSlice(request, 5) : request;
    return (
      builtins.has(name) ||
      ArrayPrototypeIncludes(unimplemented, name) ||
      (scheme && ArrayPrototypeIncludes(schemeOnly, name))
    );
  }

  function dirname(path) {
    const index = StringPrototypeLastIndexOf(path, "/");
    if (index === -1) return ".";
    if (index === 0) return "/";
    return StringPrototypeSlice(path, 0, index);
  }

  // file:// URLs for paths, as url.pathToFileURL makes them for the
  // characters a path holds in practice: the WHATWG path percent-encode set,
  // `%`, and everything outside ASCII, as UTF-8.
  function pathToFileURL(path) {
    let out = "file://";
    for (const byte of utf8(path)) {
      const c = String.fromCharCode(byte);
      out +=
        byte < 0x21 || byte > 0x7e || StringPrototypeIncludes(' "#%<>?`{}', c)
          ? `%${byte.toString(16).toUpperCase().padStart(2, "0")}`
          : c;
    }
    return out;
  }
  // UTF-8 bytes of a string, without loading TextEncoder.
  function utf8(text) {
    const bytes = [];
    for (const char of text) {
      const code = char.codePointAt(0);
      if (code < 0x80) bytes.push(code);
      else if (code < 0x800) bytes.push(0xc0 | (code >> 6), 0x80 | (code & 63));
      else if (code < 0x10000) bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
      else bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 63), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
    }
    return bytes;
  }

  // Whether path.resolve would return `path` as it is: absolute, with no
  // empty, `.` or `..` segment and no trailing slash. The resolver's paths
  // all are, and leaving node's path module unloaded for them keeps it (and
  // the internal/errors and validators it loads) out of every run that
  // doesn't ask for it, which is most of hello world's time.
  function isResolved(path) {
    if (path === "/") return true;
    return (
      StringPrototypeCharCodeAt(path, 0) === 47 &&
      !StringPrototypeEndsWith(path, "/") &&
      !StringPrototypeIncludes(path, "//") &&
      !StringPrototypeIncludes(path, "/./") &&
      !StringPrototypeIncludes(path, "/../") &&
      !StringPrototypeEndsWith(path, "/.") &&
      !StringPrototypeEndsWith(path, "/..")
    );
  }

  // Module._nodeModulePaths, from lib/internal/modules/cjs/loader.js.
  function nodeModulePaths(from) {
    if (typeof from !== "string" || !isResolved(from)) from = requireBuiltin("path").resolve(from);
    if (from === "/") return ["/node_modules"];
    const paths = [];
    const nm = "node_modules";
    for (let i = from.length - 1, p = 0, last = from.length; i >= 0; --i) {
      const code = StringPrototypeCharCodeAt(from, i);
      if (code === 47) {
        if (p !== nm.length) ArrayPrototypePush(paths, `${StringPrototypeSlice(from, 0, last)}/node_modules`);
        last = i;
        p = 0;
      } else if (p !== -1) {
        if (StringPrototypeCharCodeAt(nm, nm.length - 1 - p) === code) ++p;
        else p = -1;
      }
    }
    ArrayPrototypePush(paths, "/node_modules");
    return paths;
  }

  const kFirstParent = Symbol("kFirstParent");

  function Module(id = "", parent) {
    this.id = id;
    this.path = dirname(id);
    this.exports = {};
    this[kFirstParent] = parent;
    if (parent) ArrayPrototypePush(parent.children, this);
    this.filename = null;
    this.loaded = false;
    this.children = [];
  }
  ObjectDefineProperty(Module.prototype, "parent", {
    __proto__: null,
    get() {
      return this[kFirstParent];
    },
    set(value) {
      this[kFirstParent] = value;
    },
    configurable: true,
  });
  Module._cache = { __proto__: null };
  Module._pathCache = { __proto__: null };
  Module._extensions = { __proto__: null };
  Module.wrapper = ["(function (exports, require, module, __filename, __dirname) { ", "\n});"];
  Module.wrap = (script) => `${Module.wrapper[0]}${script}${Module.wrapper[1]}`;
  Module._nodeModulePaths = nodeModulePaths;
  Module.isBuiltin = isBuiltin;

  function emitResolutionWarnings() {
    const warnings = ops.take_resolution_warnings();
    for (let i = 0; i < warnings.length; i++) {
      const { 0: message, 1: type, 2: code } = warnings[i];
      internals.process.emitWarning(message, type, code);
    }
  }

  // The error Node throws for a resolution that failed.
  function resolutionError(failure, request, parent) {
    switch (failure[0]) {
      case "not-found": {
        const requireStack = [];
        for (let cursor = parent; cursor; cursor = cursor[kFirstParent]) {
          ArrayPrototypePush(requireStack, cursor.filename || cursor.id);
        }
        let message = `Cannot find module '${request}'`;
        if (requireStack.length > 0) {
          message += `\nRequire stack:\n- ${ArrayPrototypeJoin(requireStack, "\n- ")}`;
        }
        const error = new Error(message);
        error.code = "MODULE_NOT_FOUND";
        error.requireStack = requireStack;
        return error;
      }
      case "bad-main": {
        const error = new Error(
          `Cannot find module '${failure[1]}'. Please verify that the package.json has a valid "main" entry`,
        );
        error.code = "MODULE_NOT_FOUND";
        error.path = failure[2];
        error.requestPath = request;
        return error;
      }
      case "esm-not-found": {
        const error = new Error(`Cannot find module '${failure[1]}'`);
        error.code = "MODULE_NOT_FOUND";
        if (failure[2] !== null) error.path = failure[2];
        return error;
      }
      default: {
        const { 1: code, 2: hint } = failure;
        const values = ArrayPrototypeMap(ArrayPrototypeSlice(failure, 3), (a) =>
          a === "true" ? true : a === "false" ? false : a === null ? undefined : a,
        );
        const error = ReflectConstruct(errors()[code], values);
        if (hint !== null) {
          const stack = error.stack;
          const end = StringPrototypeIndexOf(stack, "\n");
          error.stack = `${StringPrototypeSlice(stack, 0, end)}\n${hint}${StringPrototypeSlice(stack, end)}`;
          error.message += `\n${hint}`;
        }
        return error;
      }
    }
  }

  Module._resolveFilename = function _resolveFilename(request, parent, isMain, options) {
    if (isBuiltin(request)) return request;
    if (options !== undefined && options !== null && typeof options === "object" && options.paths !== undefined) {
      const error = new Error("require.resolve's `paths` option is not supported by Opal yet");
      error.code = "ERR_OPAL_UNSUPPORTED";
      throw error;
    }
    const resolved = ops.resolve(request, parent?.filename ?? null, 0, !!isMain);
    emitResolutionWarnings();
    if (typeof resolved === "string") return resolved;
    throw resolutionError(resolved, request, parent);
  };

  let mainModule;

  Module._load = function _load(request, parent, isMain) {
    const builtin = loadBuiltin(request);
    if (builtin !== undefined) return builtin();
    const filename = Module._resolveFilename(request, parent, isMain);
    const cached = Module._cache[filename];
    if (cached !== undefined) {
      if (parent && !ArrayPrototypeIncludes(parent.children, cached)) ArrayPrototypePush(parent.children, cached);
      // A module required while it is still loading (a cycle) gives what
      // it has exported so far, as in Node.
      return cached.exports;
    }
    const module = new Module(filename, parent);
    if (isMain) {
      internals.process.mainModule = module;
      mainModule = module;
      module.id = ".";
    }
    Module._cache[filename] = module;
    let threw = true;
    try {
      module.load(filename);
      threw = false;
    } finally {
      if (threw) {
        delete Module._cache[filename];
        if (parent) {
          const index = parent.children.indexOf(module);
          if (index !== -1) ArrayPrototypeSplice(parent.children, index, 1);
        }
      }
    }
    return module.exports;
  };

  Module.prototype.load = function load(filename) {
    this.filename = filename;
    this.paths = nodeModulePaths(dirname(filename));
    const extension = StringPrototypeEndsWith(filename, ".json")
      ? ".json"
      : StringPrototypeEndsWith(filename, ".node")
        ? ".node"
        : ".js";
    Module._extensions[extension](this, filename);
    this.loaded = true;
  };

  Module.prototype.require = function require(id) {
    if (typeof id !== "string") {
      throw new (errors().ERR_INVALID_ARG_TYPE)("id", "string", id);
    }
    if (id === "") {
      throw new (errors().ERR_INVALID_ARG_VALUE)("id", id, "must be a non-empty string");
    }
    return Module._load(id, this, false);
  };

  function makeRequire(module) {
    const require = function require(path) {
      return module.require(path);
    };
    require.resolve = function resolve(request, options) {
      if (typeof request !== "string") {
        throw new (errors().ERR_INVALID_ARG_TYPE)("request", "string", request);
      }
      return Module._resolveFilename(request, module, false, options);
    };
    require.resolve.paths = function paths(request) {
      if (typeof request !== "string") {
        throw new (errors().ERR_INVALID_ARG_TYPE)("request", "string", request);
      }
      if (isBuiltin(request)) return null;
      const relative = request[0] === "." && (request.length === 1 || request[1] === "." || request[1] === "/");
      if (relative) return [module.filename ? dirname(module.filename) : "."];
      return module.paths ? ArrayPrototypeSlice(module.paths) : [];
    };
    require.main = mainModule;
    require.extensions = Module._extensions;
    require.cache = Module._cache;
    return require;
  }

  Module.createRequire = function createRequire(filename) {
    if (typeof filename === "string" && StringPrototypeStartsWith(filename, "file://")) {
      filename = decodeURIComponent(StringPrototypeSlice(filename, 7));
    }
    if (typeof filename !== "string" || filename[0] !== "/") {
      throw new (errors().ERR_INVALID_ARG_VALUE)(
        "filename",
        filename,
        "must be a file URL object, file URL string, or absolute path string",
      );
    }
    const proxy = StringPrototypeEndsWith(filename, "/") ? `${filename}noop.js` : filename;
    const module = new Module(proxy, null);
    module.filename = proxy;
    module.paths = nodeModulePaths(dirname(proxy));
    return makeRequire(module);
  };

  // The source a file runs as: TypeScript stripped of its types.
  function sourceOf(filename) {
    const content = ops.read_file(filename);
    if (RegExpPrototypeExec(/\.(ts|cts)$/, filename) === null) return content;
    if (StringPrototypeIncludes(filename, "/node_modules/")) {
      throw new (errors().ERR_UNSUPPORTED_NODE_MODULES_TYPE_STRIPPING)(filename);
    }
    const stripped = ops.strip_types(content);
    // Node names the file by URL for V8, so a stack frame in a TypeScript
    // file reads file:///... (internal/modules/typescript.js).
    if (typeof stripped === "string") return `${stripped}\n\n//# sourceURL=${pathToFileURL(filename)}`;
    // What Node's internal/modules/typescript.js throws: the error, with
    // where it is and swc's excerpt of the source above its stack, and no
    // arrow pointing into the runtime's own code.
    const { 0: code, 1: message, 2: line, 3: snippet } = stripped;
    const error = new (errors()[code])(message);
    error.stack = `${pathToFileURL(filename)}:${line}\n${snippet}\n${error.stack}`;
    internals.noArrow.add(error);
    throw error;
  }

  Module._extensions[".js"] = function loadJs(module, filename) {
    const { format, source } = internals.esm.formatOf(filename);
    if (format === "module") {
      internals.esm.requireModule(module, filename, module[kFirstParent]);
      return;
    }
    module._compile(source ?? sourceOf(filename), filename);
  };

  Module._extensions[".json"] = function loadJson(module, filename) {
    let content = ops.read_file(filename);
    if (StringPrototypeCharCodeAt(content, 0) === 0xfeff) content = StringPrototypeSlice(content, 1);
    try {
      module.exports = JSON.parse(content);
    } catch (error) {
      error.message = `${filename}: ${error.message}`;
      throw error;
    }
  };

  Module._extensions[".node"] = function loadAddon(module, filename) {
    const error = new Error(`${filename}: Opal cannot load Node-API addons yet`);
    error.code = "ERR_DLOPEN_FAILED";
    throw error;
  };

  Module.prototype._compile = function _compile(content, filename) {
    // A shebang line is not JavaScript; V8 accepts one only at the start of
    // a script, and this is a function body.
    if (StringPrototypeStartsWith(content, "#!")) {
      const end = StringPrototypeIndexOf(content, "\n");
      content = end === -1 ? "" : `//${StringPrototypeSlice(content, 2, end)}${StringPrototypeSlice(content, end)}`;
    }
    const compiled = ops.compile_function(content, filename, [
      "exports",
      "require",
      "module",
      "__filename",
      "__dirname",
    ]);
    const require = makeRequire(this);
    return ReflectApply(compiled, this.exports, [this.exports, require, this, filename, dirname(filename)]);
  };

  function runMain(filename) {
    if (internals.esm.formatOf(filename).format === "module") {
      internals.mainURL = pathToFileURL(filename);
      internals.esm.runMain(filename);
      return;
    }
    Module._load(filename, null, true);
  }

  ObjectDefineProperty(Module, "Module", { __proto__: null, value: Module });
  defineBuiltin("module", () => Module);
  Module.builtinModules = [...builtins.keys(), ...unimplemented].sort();

  internals.loader = {
    Module,
    runMain,
    isBuiltin,
    loadBuiltin,
    pathToFileURL,
    sourceOf,
    resolutionError,
    emitResolutionWarnings,
  };
})
