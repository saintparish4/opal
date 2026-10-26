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
    ArrayPrototypeIncludes,
    ObjectDefineProperty,
    ObjectKeys,
    SafeMap,
    StringPrototypeReplace,
    StringPrototypeSlice,
    StringPrototypeStartsWith,
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
      getCallSites: ops.call_sites,
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
    constants: () => ({ os: ops.os_constants(), fs: ops.fs_constants(), crypto: {}, zlib: {}, trace: {} }),
    // libuv's error names and messages, by error number.
    uv: () => {
      const binding = {};
      const map = new SafeMap();
      for (const { 0: errno, 1: name, 2: message } of ops.uv_errors()) {
        binding[`UV_${name}`] = errno;
        map.set(errno, [name, message]);
      }
      binding.getErrorMap = () => new Map(map);
      binding.errname = (errno) => map.get(errno)?.[0] ?? `Unknown system error ${errno}`;
      binding.getErrorMessage = (errno) => map.get(errno)?.[1] ?? `Unknown system error ${errno}`;
      return binding;
    },
    // Where V8 places an error. Source maps aren't supported, so turning
    // them on changes nothing.
    errors: () => ({
      getErrorSourcePositions: ops.error_source_positions,
      setSourceMapsEnabled() {},
      // An error a hook threw, which Node hands its fatal exception handler:
      // thrown on, it reaches the event loop's.
      triggerUncaughtException(error) {
        throw error;
      },
      // ExitCode (src/node_exit_code.h).
      exitCodes: {
        kNoFailure: 0,
        kGenericUserError: 1,
        kInternalJSParseError: 3,
        kInternalJSEvaluationFailure: 4,
        kV8FatalError: 5,
        kInvalidFatalExceptionMonkeyPatching: 6,
        kExceptionInFatalExceptionHandler: 7,
        kInvalidCommandLineArgument: 9,
        kBootstrapFailure: 10,
        kInvalidCommandLineArgument2: 12,
        kUnsettledTopLevelAwait: 13,
        kStartupSnapshotFailure: 14,
        kAbort: 134,
      },
    }),
    // Node's per-isolate symbols (env_properties.h), one of each.
    symbols: () => {
      const symbols = {};
      ArrayPrototypeForEach(
        [
          "fs_use_promises_symbol",
          "async_id_symbol",
          "constructor_key_symbol",
          "handle_onclose",
          "no_message_symbol",
          "messaging_deserialize_symbol",
          "imported_cjs_symbol",
          "messaging_transfer_symbol",
          "messaging_clone_symbol",
          "messaging_transfer_list_symbol",
          "oninit",
          "owner_symbol",
          "onpskexchange",
          "resource_symbol",
          "trigger_async_id_symbol",
          "builtin_source_text_module_hdo",
          "source_text_module_default_hdo",
          "vm_context_no_contextify",
          "vm_dynamic_import_default_internal",
          "vm_dynamic_import_main_context_default",
          "vm_dynamic_import_missing_flag",
          "vm_dynamic_import_no_callback",
        ],
        (name) => {
          symbols[name] = Symbol(name);
        },
      );
      return symbols;
    },
    // What lib/internal/timers.js asks of Node's C++ (timers.js).
    timers: () => internals.timerBinding,
    task_queue: () => ({ enqueueMicrotask: ops.enqueue_microtask }),
    // Where AsyncContextFrame keeps the current frame: V8's continuation
    // data, which promise reactions carry along.
    async_context_frame: () => ({
      getContinuationPreservedEmbedderData: ops.cped_get,
      setContinuationPreservedEmbedderData: ops.cped_set,
    }),
    // Environment::AsyncHooks, which lib/internal/async_hooks.js reads and
    // writes directly. No hook can be registered (node:async_hooks isn't
    // there yet), so the hook counts stay 0 but for kCheck, which is on by
    // default; the ids are the main script's (1) until something else runs.
    async_wrap: () => {
      const constants = {
        kInit: 0,
        kBefore: 1,
        kAfter: 2,
        kDestroy: 3,
        kPromiseResolve: 4,
        kTotals: 5,
        kCheck: 6,
        kExecutionAsyncId: 0,
        kTriggerAsyncId: 1,
        kAsyncIdCounter: 2,
        kDefaultTriggerAsyncId: 3,
        kUsesExecutionAsyncResource: 8,
        kStackLength: 7,
      };
      const { kCheck, kStackLength, kExecutionAsyncId, kTriggerAsyncId } = constants;
      const async_hook_fields = new Uint32Array(9);
      async_hook_fields[kCheck] = 1;
      const async_id_fields = new Float64Array([1, 0, 1, -1]);
      const execution_async_resources = [];
      const binding = {
        constants,
        async_hook_fields,
        async_id_fields,
        async_ids_stack: new Float64Array(32),
        execution_async_resources,
        // AsyncWrap's provider types (async_wrap.h), as Node numbers them.
        Providers: {
          NONE: 0,
          DIRHANDLE: 1,
          DNSCHANNEL: 2,
          ELDHISTOGRAM: 3,
          FILEHANDLE: 4,
          FILEHANDLECLOSEREQ: 5,
          BLOBREADER: 6,
          FSEVENTWRAP: 7,
          FSREQCALLBACK: 8,
          FSREQPROMISE: 9,
          GETADDRINFOREQWRAP: 10,
          GETNAMEINFOREQWRAP: 11,
          HEAPSNAPSHOT: 12,
          HTTP2SESSION: 13,
          HTTP2STREAM: 14,
          HTTP2PING: 15,
          HTTP2SETTINGS: 16,
          HTTPINCOMINGMESSAGE: 17,
          HTTPCLIENTREQUEST: 18,
          LOCKS: 19,
          JSSTREAM: 20,
          JSUDPWRAP: 21,
          MESSAGEPORT: 22,
          PIPECONNECTWRAP: 23,
          PIPESERVERWRAP: 24,
          PIPEWRAP: 25,
          PROCESSWRAP: 26,
          PROMISE: 27,
          QUERYWRAP: 28,
          QUIC_ENDPOINT: 29,
          QUIC_LOGSTREAM: 30,
          QUIC_SESSION: 31,
          QUIC_STREAM: 32,
          QUIC_UDP: 33,
          SHUTDOWNWRAP: 34,
          SIGNALWRAP: 35,
          STATWATCHER: 36,
          STREAMPIPE: 37,
          TCPCONNECTWRAP: 38,
          TCPSERVERWRAP: 39,
          TCPWRAP: 40,
          TTYWRAP: 41,
          UDPSENDWRAP: 42,
          UDPWRAP: 43,
          SIGINTWATCHDOG: 44,
          WORKER: 45,
          WORKERCPUPROFILE: 46,
          WORKERCPUUSAGE: 47,
          WORKERHEAPPROFILE: 48,
          WORKERHEAPSNAPSHOT: 49,
          WORKERHEAPSTATISTICS: 50,
          WRITEWRAP: 51,
          ZLIB: 52,
          CHECKPRIMEREQUEST: 53,
          PBKDF2REQUEST: 54,
          KEYPAIRGENREQUEST: 55,
          KEYGENREQUEST: 56,
          KEYEXPORTREQUEST: 57,
          ARGON2REQUEST: 58,
          CIPHERREQUEST: 59,
          DERIVEBITSREQUEST: 60,
          HASHREQUEST: 61,
          RANDOMBYTESREQUEST: 62,
          RANDOMPRIMEREQUEST: 63,
          SCRYPTREQUEST: 64,
          SIGNREQUEST: 65,
          TLSWRAP: 66,
          VERIFYREQUEST: 67,
        },
        // What the JavaScript calls when async_ids_stack is full: Node grows
        // it natively, and so does this, then does what the JavaScript would.
        pushAsyncContext(asyncId, triggerAsyncId) {
          const offset = async_hook_fields[kStackLength];
          if (offset * 2 >= binding.async_ids_stack.length) {
            const grown = new Float64Array(binding.async_ids_stack.length * 2);
            grown.set(binding.async_ids_stack);
            binding.async_ids_stack = grown;
          }
          binding.async_ids_stack[offset * 2] = async_id_fields[kExecutionAsyncId];
          binding.async_ids_stack[offset * 2 + 1] = async_id_fields[kTriggerAsyncId];
          async_hook_fields[kStackLength] = offset + 1;
          async_id_fields[kExecutionAsyncId] = asyncId;
          async_id_fields[kTriggerAsyncId] = triggerAsyncId;
        },
        // Reached only when the ids no longer match, which Node reports and
        // exits on (AsyncHooks::FailWithCorruptedAsyncStack).
        popAsyncContext(asyncId) {
          ops.write(
            2,
            `Error: async hook stack has become corrupted (actual: ${async_id_fields[kExecutionAsyncId]}, expected: ${asyncId})\n`,
          );
          ops.exit(1);
        },
        executionAsyncResource() {},
        clearAsyncIdStack() {
          async_id_fields[kExecutionAsyncId] = 0;
          async_id_fields[kTriggerAsyncId] = 0;
          async_hook_fields[kStackLength] = 0;
          execution_async_resources.length = 0;
        },
        setupHooks() {},
        setCallbackTrampoline() {},
        queueDestroyAsyncId() {},
        registerDestroyHook() {},
        setPromiseHooks() {},
        getPromiseHooks: () => [undefined, undefined, undefined, undefined],
      };
      return binding;
    },
    // Tracing is never on: every category's enabled flag reads 0.
    trace_events: () => ({
      getCategoryEnabledBuffer: () => new Uint8Array(1),
      trace() {},
      isTraceCategoryEnabled: () => false,
    }),
    // Opal has no permission model (`--permission`), so everything is
    // permitted.
    permission: () => ({ has: () => true }),
    // The synchronous half of Node's fs binding (ops/fs.rs). The callback
    // and promise halves, which need an event loop with I/O, aren't there
    // yet: the binding refuses a call that passes it a request.
    fs: () => {
      const kFsStatsFieldsNumber = 18;
      const statValues = new Float64Array(kFsStatsFieldsNumber * 2);
      const bigintStatValues = new BigInt64Array(kFsStatsFieldsNumber * 2);
      // StatFs's fields (node_file.h's FsStatFsOffset).
      const statFsValues = new Float64Array(8);
      const bigintStatFsValues = new BigInt64Array(8);
      useBuffers();
      ops.fs_setup(statValues, bigintStatValues, statFsValues, bigintStatFsValues);
      const unsupported = (what) =>
        class {
          constructor() {
            const error = new Error(`Opal implements only the synchronous fs functions yet, not ${what}`);
            error.code = "ERR_OPAL_UNIMPLEMENTED";
            throw error;
          }
        };
      const kUsePromises = internalBinding("symbols").fs_use_promises_symbol;
      // What node_file.cc's FileHandle gives lib/internal/fs/promises.js,
      // which wraps it: the descriptor, and closing it (a promise) or
      // giving it up. Its stream methods serve http2 alone.
      let nextAsyncId = 1;
      class FileHandle {
        #fd;
        #asyncId = nextAsyncId++;
        constructor(fd) {
          this.#fd = fd;
        }
        get fd() {
          return this.#fd;
        }
        getAsyncId() {
          return this.#asyncId;
        }
        close() {
          const fd = this.#fd;
          this.#fd = -1;
          return ops.fs_close(fd, kUsePromises);
        }
        releaseFD() {
          const fd = this.#fd;
          this.#fd = -1;
          return fd;
        }
      }
      // node_dir.cc's DirHandle, around an open directory's id.
      class DirHandle {
        #id;
        constructor(id) {
          this.#id = id;
        }
        read(encoding, bufferSize, req) {
          return ops.fs_dir_read(this.#id, encoding, bufferSize, req);
        }
        close(req) {
          return ops.fs_dir_close(this.#id, req);
        }
      }
      ops.fs_handles(FileHandle, DirHandle);
      // The request a callback-form call passes the binding. Making one is
      // fine; the binding refuses it once it has checked the arguments.
      class FSReqCallback {
        constructor(bigint) {
          this.bigint = bigint;
        }
      }
      const binding = {
        kFsStatsFieldsNumber,
        statValues,
        bigintStatValues,
        statFsValues,
        bigintStatFsValues,
        kUsePromises,
        FSReqCallback,
        FileHandle,
        DirHandle,
        StatWatcher: unsupported("fs.watchFile"),
      };
      for (const name of ObjectKeys(ops)) {
        if (
          StringPrototypeStartsWith(name, "fs_") &&
          !ArrayPrototypeIncludes(
            ["fs_setup", "fs_constants", "fs_handles", "fs_opendir", "fs_opendir_sync", "fs_dir_read", "fs_dir_close"],
            name,
          )
        ) {
          binding[camelCase(StringPrototypeSlice(name, 3))] = ops[name];
        }
      }
      return binding;
    },
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
    // DOMException, from Node's own per-context script for it. There are no
    // workers to post messages to, so nothing is deserialized, and Opal has
    // no structuredClone yet.
    messaging: () => {
      const exports = {};
      const perIsolateSymbols = internalBinding("symbols");
      const script = ops.compile_builtin("internal/per_context/domexception", [
        "exports",
        "primordials",
        "privateSymbols",
        "perIsolateSymbols",
      ]);
      script(exports, primordials, privateSymbols, perIsolateSymbols);
      return {
        DOMException: exports.DOMException,
        setDeserializerCreateObjectFunction() {},
        structuredClone() {
          const error = new Error("Opal does not implement structuredClone yet");
          error.code = "ERR_OPAL_UNIMPLEMENTED";
          throw error;
        },
      };
    },
    // src/string_decoder.cc's, ported (ops/string_decoder.rs): the decoder's
    // state is a Buffer of kSize bytes, laid out as the C++ class is.
    string_decoder: () => ({
      // Indexed by Node's `enum encoding` (src/node.h).
      encodings: ["ascii", "utf8", "base64", "utf16le", "latin1", "hex", "buffer", "base64url"],
      kIncompleteCharactersStart: 0,
      kIncompleteCharactersEnd: 4,
      kMissingBytes: 4,
      kBufferedBytes: 5,
      kEncodingField: 6,
      kNumFields: 7,
      kSize: 7,
      decode: ops.string_decoder_decode,
      flush: ops.string_decoder_flush,
    }),
    // fs.opendir's directory handles (node_dir.cc).
    fs_dir: () => ({
      opendir: ops.fs_opendir,
      opendirSync: ops.fs_opendir_sync,
      DirHandle: internalBinding("fs").DirHandle,
    }),
    // src/node_os.cc's, ported with what libuv does for it (ops/os.rs).
    // userInfo's strings can be Buffers.
    os: () => ({
      getHostname: ops.os_hostname,
      getLoadAvg: ops.os_load_avg,
      getUptime: ops.os_uptime,
      getTotalMem: ops.os_total_memory,
      getFreeMem: ops.os_free_memory,
      getCPUs: ops.os_cpus,
      getInterfaceAddresses: ops.os_interface_addresses,
      getHomeDirectory: ops.os_home_directory,
      getUserInfo(options, ctx) {
        useBuffers();
        return ops.os_user_info(options, ctx);
      },
      setPriority: ops.os_set_priority,
      getPriority: ops.os_get_priority,
      getAvailableParallelism: ops.os_available_parallelism,
      getOSInformation: ops.os_information,
      isBigEndian: new Uint8Array(new Uint16Array([1]).buffer)[0] === 0,
    }),
    // src/node_credentials.cc's.
    credentials: () => ({
      safeGetenv: ops.safe_getenv,
      getTempDir: ops.temp_dir,
      implementsPosixCredentials: true,
      getuid: () => ops.credentials()[0],
      geteuid: () => ops.credentials()[1],
      getgid: () => ops.credentials()[2],
      getegid: () => ops.credentials()[3],
      getgroups: () => ops.credentials()[4],
    }),
    // fs.watch needs the event loop to watch files, which it doesn't yet.
    fs_event_wrap: () => ({
      FSEvent: class FSEvent {
        constructor() {
          throw watchUnimplemented();
        }
      },
    }),
  };

  // Node's setBufferPrototype, which its bootstrap calls: done here by the
  // first binding whose natives make Buffers, so that a script that makes
  // none doesn't load node:buffer.
  let buffersUsed = false;
  function useBuffers() {
    if (buffersUsed) return;
    buffersUsed = true;
    ops.set_buffer_prototype(requireBuiltin("buffer").Buffer.prototype);
  }

  function watchUnimplemented() {
    const error = new Error("Opal does not implement fs.watch yet");
    error.code = "ERR_OPAL_UNIMPLEMENTED";
    return error;
  }

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
      // Node's library reached a module of its own that isn't vendored
      // (js/node/FILES) yet, on a code path Opal doesn't support.
      const error = new Error(`Opal does not implement this yet: it needs node:${id}, which Opal doesn't have`);
      error.code = "ERR_OPAL_UNIMPLEMENTED";
      throw error;
    }
    const module = { exports: {}, id, loaded: false };
    modules.set(id, module);
    compiled(module.exports, requireBuiltin, module, proc, internalBinding, primordials);
    module.loaded = true;
    // What Node's startup does with a module once it exists (pre_execution).
    if (id === "internal/util/debuglog") module.exports.initializeDebugEnv(proc.env.NODE_DEBUG);
    // Linux's recursive fs.watch watches each file with fs.watch and ignores
    // what that throws, so without fs.watch it would silently watch nothing.
    if (id === "internal/fs/recursive_watch") {
      const { kFSWatchStart } = requireBuiltin("internal/fs/watchers");
      module.exports.FSWatcher.prototype[kFSWatchStart] = function () {
        throw watchUnimplemented();
      };
    }
    return module.exports;
  }

  // While build.rs makes the snapshot; main.js's start clears it and runs
  // what modules loaded meanwhile asked to run then.
  internals.buildingSnapshot = true;
  internals.deserializeCallbacks = [];

  internals.requireBuiltin = requireBuiltin;
  internals.internalBinding = internalBinding;
  internals.customInspectSymbol = SymbolFor("nodejs.util.inspect.custom");
  // Errors whose stack already says where they came from (a TypeScript
  // error's source excerpt), so the fatal report prints no arrow of its own.
  // A WeakSet rather than a symbol on the error, so inspect doesn't show it.
  internals.noArrow = new WeakSet();

  for (const id of internals.snapshotBuiltins) requireBuiltin(id);
})
