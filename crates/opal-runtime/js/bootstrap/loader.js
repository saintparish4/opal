// CommonJS, as Node's lib/internal/modules/cjs/loader.js runs it: each file is
// compiled as the body of
//   function (exports, require, module, __filename, __dirname)
// (with V8's CompileFunction, so line 1 column 1 of the file is line 1
// column 1 in stack traces), and called with `this` set to module.exports,
// which is why a top-level frame reads `Object.<anonymous>`.
//
// A TypeScript file (.ts, .cts) has its types stripped first, as Node does
// by default since v23.6: replaced by whitespace, so lines and columns are
// the source's. Node refuses to strip files under node_modules, and so does
// this. ES modules aren't run yet: a .mjs or .mts file, or a .js or .ts file
// that turns out to use import or export (which Node would then run as an
// ES module), is refused rather than run as something it isn't.
(function (internals, ops, primordials) {
  "use strict";

  const {
    ObjectDefineProperty,
    RegExpPrototypeExec,
    StringPrototypeIncludes,
  } = primordials;
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

  // file:// URLs for paths, as url.pathToFileURL makes them for the
  // characters a path holds in practice: the WHATWG path percent-encode set,
  // `%`, and everything outside ASCII, as UTF-8.
  function pathToFileURL(path) {
    let out = "file://";
    for (const byte of new TextEncoderLite().encode(path)) {
      const c = String.fromCharCode(byte);
      out += byte < 0x21 || byte > 0x7e || ' "#%<>?`{}'.includes(c)
        ? `%${byte.toString(16).toUpperCase().padStart(2, "0")}`
        : c;
    }
    return out;
  }
  // UTF-8 bytes of a string, without loading node:util's TextEncoder.
  function TextEncoderLite() {}
  TextEncoderLite.prototype.encode = function encode(text) {
    const bytes = [];
    for (const char of text) {
      const code = char.codePointAt(0);
      if (code < 0x80) bytes.push(code);
      else if (code < 0x800) bytes.push(0xc0 | (code >> 6), 0x80 | (code & 63));
      else if (code < 0x10000) bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
      else bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 63), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
    }
    return bytes;
  };

  function unsupportedModule(filename) {
    const error = new Error(`${filename}: ES modules are not supported by \`opal run\` yet`);
    error.code = "ERR_OPAL_ESM_UNSUPPORTED";
    return error;
  }

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
    const [code, message, line, snippet] = stripped;
    const error = new (errors()[code])(message);
    error.stack = `${pathToFileURL(filename)}:${line}\n${snippet}\n${error.stack}`;
    internals.noArrow.add(error);
    throw error;
  }

  // V8's messages for syntax only an ES module may use, on which Node
  // (node_contextify.cc) retries a file as one.
  const esmSyntax = [
    "Cannot use import statement outside a module",
    "Unexpected token 'export'",
    "Cannot use 'import.meta' outside a module",
    "await is only valid in async functions and the top level bodies of modules",
  ];

  Module.prototype._compile = function _compile(content, filename) {
    // A shebang line is not JavaScript; V8 accepts one only at the start of
    // a script, and this is a function body.
    if (content.startsWith("#!")) {
      const end = content.indexOf("\n");
      content = end === -1 ? "" : `//${content.slice(2, end)}${content.slice(end)}`;
    }
    let compiled;
    try {
      compiled = ops.compile_function(content, filename, [
        "exports",
        "require",
        "module",
        "__filename",
        "__dirname",
      ]);
    } catch (error) {
      if (error instanceof SyntaxError && esmSyntax.includes(error.message)) {
        throw unsupportedModule(filename);
      }
      throw error;
    }
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
    if (RegExpPrototypeExec(/\.m[jt]s$/, filename) !== null) throw unsupportedModule(filename);
    module._compile(sourceOf(filename), filename);
    module.loaded = true;
  }

  ObjectDefineProperty(Module, "Module", { value: Module });
  defineBuiltin("module", () => Module);
  Module.builtinModules = [...builtins.keys()];

  internals.loader = { Module, runMain, builtinFor, pathToFileURL };
})
