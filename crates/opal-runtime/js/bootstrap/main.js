// The entry points engine/mod.rs calls, left in the snapshot's context data
// rather than on any global a script could reach:
//
//   start(argv, argv0)   set up process for this run and run the main module
//   drain()              nextTick callbacks and microtasks until both are
//                        empty; returns [error] for an exception from a
//                        microtask or a rejection nobody handled
//   runTimers()          the timers phase: the due timers (timers.js)
//   runImmediates()      the check phase: the pending immediates (timers.js)
//   nextTimer()          ms until something can run, -1 if nothing is pending
//   beforeExit()         emit 'beforeExit'; true if a listener scheduled work
//   exit()               emit 'exit'; returns the exit code
//   uncaught(error)      true if an 'uncaughtException' listener took it
//   initializeImportMeta, importDynamic   V8's import.meta and import() hooks
//   hasArrow(error)      false for an error whose stack shows its own source
//   rethrown(error)      true if drain's error was thrown on from runNextTicks
//   prepareStackTrace    V8's hook for formatting error.stack
//   inspectFatal(error)  what Node prints for an error that ends the process
//   fatalExit()          exit code 1, emit 'exit'; returns the exit code
//   warmup()             run at build time only, so compiled code is kept
(function (internals, ops, primordials) {
  "use strict";

  const { ObjectIs } = primordials;

  const process = internals.process;
  const tickQueue = internals.tickQueue;
  const runTicks = internals.runTicks;
  const pendingUncaught = internals.pendingUncaught;
  const timers = internals.timers;
  const inspect = (...args) => internals.requireBuiltin("internal/util/inspect").inspect(...args);
  const isError = (value) => internals.requireBuiltin("internal/util").isError(value);
  const emitter = internals.processEmitter;
  const listeners = (event) => (emitter.isEmitter() ? process.listenerCount(event) : 0);

  // The globals Node adds to V8's, in the order Node adds them (its
  // Object.keys(globalThis) shows it), each enumerable or not as in Node.
  // Buffer, atob, btoa and DOMException load their modules on first use,
  // so a script that never touches them doesn't pay for compiling them.
  const { defineLazyProperties } = internals.internalBinding("util");
  const define = (name, value, enumerable) =>
    Object.defineProperty(globalThis, name, { value, writable: true, enumerable, configurable: true });
  define("global", globalThis, true);
  for (const name of ["clearImmediate", "setImmediate", "clearInterval", "clearTimeout", "setInterval", "setTimeout"]) {
    timers.defineGlobal(name);
  }
  define("queueMicrotask", timers.queueMicrotask, true);
  defineLazyProperties(globalThis, "buffer", ["atob", "btoa"], true);
  defineLazyProperties(globalThis, "buffer", ["Buffer"], false);
  defineLazyProperties(globalThis, "internal/opal/dom_exception", ["DOMException"], false);

  // What Node does when a promise is rejected and nothing handles it, by
  // default (--unhandled-rejections=throw): 'unhandledRejection' listeners
  // get it, and if there are none it becomes an uncaught exception. A reason
  // that is not an error is wrapped in one, saying where it came from.
  function unhandledRejectionError(reason) {
    if (isError(reason)) return reason;
    let shown;
    try {
      shown = inspect(reason);
    } catch {
      shown = String(reason);
    }
    const error = new Error(
      "This error originated either by throwing inside of an async function without a catch block, " +
        `or by rejecting a promise which was not handled with .catch(). The promise rejected with the reason "${shown}".`,
    );
    error.name = "UnhandledPromiseRejection";
    error.code = "ERR_UNHANDLED_REJECTION";
    return error;
  }

  let formattingStack = false;
  function defaultStack(error, trace) {
    let header;
    try {
      header = Error.prototype.toString.call(error);
    } catch {
      header = "<error>";
    }
    return trace.length === 0 ? header : `${header}\n    at ${trace.join("\n    at ")}`;
  }

  const main = {
    start(argv, argv0) {
      internals.buildingSnapshot = false;
      for (const [callback, data] of internals.deserializeCallbacks) callback(data);
      internals.deserializeCallbacks.length = 0;
      internals.processSetup(argv, argv0);
      internals.loader.runMain(argv[1]);
    },

    drain() {
      for (;;) {
        runTicks();
        ops.run_microtasks();
        if (pendingUncaught.length > 0) return [pendingUncaught.shift()];
        if (!tickQueue.isEmpty()) continue;
        const rejected = ops.take_unhandled_rejection();
        if (rejected === undefined) return undefined;
        const { 0: reason, 1: promise } = rejected;
        if (listeners("unhandledRejection") > 0) {
          process.emit("unhandledRejection", reason, promise);
          continue;
        }
        return [unhandledRejectionError(reason)];
      }
    },

    runTimers: timers.runTimers,
    runImmediates: timers.runImmediates,
    nextTimer: timers.nextTimer,

    beforeExit() {
      if (process._exiting || listeners("beforeExit") === 0) return false;
      process.emit("beforeExit", process.exitCode || 0);
      return timers.nextTimer() >= 0 || !tickQueue.isEmpty();
    },

    exit() {
      if (!process._exiting) {
        process._exiting = true;
        if (emitter.isEmitter()) process.emit("exit", process.exitCode || 0);
      }
      return process.exitCode || 0;
    },

    uncaught(error) {
      if (listeners("uncaughtException") === 0) return false;
      process.emit("uncaughtException", error, "uncaughtException");
      return true;
    },

    initializeImportMeta(meta, url) {
      internals.esm.initializeImportMeta(meta, url);
    },

    importDynamic(specifier, referrer, type) {
      return internals.esm.importDynamic(specifier, referrer, type);
    },

    // An error drain() returned and runNextTicks threw on, so that it
    // interrupts the timers or immediates as in Node. Its report is made
    // from the error itself, as drain()'s are, not from where it was thrown
    // on.
    rethrown(error) {
      const was = rethrownError !== noError && ObjectIs(error, rethrownError);
      rethrownError = noError;
      return was;
    },

    hasArrow(error) {
      return !(error !== null && typeof error === "object" && internals.noArrow.has(error));
    },

    // V8's prepare-stack-trace hook, installed by engine/mod.rs: Node's own
    // (internal/errors.js), which writes Node errors as `Name [CODE]: ...`
    // and honours a user's Error.prepareStackTrace. Loading internal/errors
    // can itself format a stack; while it loads, V8's default stands in.
    prepareStackTrace(global, error, trace) {
      if (formattingStack) return defaultStack(error, trace);
      formattingStack = true;
      try {
        return internals.requireBuiltin("internal/errors").prepareStackTraceCallback(global, error, trace);
      } finally {
        formattingStack = false;
      }
    },

    inspectFatal(error) {
      try {
        return inspect(error);
      } catch {
        return String(error);
      }
    },

    fatalExit() {
      process.exitCode = 1;
      if (!process._exiting) {
        process._exiting = true;
        if (emitter.isEmitter()) process.emit("exit", 1);
      }
      return process.exitCode || 1;
    },

    // Run at build time so that the compiled code of what every start runs
    // is in the snapshot. Only what needs no built-in module: a module
    // loaded here would be deserialized on every start.
    warmup() {
      internals.consolePlainText(["warm", "up"]);
      timers.nextTimer();
    },
  };

  // What Node's timers call between timer lists and between immediates
  // (lib/internal/process/task_queues.js): the ticks and microtasks, and an
  // uncaught exception or unhandled rejection they leave thrown on.
  const noError = {};
  let rethrownError = noError;
  internals.runNextTicks = function runNextTicks() {
    const failed = main.drain();
    if (failed !== undefined) {
      rethrownError = failed[0];
      throw failed[0];
    }
  };

  internals.main = main;
})
