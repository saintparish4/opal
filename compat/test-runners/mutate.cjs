// Plants one failing assertion in a test file, for compat/test-runners.py.
//
//   node mutate.cjs < [{"file": "/abs/x.test.js", "max": 3}, ...]
//   => {"/abs/x.test.js": {"candidates": 12, "mutants": [{line, column, matcher, op, source}]}}
//
// Two kinds of candidate, both lexically inside the callback of a `test`/`it`
// (plain, `.only` or `.failing`):
//   - a matcher call on an `expect(...)` chain, where the mutation toggles
//     `.not` right before the matcher: inserted when absent, removed when
//     present. Snapshot matchers are never candidates, since negating them is
//     an error in every runner, not a planted failure, and running them writes
//     files.
//   - the end of the callback's body, where the mutation throws. This one
//     checks only that a runner runs a test's body to its end, awaiting
//     whatever the body awaits, and reports what it throws: Jest runs a
//     generator function as a test, for one, and a runner that only calls it
//     never reaches the throw.
// The assertion flips come first; the throws are there for files with none.
//
// Nothing here decides which test the mutant breaks or whether it breaks one at
// all: the harness runs every mutant under the reference runner and keeps the
// first per file that turns exactly one passing test into a failing one.
//
// Offsets are the parser's, which are JavaScript string indices, so the edit is
// made here and the whole mutated source is returned.
'use strict';

const fs = require('fs');
const path = require('path');
const {parse} = require(path.join(__dirname, 'runners', 'node_modules', '@babel', 'parser'));

const SNAPSHOT_MATCHERS = new Set([
  'toMatchSnapshot',
  'toMatchInlineSnapshot',
  'toThrowErrorMatchingSnapshot',
  'toThrowErrorMatchingInlineSnapshot',
  'toMatchFileSnapshot',
]);
const CHAIN = new Set(['not', 'resolves', 'rejects']);
const TEST_FUNCTIONS = new Set(['test', 'it', 'fit']);
const TEST_MODIFIERS = new Set(['only', 'failing']);

function propertyName(member) {
  return !member.computed && member.property.type === 'Identifier' ? member.property.name : null;
}

// `test`, `it.only`, `test.failing`, `test.only.failing`; never `.each`, `.skip`
// or `.todo`, whose bodies run many times or never.
function isTestCallee(callee) {
  let node = callee;
  while (node.type === 'MemberExpression') {
    if (!TEST_MODIFIERS.has(propertyName(node))) return false;
    node = node.object;
  }
  return node.type === 'Identifier' && TEST_FUNCTIONS.has(node.name);
}

function* walk(node, ancestors) {
  if (!node || typeof node.type !== 'string') return;
  yield [node, ancestors];
  ancestors.push(node);
  for (const key of Object.keys(node)) {
    if (key === 'loc' || key === 'leadingComments' || key === 'trailingComments') continue;
    const value = node[key];
    if (Array.isArray(value)) {
      for (const item of value) yield* walk(item, ancestors);
    } else if (value && typeof value.type === 'string') {
      yield* walk(value, ancestors);
    }
  }
  ancestors.pop();
}

function isTestCallback(node, parent) {
  return (
    (node.type === 'ArrowFunctionExpression' || node.type === 'FunctionExpression') &&
    parent.type === 'CallExpression' &&
    parent.arguments.includes(node) &&
    isTestCallee(parent.callee)
  );
}

function insideTest(ancestors) {
  for (let index = ancestors.length - 1; index > 0; index--) {
    if (isTestCallback(ancestors[index], ancestors[index - 1])) return true;
  }
  return false;
}

function candidates(source, file) {
  const typescript = /\.[cm]?tsx?$/.test(file);
  const ast = parse(source, {
    sourceType: 'unambiguous',
    errorRecovery: true,
    plugins: typescript
      ? ['typescript', 'explicitResourceManagement', 'decorators-legacy']
      : ['jsx', 'explicitResourceManagement'],
  });
  const found = [];
  const throws = [];
  for (const [node, ancestors] of walk(ast.program, [])) {
    const parent = ancestors[ancestors.length - 1];
    if (parent && isTestCallback(node, parent) && node.body.type === 'BlockStatement') {
      throws.push({
        start: node.body.end - 1,
        end: node.body.end - 1,
        text: "\n;throw new Error('planted failure');\n",
        line: node.body.loc.end.line,
        column: node.body.loc.end.column,
        matcher: null,
        op: 'throw at the end of the test',
      });
    }
    if (node.type !== 'CallExpression' || node.callee.type !== 'MemberExpression') continue;
    const matcher = propertyName(node.callee);
    if (!matcher || !/^to[A-Z]/.test(matcher) || SNAPSHOT_MATCHERS.has(matcher)) continue;
    let object = node.callee.object;
    while (object.type === 'MemberExpression' && CHAIN.has(propertyName(object))) {
      object = object.object;
    }
    if (
      object.type !== 'CallExpression' ||
      object.callee.type !== 'Identifier' ||
      object.callee.name !== 'expect' ||
      !insideTest(ancestors)
    ) {
      continue;
    }
    const before = node.callee.object;
    const negated = before.type === 'MemberExpression' && propertyName(before) === 'not';
    found.push({
      start: negated ? before.object.end : before.end,
      end: before.end,
      text: negated ? '' : '.not',
      line: node.callee.property.loc.start.line,
      column: node.callee.property.loc.start.column + 1,
      matcher,
      op: negated ? 'remove .not' : 'insert .not',
    });
  }
  const byStart = (a, b) => a.start - b.start;
  return {flips: found.sort(byStart), throws: throws.sort(byStart)};
}

// Up to `max` candidates spread evenly over the file, in a fixed order, so the
// first one that validates is the same every run.
function spread(list, max) {
  if (list.length <= max) return list;
  const picked = [];
  for (let index = 0; index < max; index++) {
    picked.push(list[Math.floor(((index + 0.5) * list.length) / max)]);
  }
  return picked;
}

const requests = JSON.parse(fs.readFileSync(0, 'utf8'));
const result = {};
for (const {file, max} of requests) {
  const source = fs.readFileSync(file, 'utf8');
  let found;
  try {
    found = candidates(source, file);
  } catch (error) {
    result[file] = {candidates: 0, mutants: [], error: String(error.message)};
    continue;
  }
  result[file] = {
    candidates: found.flips.length + found.throws.length,
    mutants: [...spread(found.flips, max), ...spread(found.throws, max)].map(
      ({start, end, text, line, column, matcher, op}) => ({
      line,
      column,
      matcher,
      op,
      source: source.slice(0, start) + text + source.slice(end),
    })),
  };
}
process.stdout.write(JSON.stringify(result));
