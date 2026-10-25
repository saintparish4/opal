// Ports the part of Vitest that reads a test's arguments and names it into
// opal test's runtime: @vitest/runner's parseArguments, how test.each and
// test.for turn a row into a title (formatTitle and formatTemplateString),
// and the printf and loupe formatting under it (@vitest/utils's display and
// helpers).
//
//   node crates/opal-test/port/port-vitest.mjs           write runtime/vitest-display.cjs
//   node crates/opal-test/port/port-vitest.mjs --check   verify it
//
// The source is the oracle's pinned install, compat/test-runners/runners:
// Vitest's own published build, so the code opal runs is the code the
// oracle's Vitest ran. Whole top-level declarations are copied as they are,
// and nothing inside them changes; the module's imports and exports are
// replaced by one module.exports. Left out is what needs @vitest/pretty-format
// (stringify, for browserFormat's prettifyObject), which a title never
// reaches; stringify throws if it ever is.
//
// Needs @babel/parser, from the same pinned install.
import fs from 'node:fs';
import path from 'node:path';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const RUNNERS = path.join(HERE, '..', '..', '..', 'compat', 'test-runners', 'runners');
const TARGET = path.join(HERE, '..', 'runtime', 'vitest-display.cjs');
const require = createRequire(path.join(RUNNERS, 'package.json'));
const {parse} = require('@babel/parser');

// [file in the install, the declarations to copy (null: all but EXCLUDED)]
const SOURCES = [
  ['@vitest/utils/dist/helpers.js', ['isNegativeNaN', 'isObject', 'objectAttr', 'toArray']],
  ['@vitest/utils/dist/display.js', null],
  ['@vitest/runner/dist/chunk-artifact.js', ['runner', 'parseArguments', 'formatName', 'formatTitle', 'handleRegexMatch', 'formatTemplateString']],
];
const EXCLUDED = new Set([
  'AsymmetricMatcher',
  'DOMCollection',
  'DOMElement',
  'Immutable',
  'ReactElement',
  'ReactTestComponent',
  'PLUGINS',
  'stringify',
  'createNodeFilterFromSelector',
  'browserFormat',
]);
const EXPORTS = ['format', 'formatName', 'formatTemplateString', 'formatTitle', 'inspect', 'objDisplay', 'parseArguments', 'toArray'];

function declaredNames(statement) {
  if (statement.type === 'FunctionDeclaration' || statement.type === 'ClassDeclaration') return [statement.id.name];
  if (statement.type !== 'VariableDeclaration') return [];
  const names = [];
  const collect = pattern => {
    if (pattern.type === 'Identifier') names.push(pattern.name);
    else if (pattern.type === 'ObjectPattern') for (const property of pattern.properties) collect(property.value ?? property.argument);
    else if (pattern.type === 'ArrayPattern') for (const element of pattern.elements) element && collect(element);
  };
  for (const declarator of statement.declarations) collect(declarator.id);
  return names;
}

function copy(relative, wanted) {
  const file = path.join(RUNNERS, 'node_modules', relative);
  const source = fs.readFileSync(file, 'utf8');
  const ast = parse(source, {sourceType: 'module'});
  const found = new Set();
  const parts = [];
  for (const statement of ast.program.body) {
    if (statement.type === 'ImportDeclaration' || statement.type.startsWith('Export')) continue;
    const names = declaredNames(statement);
    const keep = wanted ? names.some(name => wanted.includes(name)) : names.length > 0 && !names.some(name => EXCLUDED.has(name));
    if (!keep) continue;
    names.forEach(name => found.add(name));
    // The comments directly above a declaration go with it.
    const start = statement.leadingComments?.[0]?.start ?? statement.start;
    parts.push(source.slice(start, statement.end));
  }
  const missing = (wanted || []).filter(name => !found.has(name));
  if (missing.length > 0) throw new Error(`${relative} has no ${missing.join(', ')}`);
  return `// --- ${relative} ---\n\n${parts.join('\n')}\n`;
}

function render() {
  const version = JSON.parse(fs.readFileSync(path.join(RUNNERS, 'node_modules', 'vitest', 'package.json'), 'utf8')).version;
  return [
    `// Ported from Vitest ${version}'s published @vitest/utils and @vitest/runner`,
    '// (with loupe, which @vitest/utils bundles) by',
    '// crates/opal-test/port/port-vitest.mjs: declarations copied as they are.',
    '// Do not edit by hand; edit the port.',
    '//',
    '// MIT licenses, in compat/test-runners/LICENSES/vitest.txt and loupe.txt.',
    "'use strict';",
    '',
    ...SOURCES.map(([relative, wanted]) => copy(relative, wanted)),
    "function stringify() {\n  throw new Error('opal test: formatting with pretty-format (prettifyObject) is not ported.');\n}",
    '',
    `module.exports = {${EXPORTS.join(', ')}};`,
    '',
  ].join('\n');
}

const output = render();
if (process.argv.includes('--check')) {
  const current = fs.existsSync(TARGET) ? fs.readFileSync(TARGET, 'utf8') : '';
  console.log(current === output ? 'vitest-display.cjs matches the pinned Vitest' : 'out of date: vitest-display.cjs');
  process.exit(current === output ? 0 : 1);
}
fs.writeFileSync(TARGET, output);
console.log(`wrote ${path.relative(process.cwd(), TARGET)}`);
