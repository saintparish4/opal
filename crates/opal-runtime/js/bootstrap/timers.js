// setTimeout, setInterval, setImmediate and queueMicrotask.
//
// Timers are a binary heap ordered by due time, then by creation, so timers
// due at the same millisecond run in the order they were set. The event loop
// (engine/mod.rs, through main.js) runs one due timer per call to runTimer
// and drains nextTick and the microtask queue between each, as Node has
// since v11; then the immediates that were queued when the check phase
// began, the same way. A timer or immediate that is unref'd fires if the
// loop is still running for something else, but doesn't keep it running.
(function (internals, ops, primordials) {
  "use strict";


  const TIMEOUT_MAX = 2 ** 31 - 1;

  const heap = [];
  let sequence = 0;
  let nextId = 1;
  let refedTimers = 0;

  function now() {
    return Number(ops.hrtime()) / 1e6;
  }

  // Heap entries are { timer, when, seq }. A timer's current entry is
  // timer._entry; refresh() and an interval's next run add a new entry, and
  // the old one is skipped when it reaches the top.
  function before(a, b) {
    return a.when < b.when || (a.when === b.when && a.seq < b.seq);
  }

  function schedule(timer, when) {
    const entry = { timer, when, seq: sequence++ };
    timer._entry = entry;
    heap.push(entry);
    let i = heap.length - 1;
    while (i > 0) {
      const parent = (i - 1) >> 1;
      if (!before(heap[i], heap[parent])) break;
      [heap[i], heap[parent]] = [heap[parent], heap[i]];
      i = parent;
    }
  }

  function pop() {
    const top = heap[0];
    const last = heap.pop();
    if (heap.length > 0) {
      heap[0] = last;
      let i = 0;
      for (;;) {
        const left = 2 * i + 1;
        const right = left + 1;
        let smallest = i;
        if (left < heap.length && before(heap[left], heap[smallest])) smallest = left;
        if (right < heap.length && before(heap[right], heap[smallest])) smallest = right;
        if (smallest === i) break;
        [heap[i], heap[smallest]] = [heap[smallest], heap[i]];
        i = smallest;
      }
    }
    return top;
  }

  // The earliest entry still current, dropping stale ones off the top.
  function peek() {
    while (heap.length > 0 && (heap[0].timer._destroyed || heap[0].timer._entry !== heap[0])) pop();
    return heap[0];
  }

  function validateCallback(callback) {
    if (typeof callback !== "function") {
      internals.requireBuiltin("internal/validators").validateFunction(callback, "callback");
    }
  }

  function delayOf(after) {
    after *= 1;
    if (!(after >= 1 && after <= TIMEOUT_MAX)) {
      if (after > TIMEOUT_MAX) {
        internals.process.emitWarning(
          `${after} does not fit into a 32-bit signed integer.\nTimeout duration was set to 1.`,
          "TimeoutOverflowWarning",
        );
      }
      after = 1;
    }
    return after;
  }

  class Timeout {
    constructor(callback, after, args, repeat) {
      this._idleTimeout = after;
      this._onTimeout = callback;
      this._timerArgs = args;
      this._repeat = repeat ? after : null;
      this._destroyed = false;
      this._refed = true;
      this._id = nextId++;
      this._entry = null;
      refedTimers++;
      schedule(this, now() + after);
    }

    ref() {
      if (!this._refed) {
        this._refed = true;
        if (!this._destroyed) refedTimers++;
      }
      return this;
    }

    unref() {
      if (this._refed) {
        this._refed = false;
        if (!this._destroyed) refedTimers--;
      }
      return this;
    }

    hasRef() {
      return this._refed;
    }

    refresh() {
      if (!this._destroyed) schedule(this, now() + this._idleTimeout);
      return this;
    }

    close() {
      clearTimeout(this);
      return this;
    }

    [Symbol.toPrimitive]() {
      return this._id;
    }

    [Symbol.dispose]() {
      clearTimeout(this);
    }
  }

  const byId = new Map();

  function setTimeout(callback, after, ...args) {
    validateCallback(callback);
    const timer = new Timeout(callback, delayOf(after), args, false);
    byId.set(timer._id, timer);
    return timer;
  }

  function setInterval(callback, repeat, ...args) {
    validateCallback(callback);
    const timer = new Timeout(callback, delayOf(repeat), args, true);
    byId.set(timer._id, timer);
    return timer;
  }

  function clearTimeout(timer) {
    if (timer == null) return;
    if (!(timer instanceof Timeout)) {
      timer = byId.get(typeof timer === "number" ? timer : Number(timer));
      if (timer === undefined) return;
    }
    if (!timer._destroyed) {
      timer._destroyed = true;
      if (timer._refed) refedTimers--;
      byId.delete(timer._id);
    }
  }

  // Runs the earliest timer if it is due. Returns whether it ran one.
  function runTimer() {
    const entry = peek();
    if (entry === undefined) return false;
    const current = now();
    if (entry.when > current) return false;
    pop();
    const timer = entry.timer;
    if (timer._repeat !== null) {
      schedule(timer, current + timer._repeat);
    } else {
      timer._destroyed = true;
      if (timer._refed) refedTimers--;
      byId.delete(timer._id);
    }
    const callback = timer._onTimeout;
    const args = timer._timerArgs;
    if (args.length === 0) callback.call(timer);
    else callback.apply(timer, args);
    return true;
  }

  class Immediate {
    constructor(callback, args) {
      this._onImmediate = callback;
      this._argv = args;
      this._destroyed = false;
      this._refed = true;
      refedImmediates++;
    }

    ref() {
      if (!this._refed) {
        this._refed = true;
        if (!this._destroyed) refedImmediates++;
      }
      return this;
    }

    unref() {
      if (this._refed) {
        this._refed = false;
        if (!this._destroyed) refedImmediates--;
      }
      return this;
    }

    hasRef() {
      return this._refed;
    }

    [Symbol.dispose]() {
      clearImmediate(this);
    }
  }

  let refedImmediates = 0;
  let immediateQueue = [];
  let checkPhase = [];

  function setImmediate(callback, ...args) {
    validateCallback(callback);
    const immediate = new Immediate(callback, args);
    immediateQueue.push(immediate);
    return immediate;
  }

  function clearImmediate(immediate) {
    if (!(immediate instanceof Immediate) || immediate._destroyed) return;
    immediate._destroyed = true;
    if (immediate._refed) refedImmediates--;
  }

  // The check phase runs what was queued when it began; immediates set by
  // those callbacks wait for the next loop iteration. Left over from a phase
  // an exception interrupted run first.
  function beginCheck() {
    if (immediateQueue.length > 0) {
      checkPhase = checkPhase.length === 0 ? immediateQueue : checkPhase.concat(immediateQueue);
      immediateQueue = [];
    }
  }

  function runImmediate() {
    while (checkPhase.length > 0) {
      const immediate = checkPhase.shift();
      if (immediate._destroyed) continue;
      immediate._destroyed = true;
      if (immediate._refed) refedImmediates--;
      const args = immediate._argv;
      if (args.length === 0) immediate._onImmediate();
      else immediate._onImmediate(...args);
      return true;
    }
    return false;
  }

  // Milliseconds until the loop has something to run: 0 if an immediate is
  // waiting, the earliest timer's due time otherwise, -1 if nothing that is
  // ref'd is pending and the loop should end.
  function nextTimer() {
    if (refedTimers <= 0 && refedImmediates <= 0) return -1;
    if (immediateQueue.length > 0 || checkPhase.length > 0) return 0;
    const entry = peek();
    if (entry === undefined) return -1;
    return Math.max(0, entry.when - now());
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

  function clearInterval(timer) {
    clearTimeout(timer);
  }

  internals.pendingUncaught = [];
  internals.timers = {
    Timeout,
    Immediate,
    setTimeout,
    setInterval,
    setImmediate,
    clearTimeout,
    clearInterval,
    clearImmediate,
    queueMicrotask,
    runTimer,
    beginCheck,
    runImmediate,
    nextTimer,
  };
})
