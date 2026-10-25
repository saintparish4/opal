// Stack traces that point at the lines the user wrote, not the transformed
// code that ran: each test file's context gets this as its
// Error.prepareStackTrace, as Jest installs source-map-support. Only files
// opal transformed have mappings; every other frame prints as V8 prints it.
'use strict';

// What the loader puts before line 1 of every module (loader.cjs).
const WRAPPER_PREFIX = '({"Object.<anonymous>":function(module,exports,require,__dirname,__filename,jest){';

const registry = new Map();

function register(filename, mappings) {
  // By line, for lookup: line -> [[column, sourceLine, sourceColumn], ...].
  const byLine = new Map();
  for (const [line, column, sourceLine, sourceColumn] of mappings) {
    let list = byLine.get(line);
    if (!list) byLine.set(line, (list = []));
    list.push([column, sourceLine, sourceColumn]);
  }
  registry.set(filename, byLine);
}

// 1-based in and out, as stack traces count.
function originalPosition(filename, line, column) {
  const byLine = registry.get(filename);
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
