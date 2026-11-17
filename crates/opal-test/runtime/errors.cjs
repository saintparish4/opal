// Small pieces of jest-util and jest-message-util that the framework's
// messages depend on, ported as they are.
'use strict';

class ErrorWithStack extends Error {
  constructor(message, callsite, stackLimit) {
    const originalStackLimit = Error.stackTraceLimit;
    if (stackLimit) {
      Error.stackTraceLimit = Math.max(stackLimit, originalStackLimit || 10);
    }
    super(message);
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, callsite);
    }
    Error.stackTraceLimit = originalStackLimit;
  }
}

function convertDescriptorToString(descriptor) {
  switch (typeof descriptor) {
    case 'function':
      if (descriptor.name) return descriptor.name;
      break;
    case 'number':
    case 'undefined':
      return `${descriptor}`;
    case 'string':
      return descriptor;
  }
  throw new Error(
    `Invalid first argument, ${descriptor}. It must be a named class, named function, number, or string.`,
  );
}

function formatTime(time, prefixPower = -3, padLeftLength = 0) {
  const prefixes = ['n', 'μ', 'm', ''];
  const prefixIndex = Math.max(
    0,
    Math.min(Math.trunc(prefixPower / 3) + prefixes.length - 1, prefixes.length - 1),
  );
  return `${String(time).padStart(padLeftLength)} ${prefixes[prefixIndex]}s`;
}

const isPromise = candidate =>
  !!candidate &&
  (typeof candidate === 'object' || typeof candidate === 'function') &&
  typeof candidate.then === 'function';

const isErrorLike = value =>
  value != null && typeof value === 'object' && (typeof value.message === 'string' || typeof value.stack === 'string');

// jest-message-util's flattenErrorStack: the stack, then each cause and each
// of an AggregateError's errors, recursively.
function flattenErrorStack(errorOrStack, seen = new Set()) {
  if (typeof errorOrStack === 'string') return errorOrStack;
  const {message, stack} = errorOrStack;
  let flattened;
  if (typeof stack === 'string' && stack !== '') {
    flattened = message && !stack.includes(message) ? message + stack.replace(/^Error:?\s*\n/, '\n') : stack;
  } else {
    flattened = message;
  }
  seen.add(errorOrStack);
  const cause = errorOrStack.cause;
  if (cause !== undefined && (typeof cause === 'string' || isErrorLike(cause))) {
    flattened += seen.has(cause)
      ? '\n\n[cause]: [Circular]'
      : `\n\n[cause]: ${flattenErrorStack(cause, seen)}`;
  }
  if (Array.isArray(errorOrStack.errors)) {
    for (const error of errorOrStack.errors) {
      const nested = typeof error === 'string' || isErrorLike(error) ? error : String(error);
      flattened += seen.has(nested) ? '\n\n[errors]: [Circular]' : `\n\n[errors]: ${flattenErrorStack(nested, seen)}`;
    }
  }
  seen.delete(errorOrStack);
  return flattened;
}

module.exports = {ErrorWithStack, convertDescriptorToString, flattenErrorStack, formatTime, isPromise};
