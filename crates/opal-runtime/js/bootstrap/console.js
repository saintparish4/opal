// The global console, writing to fd 1 (log, info, debug, dir, table's
// fallback) and fd 2 (error, warn, trace, assert), one write per call as Node
// does for files and pipes. Messages are util.format'ed by Node's own
// formatWithOptions, then indented by the current group depth.
//
// Strings with no format specifier need no formatting, so they are written
// without loading util.inspect, which then stays out of the snapshot and
// out of a script that only prints text.
(function (internals, ops, primordials) {
  "use strict";

  const {
    ErrorCaptureStackTrace,
    StringPrototypeIncludes,
    StringPrototypeReplaceAll,
    StringPrototypeRepeat,
  } = primordials;
  const inspector = () => internals.requireBuiltin("internal/util/inspect");
  const formatWithOptions = (...args) => inspector().formatWithOptions(...args);
  const inspect = (...args) => inspector().inspect(...args);

  // What formatWithOptions returns for arguments that are all strings and
  // whose first has no `%`: them, joined by spaces. Undefined otherwise.
  function plainText(args) {
    if (args.length === 0) return "";
    if (typeof args[0] !== "string" || (args.length > 1 && StringPrototypeIncludes(args[0], "%"))) {
      return undefined;
    }
    let text = args[0];
    for (let i = 1; i < args.length; i++) {
      if (typeof args[i] !== "string") return undefined;
      text += ` ${args[i]}`;
    }
    return text;
  }

  const kGroupIndentationWidth = 2;
  const kNoColorInspectOptions = {};
  let groupIndent = "";
  const counts = new Map();
  const timers = new Map();

  function write(fd, args) {
    let string = plainText(args) ?? formatWithOptions(kNoColorInspectOptions, ...args);
    if (groupIndent.length !== 0) {
      if (string.includes("\n")) {
        string = StringPrototypeReplaceAll(string, "\n", `\n${groupIndent}`);
      }
      string = groupIndent + string;
    }
    ops.write(fd, `${string}\n`);
  }

  function nowMs() {
    return Number(ops.hrtime()) / 1e6;
  }

  // "1.234ms", "1.234s", "1:02.345 (m:ss.mmm)", as console.timeEnd prints.
  function formatTime(ms) {
    let hours = 0;
    let minutes = 0;
    let seconds = 0;
    if (ms >= 1000) {
      if (ms >= 60000) {
        if (ms >= 3600000) {
          hours = Math.floor(ms / 3600000);
          ms %= 3600000;
        }
        minutes = Math.floor(ms / 60000);
        ms %= 60000;
      }
      seconds = ms / 1000;
    }
    if (hours !== 0 || minutes !== 0) {
      ({ 0: seconds, 1: ms } = seconds.toFixed(3).split("."));
      const res = hours !== 0 ? `${hours}:${pad(minutes)}` : minutes;
      return `${res}:${pad(seconds)}.${ms} (${hours !== 0 ? "h:m" : ""}m:ss.mmm)`;
    }
    if (seconds !== 0) {
      return `${seconds.toFixed(3)}s`;
    }
    return `${Number(ms.toFixed(3))}ms`;
  }
  const pad = (value) => `${value}`.padStart(2, "0");

  function timeLogImpl(name, label, data) {
    const time = timers.get(label);
    if (time === undefined) {
      internals.process.emitWarning(`No such label '${label}' for console.${name}()`);
      return false;
    }
    const duration = nowMs() - time;
    if (data === undefined) {
      write(1, [`${label}: ${formatTime(duration)}`]);
    } else {
      write(1, [`${label}: ${formatTime(duration)}`, ...data]);
    }
    return true;
  }

  const console = {
    log(...args) {
      write(1, args);
    },
    warn(...args) {
      write(2, args);
    },
    dir(object, options) {
      write(1, [inspect(object, { customInspect: false, ...options })]);
    },
    time(label = "default") {
      label = `${label}`;
      if (timers.has(label)) {
        internals.process.emitWarning(`Label '${label}' already exists for console.time()`);
        return;
      }
      timers.set(label, nowMs());
    },
    timeEnd(label = "default") {
      label = `${label}`;
      if (timeLogImpl("timeEnd", label)) timers.delete(label);
    },
    timeLog(label = "default", ...data) {
      label = `${label}`;
      timeLogImpl("timeLog", label, data);
    },
    trace: function trace(...args) {
      const err = { name: "Trace", message: formatWithOptions(kNoColorInspectOptions, ...args) };
      ErrorCaptureStackTrace(err, trace);
      this.error(err.stack);
    },
    assert(expression, ...args) {
      if (!expression) {
        if (typeof args[0] === "string") {
          args[0] = `Assertion failed: ${args[0]}`;
        } else {
          args.unshift("Assertion failed");
        }
        this.warn(...args);
      }
    },
    clear() {},
    count(label = "default") {
      label = `${label}`;
      let count = counts.get(label);
      count = count === undefined ? 1 : count + 1;
      counts.set(label, count);
      write(1, [`${label}: ${count}`]);
    },
    countReset(label = "default") {
      label = `${label}`;
      if (!counts.has(label)) {
        internals.process.emitWarning(`Count for '${label}' does not exist`);
        return;
      }
      counts.delete(label);
    },
    group(...data) {
      if (data.length > 0) {
        this.log(...data);
      }
      groupIndent += StringPrototypeRepeat(" ", kGroupIndentationWidth);
    },
    groupEnd() {
      groupIndent = groupIndent.slice(0, groupIndent.length - kGroupIndentationWidth);
    },
    // A table is not implemented yet; the data is still printed.
    table(data, ...rest) {
      this.log(data, ...rest);
    },
  };
  console.debug = console.log;
  console.info = console.log;
  console.dirxml = console.log;
  console.error = console.warn;
  console.groupCollapsed = console.group;

  // Node's console methods are bound, so `const { log } = console` works.
  for (const key of Object.keys(console)) {
    const method = console[key];
    Object.defineProperty(console, key, {
      value: method.bind(console),
      writable: true,
      enumerable: true,
      configurable: true,
    });
    Object.defineProperty(console[key], "name", { value: key });
  }

  Object.defineProperty(globalThis, "console", {
    value: console,
    writable: true,
    enumerable: false,
    configurable: true,
  });
  internals.console = console;
  internals.consolePlainText = plainText;
  internals.formatTime = formatTime;
})
