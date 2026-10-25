// What the ported Jest packages (jest-*.cjs) import from packages opal
// doesn't port whole: chalk, jest-util and jest-message-util. Only the
// names they use are here.
'use strict';

const {ErrorWithStack, isPromise} = require('./errors.cjs');

// chalk. opal prints matcher messages without color; a test that snapshots
// one gets the same text either way, since Jest's own snapshots of matcher
// output are taken with colors off.
// It is chalk at level 0: every style, chained or called with arguments
// (`red.bold`, `rgb(1, 2, 3).bgRgb(...)`), returns its text as is.
const colors = new Proxy(function chalk(...args) {
  return args.join(' ');
}, {
  apply: (_target, _this, args) => args.join(' '),
  get: (target, name) => {
    if (name === 'level') return 0;
    if (name === 'supportsColor') return false;
    if (typeof name === 'symbol' || name in Function.prototype) return target[name];
    return colors;
  },
});

// jest-util
const pluralize = (word, count, ending = 's') => `${count} ${word}${count === 1 ? '' : ending}`;

// jest-message-util
const errorRegexp = /^Error:?\s*$/;
const removeBlankErrorLine = str =>
  str
    .split('\n')
    .filter(line => !errorRegexp.test(line))
    .join('\n')
    .trimEnd();

function separateMessageFromStack(content) {
  if (!content) return {message: '', stack: ''};
  const messageMatch = content.match(/^(?:Error: )?([\S\s]*?(?=\n\s*at\s.*:\d*:\d*)|\s*.*)([\S\s]*)$/);
  return {message: removeBlankErrorLine(messageMatch[1]), stack: removeBlankErrorLine(messageMatch[2])};
}

// Frames from Node's internals and from opal's runtime say nothing about the
// test, as Jest leaves out its own.
const RUNTIME_DIR = __dirname;
const isUserFrame = line => !line.includes('node:internal') && !line.includes(RUNTIME_DIR);

function formatStackTrace(stack, _config, options = {}) {
  if (options.noStackTrace) return '';
  return stack
    .split('\n')
    .filter(line => line.trim() && isUserFrame(line))
    .map(line => `      ${line.trim()}`)
    .join('\n');
}

function formatExecError(error) {
  const {message, stack} = separateMessageFromStack(error && error.stack ? error.stack : String(error));
  return `${message}\n${formatStackTrace(stack)}`;
}

// The runtime's own frames are Jest-internal frames to the code that looks
// for the test's frame.
const isJestInternalFrame = line => line.includes(RUNTIME_DIR);
const PATH_NODE_MODULES = `${require('path').sep}node_modules${require('path').sep}`;
const STACK_LINE = /^\s*at (?:(.+?) \()?(.+?):(\d+):(\d+)\)?$/;

function getStackTraceLines(stack) {
  return stack
    .split('\n')
    .filter(line => line && !/^\s+at (?:new )?Promise \(<anonymous>\)/.test(line) && !line.includes('node:internal'));
}

// The first frame outside node_modules and the runtime: where the test
// called the matcher.
function getTopFrame(lines) {
  for (const line of lines) {
    if (line.includes(PATH_NODE_MODULES) || isJestInternalFrame(line)) continue;
    const match = line.trim().match(STACK_LINE);
    if (match) {
      let file = match[2];
      if (file.startsWith('file://')) file = require('url').fileURLToPath(file);
      return {column: Number(match[4]), file, function: match[1], line: Number(match[3])};
    }
  }
  return null;
}

const isError = value => {
  switch (Object.prototype.toString.call(value)) {
    case '[object Error]':
    case '[object Exception]':
    case '[object DOMException]':
      return true;
    default:
      return value instanceof Error;
  }
};

const interopRequireDefault = obj => (obj && obj.__esModule ? obj : {default: obj});

// @jest/transform's, for a custom snapshotResolver: loaded as Node loads it.
const createTranspilingRequire = () => async (resolverPath, applyInteropRequireDefault) => {
  const loaded = require(resolverPath);
  return applyInteropRequireDefault ? interopRequireDefault(loaded).default : loaded;
};

module.exports = {
  bgYellow: colors,
  bold: colors,
  dim: colors,
  green: colors,
  inverse: colors,
  red: colors,
  yellow: colors,
  createTranspilingRequire,
  getStackTraceLines,
  getTopFrame,
  interopRequireDefault,
  isError,
  default: colors,
  ErrorWithStack,
  formatExecError,
  formatStackTrace,
  isPromise,
  pluralize,
  separateMessageFromStack,
};
