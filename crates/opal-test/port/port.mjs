// Ports Jest's `expect` and the packages under it into opal test's runtime.
//
//   node crates/opal-test/port/port.mjs JEST_CHECKOUT           write runtime/jest-*.cjs
//   node crates/opal-test/port/port.mjs JEST_CHECKOUT --check   verify them
//
// JEST_CHECKOUT is a clone of github.com/jestjs/jest at v30.5.2. Each file
// listed below has its TypeScript types removed by Node's own stripper, and
// its ES module syntax rewritten to CommonJS the way Babel's CommonJS
// transform rewrites it: an imported binding becomes a property read on the
// required module wherever it is referenced, so a cycle between two modules
// (asymmetricMatchers and jestMatchersObject import each other) behaves as
// it does under ES modules. Nothing else in the code is touched: what runs
// is Jest's own logic, which is the point, and test/equals-property.cjs
// holds the result to Jest's own build anyway.
//
// Dependencies outside the list are mapped onto opal's runtime: chalk to a
// shim with no colors, pretty-format to format.cjs, jest-util and
// jest-message-util to the few functions used, from support.cjs.
//
// Needs @babel/parser and @babel/traverse; the oracle's pinned install in
// compat/test-runners/runners has both.
import fs from 'node:fs';
import path from 'node:path';
import {createRequire, stripTypeScriptTypes} from 'node:module';
import {fileURLToPath} from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const RUNTIME = path.join(HERE, '..', 'runtime');
const require = createRequire(path.join(HERE, '..', '..', '..', 'compat', 'test-runners', 'runners', 'package.json'));
const {parse} = require('@babel/parser');
const traverse = require('@babel/traverse').default;

const TAG = 'v30.5.2';

// package folder under jest/packages -> [module prefix, source files]
const PACKAGES = {
  'expect': ['jest-expect', ['index', 'asymmetricMatchers', 'extractExpectedAssertionsErrors', 'jestMatchersObject', 'matchers', 'print', 'spyMatchers', 'toThrowMatchers']],
  'expect-utils': ['jest-expect-utils', ['index', 'immutableUtils', 'jasmineUtils', 'utils']],
  'jest-matcher-utils': ['jest-matcher-utils', ['index', 'Replaceable', 'deepCyclicCopyReplaceable']],
  'jest-diff': ['jest-diff', ['index', 'cleanupSemantic', 'constants', 'diffLines', 'diffStrings', 'escapeControlCharacters', 'getAlignedDiffs', 'joinAlignedDiffs', 'normalizeDiffOptions', 'printDiffs']],
  'diff-sequences': ['jest-diff-sequences', ['index']],
  'jest-get-type': ['jest-get-type', ['index']],
  // InlineSnapshots.ts parses and reprints the test file with Babel and
  // prettier, which opal's runtime doesn't carry; inline-snapshots.cjs
  // writes them instead.
  'jest-snapshot': ['jest-snapshot', ['index', 'State', 'utils', 'printSnapshot', 'dedentLines', 'mockSerializer', 'plugins', 'colors', 'SnapshotResolver']],
  'jest-snapshot-utils': ['jest-snapfile', ['index', 'utils', 'types']],
  'jest-expect': ['jest-expect-snapshot', ['index']],
  'jest-mock': ['jest-mock', ['index']],
};

// Bare specifiers -> runtime module.
const EXTERNAL = {
  '@jest/expect-utils': 'jest-expect-utils',
  '@jest/get-type': 'jest-get-type',
  '@jest/diff-sequences': 'jest-diff-sequences',
  'jest-matcher-utils': 'jest-matcher-utils',
  'jest-diff': 'jest-diff',
  '@jest/snapshot-utils': 'jest-snapfile',
  'expect': 'jest-expect',
  'jest-snapshot': 'jest-snapshot',
  'natural-compare': 'natural-compare',
  '@jest/transform': 'support',
  'pretty-format': 'format',
  'chalk': 'support',
  'jest-util': 'support',
  'jest-message-util': 'support',
};

