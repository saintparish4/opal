// What the ported Jest packages (jest-*.cjs) import from packages opal
// doesn't port whole: chalk, jest-util and jest-message-util. Only the
// names they use are here.
'use strict';

const {ErrorWithStack, isPromise} = require('./errors.cjs');

// chalk. opal prints matcher messages without color; a test that snapshots
// one gets the same text either way, since Jest's own snapshots of matcher
// output are taken with colors off.
const plain = (...args) => args.join(' ');
const colors = {bgYellow: plain, bold: plain, dim: plain, green: plain, inverse: plain, red: plain, yellow: plain};

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

module.exports = {
  ...colors,
  default: colors,
  ErrorWithStack,
  formatExecError,
  formatStackTrace,
  isPromise,
  pluralize,
  separateMessageFromStack,
};
