// Writing inline snapshots back into a test file: what jest-snapshot's
// InlineSnapshots.ts does, without Babel.
//
// Jest parses the file, finds the matcher call at the frame the snapshot was
// taken from (the line and column of the matcher's name), replaces its first
// template or string argument with the snapshot, or appends one, and
// reprints that call with @babel/generator. opal makes the same edit to the
// text: it finds the call at the same frame, walks its arguments with a
// scanner that knows strings, templates, comments and regular expressions,
// and splices the snapshot in. Everything else in the call stays as written.
// Where Babel would have reprinted it differently (it normalizes spacing
// inside the call), the bytes differ; SEMANTICS.md says so.
//
// Jest then formats the file with the project's prettier when it has one.
// opal doesn't yet.
'use strict';

const fs = require('fs');
const {escapeBacktickString} = require('./jest-snapfile.cjs');

const REGEX_PRECEDERS = new Set(['(', ',', '=', ':', '[', '!', '&', '|', '?', '{', '}', ';', '+', '-', '*', '%', '<', '>', '~', '^']);

// Index just past the string, template, comment or regex starting at `i`,
// or -1 if nothing of the kind starts there.
function skipToken(source, i, previous) {
  const c = source[i];
  if (c === '"' || c === "'") {
    for (let j = i + 1; j < source.length; j++) {
      if (source[j] === '\\') j++;
      else if (source[j] === c) return j + 1;
    }
    return source.length;
  }
  if (c === '`') return skipTemplate(source, i + 1);
  if (c === '/' && source[i + 1] === '/') {
    const end = source.indexOf('\n', i);
    return end < 0 ? source.length : end;
  }
  if (c === '/' && source[i + 1] === '*') {
    const end = source.indexOf('*/', i + 2);
    return end < 0 ? source.length : end + 2;
  }
  if (c === '/' && (previous === undefined || REGEX_PRECEDERS.has(previous))) {
    let inClass = false;
    for (let j = i + 1; j < source.length; j++) {
      if (source[j] === '\\') j++;
      else if (source[j] === '[') inClass = true;
      else if (source[j] === ']') inClass = false;
      else if (source[j] === '/' && !inClass) {
        let k = j + 1;
        while (/[a-z]/i.test(source[k] || '')) k++;
        return k;
      }
      else if (source[j] === '\n') return -1;
    }
  }
  return -1;
}

function skipTemplate(source, i) {
  for (let j = i; j < source.length; j++) {
    if (source[j] === '\\') j++;
    else if (source[j] === '`') return j + 1;
    else if (source[j] === '$' && source[j + 1] === '{') {
      j = skipBalanced(source, j + 2, '}') - 1;
    }
  }
  return source.length;
}

// From just inside an opening bracket to just past its closing `close`.
function skipBalanced(source, i, close) {
  let depth = 0;
  let previous;
  for (let j = i; j < source.length; j++) {
    const c = source[j];
    const skipped = skipToken(source, j, previous);
    if (skipped > j) {
      j = skipped - 1;
      previous = 'x';
      continue;
    }
    if (c === '(' || c === '[' || c === '{') depth++;
    else if (c === ')' || c === ']' || c === '}') {
      if (depth === 0 && c === close) return j + 1;
      depth--;
    }
    if (!/\s/.test(c)) previous = c;
  }
  return source.length;
}

// The argument list of the call whose callee's property starts at `offset`.
function findCall(source, offset) {
  const name = source.slice(offset).match(/^[A-Za-z_$][\w$]*/);
  if (!name) return null;
  let i = offset + name[0].length;
  for (;;) {
    while (/\s/.test(source[i] || '')) i++;
    const skipped = source[i] === '/' ? skipToken(source, i, '.') : -1;
    if (skipped > i && (source[i + 1] === '/' || source[i + 1] === '*')) {
      i = skipped;
      continue;
    }
    break;
  }
  if (source[i] !== '(') return null;
  const open = i;
  const args = [];
  let depth = 0;
  let start = open + 1;
  let previous = '(';
  for (let j = open + 1; j < source.length; j++) {
    const c = source[j];
    const skipped = skipToken(source, j, previous);
    if (skipped > j) {
      j = skipped - 1;
      previous = 'x';
      continue;
    }
    if (c === '(' || c === '[' || c === '{') depth++;
    else if (c === ')' || c === ']' || c === '}') {
      if (depth === 0) {
        if (source.slice(start, j).trim()) args.push(argument(source, start, j));
        return {args, close: j, open};
      }
      depth--;
    } else if (c === ',' && depth === 0) {
      args.push(argument(source, start, j));
      start = j + 1;
    }
    if (!/\s/.test(c)) previous = c;
  }
  return null;
}

function argument(source, start, end) {
  const text = source.slice(start, end);
  const lead = text.length - text.trimStart().length;
  const trimmed = text.trim();
  return {
    end: start + lead + trimmed.length,
    isSnapshot: trimmed.startsWith('`') || trimmed.startsWith("'") || trimmed.startsWith('"'),
    start: start + lead,
  };
}

function saveInlineSnapshots(snapshots) {
  const byFile = new Map();
  for (const snapshot of snapshots) {
    const list = byFile.get(snapshot.frame.file) || [];
    list.push(snapshot);
    byFile.set(snapshot.frame.file, list);
  }
  for (const [file, list] of byFile) {
    const source = fs.readFileSync(file, 'utf8');
    const lineStarts = [0];
    for (let i = 0; i < source.length; i++) if (source[i] === '\n') lineStarts.push(i + 1);
    const seen = new Set();
    const edits = [];
    for (const {frame, snapshot} of list) {
      const key = `${frame.line}:${frame.column}`;
      if (seen.has(key)) throw new Error('Jest: Multiple inline snapshots for the same call are not supported.');
      seen.add(key);
      const call = findCall(source, lineStarts[frame.line - 1] + frame.column - 1);
      if (!call) throw new Error("Jest: Couldn't locate all inline snapshots.");
      const literal = `\`${escapeBacktickString(snapshot)}\``;
      const existing = call.args.find(arg => arg.isSnapshot);
      if (existing) edits.push({end: existing.end, start: existing.start, text: literal});
      else if (call.args.length === 0) edits.push({end: call.close, start: call.open + 1, text: literal});
      else {
        const last = call.args.at(-1);
        edits.push({end: last.end, start: last.end, text: `, ${literal}`});
      }
    }
    edits.sort((a, b) => b.start - a.start);
    let updated = source;
    for (const {start, end, text} of edits) updated = updated.slice(0, start) + text + updated.slice(end);
    if (updated !== source) fs.writeFileSync(file, updated);
  }
}

module.exports = {saveInlineSnapshots};
