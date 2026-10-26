// ES modules, as Node's lib/internal/modules/esm/ runs them: a file's format
// from its extension, its package's "type", or (for a typeless .js or .ts)
// whether it only parses as a module; what a specifier names from Node's
// resolver (the resolve op, mode 1); and the module graph compiled, linked,
// instantiated and evaluated by V8 (engine/esm.rs). A CommonJS module, a
// JSON file or a built-in imported from an ES module is a synthetic module
// whose exports are set when it is evaluated, as Node's translators make
// them. require() of an ES module (without top-level await) runs it
// synchronously and returns its namespace, as Node does since v22.12.
(function (internals, ops, primordials) {
  "use strict";

  const {
    ArrayPrototypeConcat,
    ArrayPrototypeIncludes,
    ArrayPrototypePush,
    ObjectKeys,
    ObjectPrototypeHasOwnProperty,
    PromiseResolve,
    SafeMap,
    SafeSet,
    StringPrototypeIndexOf,
    StringPrototypeLastIndexOf,
    StringPrototypeSlice,
    StringPrototypeStartsWith,
  } = primordials;
  const errors = () => internals.requireBuiltin("internal/errors").codes;
  const loader = internals.loader;

  // V8's messages for syntax only an ES module may use (node_contextify.cc).
  const esmSyntax = [
    "Cannot use import statement outside a module",
    "Unexpected token 'export'",
    "Cannot use 'import.meta' outside a module",
    "await is only valid in async functions and the top level bodies of modules",
  ];

  function extname(path) {
    const slash = StringPrototypeLastIndexOf(path, "/");
    const dot = StringPrototypeLastIndexOf(path, ".");
    return dot > slash + 1 ? StringPrototypeSlice(path, dot) : "";
  }

  function fileURLToPath(url) {
    let rest = StringPrototypeSlice(url, "file://".length);
    const query = StringPrototypeIndexOf(rest, "?");
    if (query !== -1) rest = StringPrototypeSlice(rest, 0, query);
    const hash = StringPrototypeIndexOf(rest, "#");
    if (hash !== -1) rest = StringPrototypeSlice(rest, 0, hash);
    return decodeURIComponent(rest);
  }

  // Whether `source` is an ES module by its syntax: it fails to compile as
  // CommonJS, for a reason only module syntax gives.
  function detect(source, path) {
    try {
      ops.compile_function(source, path, ["exports", "require", "module", "__filename", "__dirname"]);
      return false;
    } catch (error) {
      return error instanceof SyntaxError && ArrayPrototypeIncludes(esmSyntax, error.message);
    }
  }

  // defaultGetFormat for a file: "module", "commonjs" or "json", and whether
  // its source needs its types stripped.
  function formatOf(path) {
    const ext = extname(path);
    switch (ext) {
      case ".mjs":
        return { format: "module", typescript: false };
      case ".cjs":
        return { format: "commonjs", typescript: false };
      case ".json":
        return { format: "json", typescript: false };
      case ".mts":
        return { format: "module", typescript: true };
      case ".cts":
        return { format: "commonjs", typescript: true };
      case ".js":
      case ".ts": {
        const typescript = ext === ".ts";
        const { 0: type, 1: packageJson } = ops.package_type(path);
        if (type === "module" || type === "commonjs") return { format: type, typescript };
        const source = loader.sourceOf(path);
        const isModule = detect(source, path);
        if (isModule && packageJson !== null) warnTypeless(packageJson, loader.pathToFileURL(path));
        return { format: isModule ? "module" : "commonjs", typescript, source };
      }
      default:
        if (ext === "" && !typeless(path)) return { format: "commonjs", typescript: false };
        throw new (errors().ERR_UNKNOWN_FILE_EXTENSION)(ext, path);
    }
  }

  // An extensionless file outside a "type": "module" package runs as
  // CommonJS (Node's rule for bin scripts); inside one it is an error.
  function typeless(path) {
    return ops.package_type(path)[0] === "module";
  }

  // warnTypelessPackageJsonFile, once per package.json, outside node_modules.
  const warnedTypeless = new SafeSet();
  function warnTypeless(packageJson, url) {
    if (url.includes("/node_modules/") || warnedTypeless.has(packageJson)) return;
    warnedTypeless.add(packageJson);
    internals.process.emitWarning(
      `Module type of ${url} is not specified and it doesn't parse as CommonJS.\n` +
        "Reparsing as ES module because module syntax was detected. This incurs a performance overhead.\n" +
        `To eliminate this warning, add "type": "module" to ${packageJson}.`,
      { code: "MODULE_TYPELESS_PACKAGE_JSON" },
    );
  }

  // What an import names: a file, or a built-in.
  function resolveImport(specifier, parentPath) {
    if (loader.isBuiltin(specifier) && !StringPrototypeStartsWith(specifier, "node:sea")) {
      const name = StringPrototypeStartsWith(specifier, "node:") ? StringPrototypeSlice(specifier, 5) : specifier;
      return { builtin: name, url: `node:${name}` };
    }
    if (StringPrototypeStartsWith(specifier, "node:")) {
      throw new (errors().ERR_UNKNOWN_BUILTIN_MODULE)(specifier);
    }
    const resolved = ops.resolve(specifier, parentPath, 1, false);
    loader.emitResolutionWarnings();
    if (typeof resolved === "string") return { path: resolved, url: loader.pathToFileURL(resolved) };
    if (resolved[0] === "builtin") return resolveImport(`node:${resolved[1]}`, parentPath);
    if (resolved[0] === "url") {
      const url = resolved[1];
      const protocol = StringPrototypeSlice(url, 0, url.indexOf(":") + 1);
      throw new (errors().ERR_UNSUPPORTED_ESM_URL_SCHEME)({ href: url, protocol }, ["file", "data", "node"]);
    }
    throw loader.resolutionError(resolved, specifier, null);
  }

  // Modules by URL, and by URL and `type` attribute for JSON.
  const records = new SafeMap();

  function checkAttributes(url, format, type) {
    if (type !== "" && type !== "json") {
      throw new (errors().ERR_IMPORT_ATTRIBUTE_UNSUPPORTED)("type", type, url);
    }
    if (format === "json" && type !== "json") {
      throw new (errors().ERR_IMPORT_ATTRIBUTE_MISSING)(url, "type", "json");
    }
    if (type === "json" && format !== "json") {
      throw new (errors().ERR_IMPORT_ATTRIBUTE_TYPE_INCOMPATIBLE)(url, "json");
    }
  }

  // The record for what `resolved` names, compiled and linked with
  // everything it imports, made once per URL.
  function recordFor(resolved, type) {
    const cached = records.get(resolved.url);
    if (cached !== undefined) {
      if (resolved.path !== undefined) checkAttributes(resolved.url, cached.format, type);
      return cached;
    }
    if (resolved.builtin !== undefined) return builtinRecord(resolved);
    const { path, url } = resolved;
    const { format, source } = formatOf(path);
    checkAttributes(url, format, type);
    const record = { url, path, format, id: undefined, dependencies: [] };
    records.set(url, record);
    if (format === "module") {
      const code = source ?? loader.sourceOf(path);
      const { 0: id, 1: requests } = ops.esm_compile(code, url);
      record.id = id;
      const links = [];
      for (let i = 0; i < requests.length; i++) {
        const { 0: specifier, 1: attributes } = requests[i];
        let kind = "";
        for (let j = 0; j < attributes.length; j++) {
          const { 0: key, 1: value } = attributes[j];
          if (key === "type") kind = value;
          else throw new (errors().ERR_IMPORT_ATTRIBUTE_UNSUPPORTED)(key, value, url);
        }
        const target = recordFor(resolveImport(specifier, path), kind);
        ArrayPrototypePush(links, [specifier, kind, target.id]);
        ArrayPrototypePush(record.dependencies, [specifier, target.format]);
      }
      ops.esm_link(id, links);
    } else if (format === "json") {
      record.id = ops.esm_synthetic(url, ["default"], (id) => {
        let content = ops.read_file(path);
        if (content.charCodeAt(0) === 0xfeff) content = StringPrototypeSlice(content, 1);
        let value;
        try {
          value = JSON.parse(content);
        } catch (error) {
          error.message = `${path}: ${error.message}`;
          throw error;
        }
        ops.esm_set_export(id, "default", value);
      });
    } else {
      record.id = commonjsFacade(url, path);
    }
    return record;
  }

  // The named exports a CommonJS module is statically seen to have, by
  // cjs-module-lexer (js/opal/internal/deps/), with those of what it
  // re-exports (cjsPreparseModuleExports). Node 24's lexer is a native port
  // of this one; where the JS version throws (on module syntax, or a regular
  // expression it can't read) Node's returns nothing, and so does this.
  function lex(source) {
    try {
      const { exports, reexports } = internals.requireBuiltin("internal/deps/cjs-module-lexer/lexer").parse(source);
      return [exports, reexports];
    } catch {
      return [[], []];
    }
  }

  function commonjsNames(path, seen = new SafeSet()) {
    const names = new SafeSet();
    if (seen.has(path)) return names;
    seen.add(path);
    let source;
    try {
      source = loader.sourceOf(path);
    } catch {
      return names;
    }
    const { 0: exportNames, 1: reexports } = lex(source);
    for (let i = 0; i < exportNames.length; i++) names.add(exportNames[i]);
    for (let i = 0; i < reexports.length; i++) {
      const reexport = reexports[i];
      let resolved;
      try {
        resolved = loader.Module._resolveFilename(reexport, { filename: path }, false);
      } catch {
        continue;
      }
      if (!loader.isBuiltin(resolved) && (extname(resolved) === ".js" || extname(resolved) === ".cjs")) {
        for (const name of commonjsNames(resolved, seen)) names.add(name);
      }
    }
    return names;
  }

  // createCJSModuleWrap: the CommonJS module runs when the facade is
  // evaluated, and its exports become the facade's.
  function commonjsFacade(url, path) {
    const exportNames = commonjsNames(path);
    const names = [...exportNames];
    if (!exportNames.has("default")) ArrayPrototypePush(names, "default");
    if (!exportNames.has("module.exports")) ArrayPrototypePush(names, "module.exports");
    return ops.esm_synthetic(url, names, (id) => {
      const exports = loader.Module._load(path, null, false);
      for (const name of exportNames) {
        if (name === "default" || name === "module.exports" || !ObjectPrototypeHasOwnProperty(exports, name)) continue;
        let value;
        try {
          value = exports[name];
        } catch {
          // A throwing getter leaves the export undefined, as in Node.
        }
        ops.esm_set_export(id, name, value);
      }
      ops.esm_set_export(id, "default", exports);
      ops.esm_set_export(id, "module.exports", exports);
    });
  }

  // A built-in's facade: its exports by name, and itself as the default.
  function builtinRecord(resolved) {
    const exports = loader.loadBuiltin(resolved.builtin)();
    const names = ObjectKeys(exports).filter((name) => name !== "default");
    const record = { url: resolved.url, format: "builtin", id: undefined };
    record.id = ops.esm_synthetic(resolved.url, ArrayPrototypeConcat(names, "default"), (id) => {
      for (let i = 0; i < names.length; i++) {
        const name = names[i];
        let value;
        try {
          value = exports[name];
        } catch {
          // A lazy export whose module Opal lacks (events'
          // EventEmitterAsyncResource needs async_hooks) stays undefined.
        }
        ops.esm_set_export(id, name, value);
      }
      ops.esm_set_export(id, "default", exports);
    });
    records.set(resolved.url, record);
    return record;
  }

  // Links a graph. A link error (an import of an export that isn't there)
  // gets the import's line above its stack, as Node's module_job.js
  // decorates it.
  function instantiate(record) {
    try {
      ops.esm_instantiate(record.id);
    } catch (error) {
      if (error instanceof SyntaxError && !internals.noArrow.has(error)) {
        const arrow = ops.error_arrow(error);
        if (arrow !== undefined) {
          error.stack = `${arrow}${error.stack}`;
          internals.noArrow.add(error);
        }
        commonjsExportHint(error, record);
      }
      throw error;
    }
  }

  // A missing named export of a CommonJS module the failing module imports
  // directly gets Node's explanation and the default-import way out
  // (module_job.js).
  function commonjsExportHint(error, record) {
    const match = /module '(.*)' does not provide an export named '(.+)'/.exec(error.message);
    if (match === null) return;
    const { 1: childSpecifier, 2: name } = match;
    const dependency = record.dependencies?.find((dependency) => dependency[0] === childSpecifier);
    if (dependency === undefined || dependency[1] !== "commonjs") return;
    const importStatement = error.stack.split("\n", 2)[1];
    const oneLineNamedImports = /{.*}/.exec(importStatement);
    const destructuringAssignment = oneLineNamedImports && oneLineNamedImports[0].replace(/\s+as\s+/g, ": ");
    error.message =
      `Named export '${name}' not found. The requested module` +
      ` '${childSpecifier}' is a CommonJS module, which may not support` +
      " all module.exports as named exports.\nCommonJS modules can " +
      "always be imported via the default export, for example using:" +
      `\n\nimport pkg from '${childSpecifier}';\n${
        destructuringAssignment ? `const ${destructuringAssignment} = pkg;\n` : ""
      }`;
    const stack = error.stack.split("\n");
    stack[3] = `SyntaxError: ${error.message}`;
    error.stack = stack.join("\n");
  }

  // Instantiates and evaluates a graph; the evaluation's promise.
  function run(record) {
    instantiate(record);
    return ops.esm_evaluate(record.id);
  }

  // The main module is loaded now and run after the ticks queued while
  // loading (a warning about its package.json, say), as Node's promise-based
  // loader runs it. An error it throws, now or after an await, ends the
  // process as an uncaught exception, as in Node.
  function runMain(path) {
    const record = recordFor({ path, url: loader.pathToFileURL(path) }, "");
    PromiseResolve()
      .then(() => run(record))
      .then(undefined, (error) => {
        ArrayPrototypePush(internals.pendingUncaught, error);
      });
  }

  // require() of an ES module.
  function requireModule(module, path, parent) {
    const record = recordFor({ path, url: loader.pathToFileURL(path) }, "");
    instantiate(record);
    const { 0: status, 1: isAsync } = ops.esm_status(record.id);
    if (isAsync && status < 4) {
      throw new (errors().ERR_REQUIRE_ASYNC_MODULE)(path, parent?.filename);
    }
    const promise = ops.esm_evaluate(record.id);
    const { 0: state, 1: value } = ops.promise_details(promise);
    if (state === 2) {
      // Handled here: the error is require()'s to throw.
      promise.then(undefined, () => {});
      throw value;
    }
    const namespace = ops.esm_namespace(record.id);
    module.exports = ObjectPrototypeHasOwnProperty(namespace, "module.exports") ? namespace["module.exports"] : namespace;
  }

  // The path an import from `referrer` (a file URL for an ES module, a path
  // for a CommonJS one) is resolved from.
  function referrerPath(referrer) {
    if (typeof referrer === "string") {
      if (StringPrototypeStartsWith(referrer, "file://")) return fileURLToPath(referrer);
      if (referrer[0] === "/") return referrer;
    }
    return `${ops.cwd()}/[eval]`;
  }

  // import() from anywhere: the module's namespace, once it has run.
  async function importDynamic(specifier, referrer, type) {
    const record = recordFor(resolveImport(`${specifier}`, referrerPath(referrer)), type);
    await run(record);
    return ops.esm_namespace(record.id);
  }

  // initializeImportMeta: dirname and filename for a file, main, resolve
  // and url, in that order.
  function initializeImportMeta(meta, url) {
    if (StringPrototypeStartsWith(url, "file:")) {
      const filename = fileURLToPath(url);
      const slash = StringPrototypeLastIndexOf(filename, "/");
      meta.dirname = slash === 0 ? "/" : StringPrototypeSlice(filename, 0, slash);
      meta.filename = filename;
    }
    meta.main = internals.mainURL === url;
    const parent = referrerPath(url);
    meta.resolve = function resolve(specifier) {
      try {
        return resolveImport(`${specifier}`, parent).url;
      } catch (error) {
        if ((error?.code === "ERR_MODULE_NOT_FOUND" || error?.code === "ERR_UNSUPPORTED_DIR_IMPORT") && error.url) {
          return error.url;
        }
        throw error;
      }
    };
    meta.url = url;
  }

  internals.esm = { formatOf, runMain, requireModule, importDynamic, initializeImportMeta, fileURLToPath };
})
