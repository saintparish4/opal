// setTimeout, setInterval, setImmediate and their clear functions are Node's
// own (lib/timers.js and lib/internal/timers.js, vendored unmodified), loaded
// the first time a script reads one of those globals: they bring
// internal/errors, the validators, util.inspect and async_hooks with them,
// which a script that never uses a timer shouldn't pay for, and in the
// snapshot they would cost every start more than loading them costs once
// (the snapshot nearly doubles). Each global is a getter until then, as
// Node's lazy globals are, so no script can see anything but Node's
// functions.
//
// What Node's C++ does for them is here, as internalBinding('timers'): one
// libuv timer, due when processTimers() says the next timer list is, the
// count of pending immediates, and the loop's millisecond clock. The event
// loop (engine/mod.rs, through main.js) calls runTimers() in its timers
// phase and runImmediates() in its check phase, and calls each again after
// an exception it has reported, as Node's RunTimers and CheckImmediate do,
// so the timers and immediates after the one that threw still run.
(function (internals, ops, primordials) {
  "use strict";

  const { MathAbs, MathFloor, MathMax, MathTrunc, ObjectDefineProperty } = primordials;

  // The loop's clock, as libuv keeps it: whole milliseconds, here since the
  // process started (Node's GetLibuvNow subtracts its timer base the same
  // way).
  function preciseNow() {
    return Number(ops.hrtime() - internals.startNs) / 1e6;
  }
  function getLibuvNow() {
    return MathFloor(preciseNow());
  }

  // Environment::ImmediateInfo's fields (src/env.h).
  const kCount = 0;
  const kRefCount = 1;
  const kHasOutstanding = 2;
  const immediateInfo = new Uint32Array(3);
  // The number of ref'd timers.
  const timeoutInfo = new Int32Array(1);

  // The timer handle: the loop time it is due at (null when stopped), and
  // whether it keeps the loop alive. Both handles start unref'd, as Node's.
  let due = null;
  let timerRefed = false;
  let immediateRefed = false;

  internals.timerBinding = {
    immediateInfo,
    timeoutInfo,
    getLibuvNow,
    // uv_timer_start, which takes whole milliseconds.
    scheduleTimer(duration) {
      due = getLibuvNow() + MathTrunc(duration);
    },
    toggleTimerRef(refed) {
      timerRefed = refed;
    },
    toggleImmediateRef(refed) {
      immediateRefed = refed;
    },
    setupTimers() {},
  };

  let timers;
  let callbacks;

  // Loads Node's timers, once.
  function load() {
    if (timers === undefined) {
      timers = internals.requireBuiltin("timers");
      callbacks = internals.requireBuiltin("internal/timers").getTimerCallbacks(internals.runNextTicks);
    }
    return timers;
  }

  // Defines global `name` (one of lib/timers.js's exports) as a getter that
  // loads Node's timers, and as Node's own value once assigned.
  function defineGlobal(name) {
    ObjectDefineProperty(globalThis, name, {
      __proto__: null,
      enumerable: true,
      configurable: true,
      get() {
        return load()[name];
      },
      set(value) {
        ObjectDefineProperty(globalThis, name, {
          __proto__: null,
          value,
          writable: true,
          enumerable: true,
          configurable: true,
        });
      },
    });
  }

  // The timers phase: Node's processTimers(now), when the timer is due. If
  // it throws, the timer stays due, so the loop's next call carries on with
  // the lists after the one that threw.
  function runTimers() {
    if (due === null) return;
    const now = getLibuvNow();
    if (now < due) return;
    const next = MathTrunc(callbacks.processTimers(now));
    // What Node's RunTimers does with the answer: the next list's expiry,
    // negative when every timer left is unref'd, or 0 when none is left.
    if (next === 0) {
      due = null;
      timerRefed = false;
      return;
    }
    const duration = MathAbs(next) - getLibuvNow();
    due = getLibuvNow() + (duration > 0 ? duration : 1);
    timerRefed = next > 0;
  }

  // The check phase: Node's processImmediate(), when an immediate is
  // pending or an exception interrupted the last one.
  function runImmediates() {
    if (immediateInfo[kCount] === 0 && immediateInfo[kHasOutstanding] === 0) return;
    callbacks.processImmediate();
  }

  // Milliseconds until the loop has something to run: 0 while a ref'd
  // immediate waits, until the timer is due while it is ref'd, and -1 when
  // nothing ref'd is pending and the loop should end.
  function nextTimer() {
    if (immediateRefed && immediateInfo[kRefCount] > 0) return 0;
    if (due !== null && timerRefed) return MathMax(0, due - preciseNow());
    return -1;
  }

  function validateCallback(callback) {
    if (typeof callback !== "function") {
      internals.requireBuiltin("internal/validators").validateFunction(callback, "callback");
    }
  }

  function queueMicrotask(callback) {
    validateCallback(callback);
    Promise.resolve().then(() => {
      try {
        callback();
      } catch (error) {
        // An exception in a microtask is uncaught, not a rejection.
        internals.pendingUncaught.push(error);
      }
    });
  }

  internals.pendingUncaught = [];
  internals.timers = {
    __proto__: null,
    defineGlobal,
    queueMicrotask,
    load,
    runTimers,
    runImmediates,
    nextTimer,
  };
})