// Node's own modules, required from the host as they are.
const HOST = new Set(['graceful-fs', 'node:path', 'node:fs', 'path', 'fs']);
// A sibling replaced by a hand-written module, or a package that one
// package reaches through a wrapper: jest-snapshot writes snapshot files
// through snapfile-dialect.cjs, which knows Vitest's header and keys.
const REPLACED = {
  'jest-snapshot/./InlineSnapshots': 'inline-snapshots',
  'jest-snapshot/@jest/snapshot-utils': 'snapfile-dialect',
};

const moduleName = (prefix, file) => (file === 'index' ? prefix : `${prefix}-${file}`);

export function outputs() {
  return Object.values(PACKAGES).flatMap(([prefix, files]) => files.map(file => `${moduleName(prefix, file)}.cjs`));
}

function convert(source, pkg, file) {
  const [prefix] = PACKAGES[pkg];
  const ast = parse(source, {sourceType: 'module', plugins: ['jsx']});
  const edits = [];
  const replace = (start, end, text) => edits.push({start, end, text});
  const headerLines = [];
  const exportLines = [];
  const reexportLines = [];
  let counter = 0;

  const target = specifier => {
    if (REPLACED[`${pkg}/${specifier}`]) return `./${REPLACED[`${pkg}/${specifier}`]}.cjs`;
    if (HOST.has(specifier)) return specifier === 'graceful-fs' ? 'fs' : specifier;
    if (specifier.startsWith('./')) return `./${moduleName(prefix, specifier.slice(2))}.cjs`;
    const mapped = EXTERNAL[specifier];
    if (!mapped) throw new Error(`${pkg}/${file}: no mapping for '${specifier}'`);
    return `./${mapped}.cjs`;
  };

  // Imported bindings first, so an `export {name}` of one can point at it.
  const importedAccess = new Map();
  const removed = [];
  const local = name => importedAccess.get(name) ?? name;

  traverse(ast, {
    Program(programPath) {
      for (const statement of programPath.get('body')) {
        const node = statement.node;
        if (node.type === 'ImportDeclaration') {
          const ns = `_m${counter++}`;
          headerLines.push(`const ${ns} = require(${JSON.stringify(target(node.source.value))});`);
          for (const specifier of node.specifiers) {
            const local = specifier.local.name;
            let access;
            if (specifier.type === 'ImportNamespaceSpecifier') access = ns;
            else if (specifier.type === 'ImportDefaultSpecifier') access = `(${ns}.default ?? ${ns})`;
            else {
              const imported = specifier.imported.type === 'StringLiteral' ? specifier.imported.value : specifier.imported.name;
              access = `${ns}.${imported}`;
            }
            importedAccess.set(local, access);
            const binding = statement.scope.getBinding(local);
            for (const reference of binding ? binding.referencePaths : []) {
              const parent = reference.parent;
              if (parent.type === 'ObjectProperty' && parent.shorthand && parent.value === reference.node) {
                replace(reference.node.start, reference.node.end, `${local}: ${access}`);
              } else {
                replace(reference.node.start, reference.node.end, access);
              }
            }
          }
          removed.push([node.start, node.end]);
          replace(node.start, node.end, '');
        }
      }
      for (const statement of programPath.get('body')) {
        const node = statement.node;
        if (node.type === 'ExportNamedDeclaration') {
          if (node.source) {
            const ns = `_m${counter++}`;
            headerLines.push(`const ${ns} = require(${JSON.stringify(target(node.source.value))});`);
            for (const specifier of node.specifiers) {
              reexportLines.push(`Object.defineProperty(exports, ${JSON.stringify(specifier.exported.name)}, {enumerable: true, get: () => ${ns}.${specifier.local.name}});`);
            }
            replace(node.start, node.end, '');
          } else if (node.declaration) {
            const declaration = node.declaration;
            const names = declaration.type === 'VariableDeclaration'
              ? declaration.declarations.flatMap(d => Object.keys(statement.get('declaration').getBindingIdentifiers()))
              : [declaration.id.name];
            for (const name of new Set(names)) {
              exportLines.push(`Object.defineProperty(exports, ${JSON.stringify(name)}, {enumerable: true, get: () => ${name}});`);
            }
            replace(node.start, declaration.start, '');
          } else {
            for (const specifier of node.specifiers) {
              exportLines.push(`Object.defineProperty(exports, ${JSON.stringify(specifier.exported.name)}, {enumerable: true, get: () => ${local(specifier.local.name)}});`);
            }
            removed.push([node.start, node.end]);
            replace(node.start, node.end, '');
          }
        } else if (node.type === 'ExportDefaultDeclaration') {
          const declaration = node.declaration;
          if ((declaration.type === 'FunctionDeclaration' || declaration.type === 'ClassDeclaration') && declaration.id) {
            replace(node.start, declaration.start, '');
            exportLines.push(`Object.defineProperty(exports, 'default', {enumerable: true, get: () => ${declaration.id.name}});`);
          } else {
            replace(node.start, declaration.start, 'const _default = ');
            exportLines.push(`Object.defineProperty(exports, 'default', {enumerable: true, get: () => _default});`);
          }
        } else if (node.type === 'ExportAllDeclaration') {
          const ns = `_m${counter++}`;
          headerLines.push(`const ${ns} = require(${JSON.stringify(target(node.source.value))});`);
          reexportLines.push(`for (const key of Object.keys(${ns})) if (key !== 'default' && !Object.prototype.hasOwnProperty.call(exports, key)) Object.defineProperty(exports, key, {enumerable: true, get: () => ${ns}[key]});`);
          replace(node.start, node.end, '');
        }
      }
      programPath.stop();
    },
  });

  // A reference inside a statement that is removed whole goes with it.
  const kept = edits.filter(
    edit => edit.text === '' || !removed.some(([start, end]) => edit.start >= start && edit.end <= end),
  );
  edits.length = 0;
  edits.push(...kept);
  edits.sort((a, b) => b.start - a.start);
  let out = source;
  for (const {start, end, text} of edits) out = out.slice(0, start) + text + out.slice(end);
  // The module's own exports first, as getters: a module that requires this
  // one while it is still being evaluated sees the bindings, as an ES module
  // would. Re-exports need the module they come from, so they follow the
  // requires.
  return `${exportLines.join('\n')}\n${headerLines.join('\n')}\n${reexportLines.join('\n')}\n${out}`;
}

