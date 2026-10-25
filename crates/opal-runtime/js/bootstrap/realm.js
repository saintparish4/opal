// The built-in module loader and internalBinding, the two things Node's own
// library files (js/node/) expect to be handed, as Node's
// lib/internal/bootstrap/realm.js hands them: each built-in is the body of a
// function of (exports, require, module, process, internalBinding,
// primordials), compiled on first require from source embedded in the
// binary, and cached.
//
// internalBinding(name) is where Node's files reach into C++. Each binding
// here is the subset of Node's that the vendored files call, backed by ops
// (src/engine/ops.rs); a name with no binding is an error naming it, so a
// vendored file that needs more fails loudly rather than misbehaving.
//
// It also makes the `process` object, before any built-in loads: Node's
// files read process.platform as they load (internal/util.js), and some of
// them load while the snapshot is made. platform, arch and the versions are
// properties of the build target the snapshot is for; everything that
// belongs to a run is filled in by process.js's setup.
(function (internals, ops, primordials) {
  "use strict";

  const {
    ArrayPrototypeForEach,
    ObjectDefineProperty,
    ObjectKeys,
    SafeMap,
    StringPrototypeReplace,
    SymbolFor,
  } = primordials;

  // The Node release whose behavior this runtime is measured against
  // (compat/ and Node's own tests run against it, and js/node/ is its
  // library). Packages gate features on process.version, so it names a Node
  // version, as Bun's does; Opal's own version is process.versions.opal.
  const NODE_VERSION = "24.19.0";
  const OPAL_VERSION = "0.4.1";

  // Node creates process in C++ with a constructor named `process`, then
  // gives it EventEmitter's prototype (process.js).
  function process() {}
  const proc = new process();
  const [, , platform, arch, v8Version] = ops.process_info();
  proc.version = `v${NODE_VERSION}`;
  proc.versions = { node: NODE_VERSION, opal: OPAL_VERSION, v8: v8Version, modules: "137" };
  proc.platform = platform;
  proc.arch = arch;
  proc.release = { name: "node" };
  internals.process = proc;

  const privateSymbols = { __proto__: null };
  ArrayPrototypeForEach(
    [
      "arrow_message_private_symbol",
      "contextify_context_private_symbol",
      "decorated_private_symbol",
      "entry_point_module_private_symbol",
      "entry_point_promise_private_symbol",
      "exit_info_private_symbol",
      "host_defined_option_symbol",
      "module_circular_visited_private_symbol",
      "module_export_names_private_symbol",
      "module_export_private_symbol",
      "module_first_parent_private_symbol",
      "module_last_parent_private_symbol",
      "module_source_private_symbol",
      "source_map_data_private_symbol",
      "transfer_mode_private_symbol",
      "untransferable_object_private_symbol",
    ],
    (name) => {
      // V8 private symbols in Node, which reflection can't see. These are
      // ordinary symbols; Object.getOwnPropertySymbols lists them.
      privateSymbols[name] = Symbol(name);
    },
  );

  function defineLazyProperties(target, id, keys, enumerable = true) {
    for (const key of keys) {
      let value;
      let loaded = false;
      ObjectDefineProperty(target, key, {
        __proto__: null,
        enumerable,
        configurable: true,
        get() {
          if (!loaded) {
            value = requireBuiltin(id)[key];
            loaded = true;
          }
          return value;
        },
        set(newValue) {
          ObjectDefineProperty(target, key, {
            __proto__: null,
            value: newValue,
            writable: true,
            enumerable,
            configurable: true,
          });
        },
      });
    }
  }

  const camelCase = (name) => StringPrototypeReplace(name, /_([a-z])/g, (_, c) => c.toUpperCase());

  const bindingMakers = {
    __proto__: null,
    util: () => ({
      constants: {
        kPending: 0,
        kFulfilled: 1,
        kRejected: 2,
        kExiting: 0,
        kExitCode: 1,
        kHasExitCode: 2,
        ALL_PROPERTIES: 0,
        ONLY_WRITABLE: 1,
        ONLY_ENUMERABLE: 2,
        ONLY_CONFIGURABLE: 4,
        SKIP_STRINGS: 8,
        SKIP_SYMBOLS: 16,
        kDisallowCloneAndTransfer: 0,
        kTransferable: 1,
        kCloneable: 2,
      },
      getOwnNonIndexProperties: ops.own_non_index_properties,
      getPromiseDetails: ops.promise_details,
      getProxyDetails: ops.proxy_details,
      previewEntries: ops.preview_entries,
      getConstructorName: ops.constructor_name,
      getExternalValue: () => 0n,
      privateSymbols,
      constructSharedArrayBuffer: (length) => new SharedArrayBuffer(length),
      guessHandleType: ops.guess_handle_type,
      defineLazyProperties,
      sleep: ops.sleep,
      isInsideNodeModules: () => false,
      shouldAbortOnUncaughtToggle: [0],
    }),
    types: () => {
      const binding = {};
      for (const name of ObjectKeys(ops)) {
        if (name.startsWith("is_")) binding[camelCase(name)] = ops[name];
      }
      return binding;
    },
    constants: () => ({ os: ops.os_constants(), fs: {}, crypto: {}, zlib: {}, trace: {} }),
    config: () => ({ hasIntl: false, hasInspector: false, noBrowserGlobals: false, bits: 64 }),
    buffer: () => {
      const slice = (encoding) => (buffer, start, end) => ops.buffer_slice(buffer, start, end, encoding);
      const write = (encoding) => (buffer, string, offset, length) =>
        ops.buffer_write(buffer, string, offset, length, encoding);
      // Encodings by Node's `enum encoding` (src/node.h).
      const [ASCII, UTF8, BASE64, UCS2, LATIN1, HEX, , BASE64URL] = [0, 1, 2, 3, 4, 5, 6, 7];
      return {
        kMaxLength: 2 ** 53 - 1,
        kStringMaxLength: 2 ** 29 - 24,
        asciiSlice: slice(ASCII),
        base64Slice: slice(BASE64),
        base64urlSlice: slice(BASE64URL),
        latin1Slice: slice(LATIN1),
        hexSlice: slice(HEX),
        ucs2Slice: slice(UCS2),
        utf8Slice: slice(UTF8),
        asciiWriteStatic: write(ASCII),
        latin1WriteStatic: write(LATIN1),
        utf8WriteStatic: write(UTF8),
        base64Write: write(BASE64),
        base64urlWrite: write(BASE64URL),
        hexWrite: write(HEX),
        ucs2Write: write(UCS2),
        byteLengthUtf8: ops.buffer_byte_length_utf8,
        compare: ops.buffer_compare,
        compareOffset: ops.buffer_compare_offset,
        copy: ops.buffer_copy,
        fill: ops.buffer_fill,
        isAscii: ops.buffer_is_ascii,
        isUtf8: ops.buffer_is_utf8,
        indexOfBuffer: ops.buffer_index_of_buffer,
        indexOfNumber: ops.buffer_index_of_number,
        indexOfString: ops.buffer_index_of_string,
        swap16: ops.buffer_swap16,
        swap32: ops.buffer_swap32,
        swap64: ops.buffer_swap64,
        atob: ops.buffer_atob,
        btoa: ops.buffer_btoa,
        // Node leaves this memory uninitialized; V8's is zeroed.
        createUnsafeArrayBuffer: (size) => new ArrayBuffer(size),
        // ArrayBuffer detach keys guard against transferring the pool to a
        // worker; there are no workers.
        setDetachKey() {},
      };
    },
    // DOMException, from Node's own per-context script for it.
    messaging: () => {
      const exports = {};
      const perIsolateSymbols = {
        messaging_clone_symbol: Symbol("messaging_clone_symbol"),
        messaging_deserialize_symbol: Symbol("messaging_deserialize_symbol"),
      };
      const script = ops.compile_builtin("internal/per_context/domexception", [
        "exports",
        "primordials",
        "privateSymbols",
        "perIsolateSymbols",
      ]);
      script(exports, primordials, privateSymbols, perIsolateSymbols);
      return { DOMException: exports.DOMException };
    },
    // Indexed by Node's `enum encoding` (src/node.h).
    string_decoder: () => ({
      encodings: ["ascii", "utf8", "base64", "utf16le", "latin1", "hex", "buffer", "base64url"],
    }),
  };

  const bindings = new SafeMap();
  function internalBinding(name) {
    let binding = bindings.get(name);
    if (binding === undefined) {
      const make = bindingMakers[name];
      if (make === undefined) {
        throw new Error(`No such binding: ${name} (Opal does not implement it yet)`);
      }
      binding = make();
      bindings.set(name, binding);
    }
    return binding;
  }

  const modules = new SafeMap();
  // Opal's own built-ins (js/opal/) reach the bootstrap's state through this.
  modules.set("internal/opal/internals", { exports: internals, id: "internal/opal/internals", loaded: true });
  function requireBuiltin(id) {
    const cached = modules.get(id);
    if (cached !== undefined) return cached.exports;
    const compiled = ops.compile_builtin(id);
    if (compiled === undefined) {
      const error = new Error(`No such built-in module: ${id}`);
      error.code = "ERR_UNKNOWN_BUILTIN_MODULE";
      throw error;
    }
    const module = { exports: {}, id, loaded: false };
    modules.set(id, module);
    compiled(module.exports, requireBuiltin, module, proc, internalBinding, primordials);
    module.loaded = true;
    return module.exports;
  }

  // While build.rs makes the snapshot; main.js's start clears it and runs
  // what modules loaded meanwhile asked to run then.
  internals.buildingSnapshot = true;
  internals.deserializeCallbacks = [];

  internals.requireBuiltin = requireBuiltin;
  internals.internalBinding = internalBinding;
  internals.customInspectSymbol = SymbolFor("nodejs.util.inspect.custom");

  for (const id of internals.snapshotBuiltins) requireBuiltin(id);
})
