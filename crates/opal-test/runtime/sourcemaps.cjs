// Stack traces that point at the lines the user wrote, not the transformed
// code that ran: each test file's context gets this as its
// Error.prepareStackTrace, as Jest installs source-map-support. Only files
// opal transformed have mappings; every other frame prints as V8 prints it.
'use strict';

// What the loader puts before line 1 of every module (loader.cjs).
// loader.cjs's wrapper, async or not: both are this long.
const WRAPPER_PREFIX = '({"Object.<anonymous>":      function(module,exports,require,__dirname,__filename,jest){';

// filename -> the mappings as they came, until a stack trace needs that
// file; then by line. opal's transform sends `line,column,sourceLine,
// sourceColumn` repeated; a project's transformer gives a source map v3,
// kept as {mappings}.
const registry = new Map();

function register(filename, mappings) {
  if (mappings == null) registry.delete(filename);
  else registry.set(filename, mappings);
}

const BASE64 = new Map([...'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'].map((c, i) => [c, i]));

// A v3 map's mappings: `;` between generated lines, `,` between segments,
// each segment's fields base64 VLQs relative to the last segment's (the
// column to the last on the same line). Only segments with a source
// position count.
function decodeV3(mappings, byLine) {
  let sourceLine = 0;
  let sourceColumn = 0;
  let line = 0;
  for (const text of mappings.split(';')) {
    let column = 0;
    let list = null;
    for (const segment of text.split(',')) {
      if (!segment) continue;
      const fields = [];
      let value = 0;
      let shift = 0;
      for (const c of segment) {
        const digit = BASE64.get(c);
        value += (digit & 31) << shift;
        if (digit & 32) {
          shift += 5;
        } else {
          fields.push(value & 1 ? -(value >>> 1) : value >>> 1);
          value = 0;
          shift = 0;
        }
      }
      column += fields[0];
      if (fields.length < 4) continue;
      sourceLine += fields[2];
      sourceColumn += fields[3];
      if (!list) byLine.set(line, (list = []));
      list.push([column, sourceLine, sourceColumn]);
    }
    if (list) list.sort((a, b) => a[0] - b[0]);
    line++;
  }
}

function byLineOf(filename) {
  const entry = registry.get(filename);
  if (entry instanceof Map || entry === undefined) return entry;
  // line -> [[column, sourceLine, sourceColumn], ...], in column order.
  const byLine = new Map();
  if (typeof entry === 'object') {
    decodeV3(entry.mappings, byLine);
  } else {
    const numbers = entry ? entry.split(',').map(Number) : [];
    for (let i = 0; i + 3 < numbers.length; i += 4) {
      let list = byLine.get(numbers[i]);
      if (!list) byLine.set(numbers[i], (list = []));
      list.push([numbers[i + 1], numbers[i + 2], numbers[i + 3]]);
    }
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