function header(pkg, file) {
  return [
    `// Ported from jest/packages/${pkg}/src/${file}.ts at Jest ${TAG} by`,
    '// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to',
    '// CommonJS, nothing else changed. Do not edit by hand; edit the port.',
    '//',
    '// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in',
    '// compat/test-runners/LICENSES/jest.txt.',
    "'use strict';",
    '',
  ].join('\n');
}

function main() {
  const [checkout, ...flags] = process.argv.slice(2);
  if (!checkout) {
    console.error('usage: node port.mjs JEST_CHECKOUT [--check]');
    process.exit(2);
  }
  const check = flags.includes('--check');
  const problems = [];
  for (const [pkg, [prefix, files]] of Object.entries(PACKAGES)) {
    for (const file of files) {
      const sourcePath = path.join(checkout, 'packages', pkg, 'src', `${file}.ts`);
      const typescript = fs.readFileSync(sourcePath, 'utf8');
      // Parameter properties (asymmetricMatchers.ts) need transform mode;
      // everything else is plain stripping.
      const mode = /constructor\([^)]*\b(?:protected|private|public|readonly)\s/.test(typescript) ? 'transform' : 'strip';
      const javascript = stripTypeScriptTypes(typescript, {mode, sourceMap: false});
      const output = header(pkg, file) + convert(javascript, pkg, file);
      const target = path.join(RUNTIME, `${moduleName(prefix, file)}.cjs`);
      if (check) {
        if (!fs.existsSync(target) || fs.readFileSync(target, 'utf8') !== output) problems.push(path.basename(target));
      } else {
        fs.writeFileSync(target, output);
      }
    }
  }
  if (check) {
    console.log(problems.length ? `out of date: ${problems.join(', ')}` : `${outputs().length} ported files match ${TAG}`);
    process.exit(problems.length ? 1 : 0);
  }
  console.log(`wrote ${outputs().length} files into ${path.relative(process.cwd(), RUNTIME)}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
