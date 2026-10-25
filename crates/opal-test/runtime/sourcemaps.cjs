// Stack traces that point at the lines the user wrote, not the transformed
// code that ran: each test file's context gets this as its
// Error.prepareStackTrace, as Jest installs source-map-support. Only files
// opal transformed have mappings; every other frame prints as V8 prints it.
'use strict';

// What the loader puts before line 1 of every module (loader.cjs).
// loader.cjs's wrapper, async or not: both are this long.
const WRAPPER_PREFIX = '({"Object.<anonymous>":      function(module,exports,require,__dirname,__filename,jest){';

// filename -> the mappings as opal sent them, `line,column,sourceLine,
// sourceColumn` repeated, until a stack trace needs that file; then by line.
const registry = new Map();

function register(filename, mappings) {
  registry.set(filename, mappings);
}

function byLineOf(filename) {
  const entry = registry.get(filename);
  if (typeof entry !== 'string') return entry;
  // line -> [[column, sourceLine, sourceColumn], ...], in column order.
  const byLine = new Map();
  const numbers = entry ? entry.split(',').map(Number) : [];
  for (let i = 0; i + 3 < numbers.length; i += 4) {
    let list = byLine.get(numbers[i]);
    if (!list) byLine.set(numbers[i], (list = []));
    list.push([numbers[i + 1], numbers[i + 2], numbers[i + 3]]);
  }
  registry.set(filename, byLine);
  return byLine;
}

// 1-based in and out, as stack traces count.
function originalPosition(filename, line, column) {
  const byLine = byLineOf(filename);
  if (!byLine) return null;
  let generatedColumn = column - 1;
  if (line === 1) generatedColumn -= WRAPPER_PREFIX.length;
  const list = byLine.get(line - 1);
  if (!list || list.length === 0) return null;
  let best = list[0];
  for (const entry of list) {
    if (entry[0] <= generatedColumn) best = entry;
    else break;
  }
  return {column: best[2] + 1, line: best[1] + 1};
}

const FRAME = /^(.*?)(\(?)([^()\s]+):(\d+):(\d+)(\)?)$/;

function mapFrame(text) {
  const match = text.match(FRAME);
  if (!match) return text;
  const [, before, open, file, line, column, close] = match;
  const position = originalPosition(file, Number(line), Number(column));
  return position ? `${before}${open}${file}:${position.line}:${position.column}${close}` : text;
}

function prepareStackTrace(error, callsites) {
  let header;
  try {
    header = Error.prototype.toString.call(error);
  } catch {
    header = '<error>';
  }
  return header + callsites.map(callsite => `\n    at ${mapFrame(String(callsite))}`).join('');
}

module.exports = {originalPosition, prepareStackTrace, register};
