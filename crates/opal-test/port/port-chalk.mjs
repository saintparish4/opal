// Ports chalk 4, the version Jest 30's matcher messages and diffs are colored
// with, into opal test's runtime, with the packages it reads: ansi-styles and
// color-convert (with color-name) for the codes, supports-color (with
// has-flag) for whether to use them.
//
//   node crates/opal-test/port/port-chalk.mjs           write runtime/chalk*.cjs and the rest
//   node crates/opal-test/port/port-chalk.mjs --check   verify them
//
// The source is the oracle's pinned install, compat/test-runners/runners: the
// copies jest-matcher-utils itself resolves. Each file is copied as it is,
// with only its requires pointed at the other ported files. Inside a worker,
// supports-color then decides as it does inside a Jest worker: stdout is a
// pipe, so colors are on only when FORCE_COLOR says so, and opal's pool sets
// FORCE_COLOR the way jest-worker does (src/pool.rs). Left out is chalk's
// tagged-template support, which Jest never uses; it throws if reached.
import fs from 'node:fs';
import path from 'node:path';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const RUNNERS = path.join(HERE, '..', '..', '..', 'compat', 'test-runners', 'runners');
const RUNTIME = path.join(HERE, '..', 'runtime');
const require = createRequire(path.join(RUNNERS, 'package.json'));

const matcherUtils = path.dirname(require.resolve('jest-matcher-utils/package.json'));
const chalkDir = path.dirname(require.resolve('chalk/package.json', {paths: [matcherUtils]}));
const resolveFrom = (dir, name) => path.dirname(require.resolve(`${name}/package.json`, {paths: [dir]}));
const ansiStylesDir = resolveFrom(chalkDir, 'ansi-styles');
const supportsColorDir = resolveFrom(chalkDir, 'supports-color');
const hasFlagDir = resolveFrom(supportsColorDir, 'has-flag');
const colorConvertDir = resolveFrom(ansiStylesDir, 'color-convert');
const colorNameDir = resolveFrom(colorConvertDir, 'color-name');

// [source, runtime file, package]
const FILES = [
  [path.join(chalkDir, 'source', 'index.js'), 'chalk.cjs', chalkDir],
  [path.join(chalkDir, 'source', 'util.js'), 'chalk-util.cjs', chalkDir],
  [path.join(ansiStylesDir, 'index.js'), 'ansi-styles.cjs', ansiStylesDir],
  [path.join(supportsColorDir, 'index.js'), 'supports-color.cjs', supportsColorDir],
  [path.join(hasFlagDir, 'index.js'), 'has-flag.cjs', hasFlagDir],
  [path.join(colorConvertDir, 'index.js'), 'color-convert.cjs', colorConvertDir],
  [path.join(colorConvertDir, 'conversions.js'), 'color-convert-conversions.cjs', colorConvertDir],
  [path.join(colorConvertDir, 'route.js'), 'color-convert-route.cjs', colorConvertDir],
  [path.join(colorNameDir, 'index.js'), 'color-name.cjs', colorNameDir],
];

// What each require becomes.
const REQUIRES = {
  'ansi-styles': './ansi-styles.cjs',
  'supports-color': './supports-color.cjs',
  'has-flag': './has-flag.cjs',
  './util': './chalk-util.cjs',
  './templates': './chalk-unported.cjs',
  'color-convert': './color-convert.cjs',
  './conversions': './color-convert-conversions.cjs',
  './route': './color-convert-route.cjs',
  'color-name': './color-name.cjs',
  os: 'os',
  tty: 'tty',
};

function render([source, target, packageDir]) {
  const manifest = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'));
  // LF throughout, as git stores it (color-name ships with CRLF).
  const code = fs.readFileSync(source, 'utf8').replaceAll('\r\n', '\n').replace(/require\('([^']+)'\)/g, (whole, request) => {
    if (!(request in REQUIRES)) throw new Error(`${source}: unexpected require('${request}')`);
    return `require('${REQUIRES[request]}')`;
  });
  const relative = path.relative(path.join(packageDir, '..'), source);
  return [
    `// Copied from ${manifest.name} ${manifest.version} (${relative}) by`,
    '// crates/opal-test/port/port-chalk.mjs, with its requires pointed at the',
    '// other ported files. Do not edit by hand; edit the port.',
    '//',
    `// ${manifest.license} license, in compat/test-runners/LICENSES/${manifest.name}.txt.`,
    code.startsWith("'use strict'") ? '' : "'use strict';",
    code,
  ]
    .filter(line => line !== '')
    .join('\n');
}

const outputs = FILES.map(file => [file[1], render(file)]);
if (process.argv.includes('--check')) {
  const stale = outputs.filter(([name, text]) => {
    const target = path.join(RUNTIME, name);
    return !fs.existsSync(target) || fs.readFileSync(target, 'utf8') !== text;
  });
  console.log(stale.length ? `out of date: ${stale.map(([name]) => name).join(', ')}` : `${outputs.length} chalk files match the pinned install`);
  process.exit(stale.length ? 1 : 0);
}
for (const [name, text] of outputs) fs.writeFileSync(path.join(RUNTIME, name), text);
// The licenses go with the code.
for (const [, , packageDir] of FILES) {
  const manifest = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'));
  const license = fs.readdirSync(packageDir).find(name => /^licen[sc]e/i.test(name));
  if (license) {
    const text = fs.readFileSync(path.join(packageDir, license), 'utf8').replaceAll('\r\n', '\n');
    fs.writeFileSync(path.join(RUNNERS, '..', 'LICENSES', `${manifest.name}.txt`), text);
  }
}
console.log(`wrote ${outputs.map(([name]) => name).join(', ')}`);
