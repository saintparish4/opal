// The global `process`: realm.js made the object, this gives it its methods,
// following Node's lib/internal/bootstrap/node.js and
// lib/internal/process/per_thread.js (whose fragments are marked where they
// are copied), with Node's own errors and validators so that what a bad
// argument throws is what Node throws. What belongs to one run (argv, env,
// pid) is filled in by `setup`, called from main.js's start.
//
// Nothing here loads a built-in module while the snapshot is made, because
// every module in the snapshot is deserialized on every start (build.rs).
// So `process` becomes an EventEmitter on first use: its prototype starts
// as a stand-in carrying EventEmitter's method names, and the first call to
// one (or the first require('events')) loads Node's events module and gives
// process EventEmitter's prototype for good. Until then nothing can be
// listening, which is what main.js relies on when it decides whether to
// emit 'exit'. Node's errors and validators load the first time an argument
// is wrong.
(function (internals, ops, primordials) {
  "use strict";

  const {
    ArrayIsArray,
    BigInt,
    FunctionPrototypeCall,
    NumberIsInteger,
    NumberIsNaN,
    ObjectCreate,
    ObjectDefineProperty,
    ObjectGetPrototypeOf,
    ObjectPrototype,
    ObjectSetPrototypeOf,
    ReflectApply,
    String,
  } = primordials;

  const require = internals.requireBuiltin;
  const errors = () => require("internal/errors").codes;
  const validators = () => require("internal/validators");
  const FixedQueue = require("internal/fixed_queue");

  const EMITTER_METHODS = [
    "addListener",
    "emit",
    "eventNames",
    "getMaxListeners",
    "listenerCount",
    "listeners",
    "off",
    "on",
    "once",
    "prependListener",
    "prependOnceListener",
    "rawListeners",
    "removeAllListeners",
    "removeListener",
    "setMaxListeners",
  ];

  // Makes `object` (whose prototype is `proto`) an EventEmitter the first
  // time one of EventEmitter's methods is called on it. `ready` runs once,
  // after, for what the object needs set up as an emitter.
  function lazyEmitter(object, proto, ready) {
    let done = false;
    const become = () => {
      if (done) return;
      done = true;
      const EventEmitter = require("events");
      ObjectSetPrototypeOf(proto, EventEmitter.prototype);
      FunctionPrototypeCall(EventEmitter, object);
      if (ready) ready();
    };
    const standIn = ObjectCreate(ObjectPrototype);
    for (const name of EMITTER_METHODS) {
      standIn[name] = function (...args) {
        become();
        return ReflectApply(object[name], this, args);
      };
    }
    ObjectSetPrototypeOf(proto, standIn);
    return { become, isEmitter: () => done };
  }

  const process = internals.process;
  const processEmitter = lazyEmitter(process, ObjectGetPrototypeOf(process), () => {
    process.on("warning", require("internal/process/warning").onWarning);
  });
  internals.processEmitter = processEmitter;

  process.title = "opal";
  process.execArgv = [];
  process.config = { target_defaults: {}, variables: {} };
  process.features = { inspector: false, ipv6: true, tls: false, typescript: "strip" };

  // process._exiting and process.exitCode, from lib/internal/bootstrap/node.js.
  let exiting = false;
  let exitCode;
  ObjectDefineProperty(process, "_exiting", {
    __proto__: null,
    get() {
      return exiting;
    },
    set(value) {
      exiting = !!value;
    },
    enumerable: true,
    configurable: true,
  });
  ObjectDefineProperty(process, "exitCode", {
    __proto__: null,
    get() {
      return exitCode;
    },
    set(code) {
      if (code !== null && code !== undefined) {
        let value = code;
        if (typeof code === "string" && code !== "" && NumberIsNaN((value = Number(code)))) {
          value = code;
        }
        if (!NumberIsInteger(value)) validators().validateInteger(value, "code");
        exitCode = value;
      } else {
        exitCode = undefined;
      }
    },
    enumerable: true,
    configurable: false,
  });

  // From lib/internal/process/per_thread.js.
  process.exit = function exit(code) {
    if (arguments.length !== 0) {
      process.exitCode = code;
    }
    if (!process._exiting) {
      process._exiting = true;
      if (processEmitter.isEmitter()) process.emit("exit", process.exitCode || 0);
    }
    process.reallyExit(process.exitCode || 0);
  };
  process.reallyExit = function reallyExit(code) {
    ops.exit(code | 0);
  };

  process.cwd = function cwd() {
    return ops.cwd();
  };

  // The process's user and group ids (POSIX only, as in Node).
  process.getuid = function getuid() {
    return ops.credentials()[0];
  };
  process.geteuid = function geteuid() {
    return ops.credentials()[1];
  };
  process.getgid = function getgid() {
    return ops.credentials()[2];
  };
  process.getegid = function getegid() {
    return ops.credentials()[3];
  };
  process.getgroups = function getgroups() {
    return ops.credentials()[4];
  };

  // process.umask and process.chdir, from
  // lib/internal/bootstrap/switches/does_own_process_state.js.
  process.umask = function umask(mask) {
    if (mask !== undefined) {
      mask = require("internal/validators").parseFileMode(mask, "mask");
    }
    return ops.umask(mask);
  };
  process.chdir = function chdir(directory) {
    require("internal/validators").validateString(directory, "directory");
    ops.chdir(directory);
  };

  // process.hrtime, from lib/internal/process/per_thread.js, reading the
  // monotonic clock as nanoseconds.
  process.hrtime = function hrtime(time) {
    const now = ops.hrtime();
    const sec = Number(now / 1000000000n);
    const nsec = Number(now % 1000000000n);
    if (time !== undefined) {
      if (!ArrayIsArray(time)) {
        throw new (errors().ERR_INVALID_ARG_TYPE)("time", "Array", time);
      }
      if (time.length !== 2) {
        throw new (errors().ERR_OUT_OF_RANGE)("time", 2, time.length);
      }
      const secDiff = sec - time[0];
      const nsecDiff = nsec - time[1];
      return [nsecDiff < 0 ? secDiff - 1 : secDiff, nsecDiff < 0 ? nsecDiff + 1e9 : nsecDiff];
    }
    return [sec, nsec];
  };
  process.hrtime.bigint = function hrtimeBigInt() {
    return ops.hrtime();
  };

  let startNs = BigInt(0);
  process.uptime = function uptime() {
    return Number(ops.hrtime() - startNs) / 1e9;
  };

  // process.nextTick's queue, a FixedQueue as in lib/internal/process/
  // task_queues.js, drained by main.js before each microtask checkpoint so
  // that ticks run before promise callbacks.
  const tickQueue = new FixedQueue();
  process.nextTick = function nextTick(callback) {
    if (typeof callback !== "function") validators().validateFunction(callback, "callback");
    if (process._exiting) return;
    let args;
    switch (arguments.length) {
      case 1:
        break;
      case 2:
        args = [arguments[1]];
        break;
      case 3:
        args = [arguments[1], arguments[2]];
        break;
      case 4:
        args = [arguments[1], arguments[2], arguments[3]];
        break;
      default:
        args = new Array(arguments.length - 1);
        for (let i = 1; i < arguments.length; i++) args[i - 1] = arguments[i];
    }
    tickQueue.push({ callback, args });
  };
  function runTicks() {
    while (!tickQueue.isEmpty()) {
      const { callback, args } = tickQueue.shift();
      if (args === undefined) callback();
      else ReflectApply(callback, undefined, args);
    }
  }

  process.emitWarning = function emitWarning(...args) {
    processEmitter.become();
    return ReflectApply(require("internal/process/warning").emitWarning, this, args);
  };

  process._rawDebug = function _rawDebug(...args) {
    ops.write(2, `${ReflectApply(require("internal/util/inspect").format, undefined, args)}\n`);
  };

  // process.stdout and process.stderr: writes go straight to the fd, one
  // write per call, as Node's do for files and pipes. They are emitters on
  // first use too.
  function makeStdioStream(fd) {
    function WriteStream() {}
    const stream = new WriteStream();
    lazyEmitter(stream, WriteStream.prototype);
    stream.fd = fd;
    stream._isStdio = true;
    stream.writable = true;
    stream.write = function write(chunk, encoding, callback) {
      if (typeof encoding === "function") {
        callback = encoding;
      }
      if (typeof chunk !== "string" && !ArrayBuffer.isView(chunk)) {
        throw new (errors().ERR_INVALID_ARG_TYPE)("chunk", ["string", "Buffer", "TypedArray", "DataView"], chunk);
      }
      ops.write(fd, chunk);
      if (typeof callback === "function") process.nextTick(callback, null);
      return true;
    };
    stream.end = function end(chunk, encoding, callback) {
      if (typeof chunk === "function") {
        callback = chunk;
        chunk = undefined;
      }
      if (chunk !== undefined && chunk !== null) stream.write(chunk, encoding);
      if (typeof callback === "function") process.nextTick(callback);
      return stream;
    };
    stream.cork = () => {};
    stream.uncork = () => {};
    stream.setDefaultEncoding = () => stream;
    stream.getColorDepth = () => 1;
    stream.hasColors = () => false;
    ObjectDefineProperty(stream, "isTTY", {
      __proto__: null,
      enumerable: true,
      configurable: true,
      get() {
        const value = ops.isatty(fd);
        ObjectDefineProperty(stream, "isTTY", {
          __proto__: null,
          value,
          enumerable: true,
          configurable: true,
          writable: true,
        });
        return value;
      },
    });
    return stream;
  }
  for (const [name, fd] of [
    ["stdout", 1],
    ["stderr", 2],
  ]) {
    ObjectDefineProperty(process, name, {
      __proto__: null,
      enumerable: true,
      configurable: true,
      get() {
        const stream = makeStdioStream(fd);
        ObjectDefineProperty(process, name, {
          __proto__: null,
          value: stream,
          enumerable: true,
          configurable: true,
          writable: true,
        });
        return stream;
      },
    });
  }

  // Values are strings, as in Node: assigning a number stores "1".
  function makeEnv(store) {
    return new Proxy(store, {
      set(target, key, value) {
        if (typeof key === "symbol") {
          throw new TypeError("Cannot convert a Symbol value to a string");
        }
        target[key] = String(value);
        return true;
      },
      defineProperty(target, key, descriptor) {
        if (!("value" in descriptor) || descriptor.get || descriptor.set) {
          const error = new TypeError(
            "'process.env' only accepts a configurable, writable, and enumerable data descriptor",
          );
          error.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
          throw error;
        }
        target[key] = String(descriptor.value);
        return true;
      },
    });
  }

  // Fills in what belongs to this run. Called once, before the main module.
  function setup(argv, argv0) {
    startNs = ops.hrtime();
    const [pid, ppid] = ops.process_info();
    process.pid = pid;
    process.ppid = ppid;
    process.argv = argv;
    process.argv0 = argv0;
    process.execPath = argv[0];
    process.env = makeEnv(ops.env());
  }

  ObjectDefineProperty(globalThis, "process", {
    __proto__: null,
    value: process,
    writable: true,
    enumerable: false,
    configurable: true,
  });

  internals.processSetup = setup;
  internals.tickQueue = tickQueue;
  internals.runTicks = runTicks;
})
