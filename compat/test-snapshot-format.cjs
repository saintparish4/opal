#!/usr/bin/env node
// How opal test prints a value into a snapshot, held to Jest's on random values.
//
//   node compat/test-snapshot-format.cjs [--runs N] [--seed N] [--against vitest]
//
// A snapshot file round-trips only if every value prints to the same bytes.
// Every value test-expect.cjs's generator makes, plus a fixed list of the
// ones it doesn't (mock functions, errors with causes, functions, classes,
// awkward strings, typed arrays, URLs), is serialized the way jest-snapshot
// serializes a snapshot: Jest's pretty-format with the snapshot plugins,
// escapeRegex, no function names, and Jest 30's snapshotFormat defaults. Jest's
// installed build does it in one test context and opal's port in another, each
// building the values from its own built-ins, and the two strings have to be
// identical.
//
// --against vitest compares opal with Vitest's serializer instead
// (@vitest/snapshot's, through @vitest/pretty-format, with Vitest's defaults,
// in this realm), to measure the distance rather than to close it: a Vitest
// project's committed snapshot reads back in opal only where the two print a
// value alike. What differs is printed, for COMPATIBILITY.md.
'use strict';

const path = require('path');
const vm = require('vm');
const {pathToFileURL} = require('url');
const {GENERATOR, RUNNERS, loadInContext} = require('./test-expect.cjs');

const RUNTIME = path.join(__dirname, '..', 'crates', 'opal-test', 'runtime');
const {createEnvironment} = require(path.join(RUNTIME, 'environment.cjs'));
const {frameworkRequire} = require(path.join(RUNTIME, 'runner.cjs'));

// Values the generator doesn't make, built in each realm with that side's
// own mock functions.
const EXTRAS = String.raw`
(function (fn, spyOn) {
  class Foo { constructor() { this.a = 1; } }
  const owner = {method() { return 1; }};
  const spied = spyOn(owner, 'method');
  spied();
  const called = fn();
  called(1, 'a');
  const named = fn().mockName('named');
  const returning = fn(() => 2);
  returning();
  return [
    function named() {}, () => {}, class K {}, async function a() {}, function* g() {},
    Symbol('s'), Symbol(), -0, 1n, NaN, new Foo(), Object.create(null), Object.create({inherited: 1}),
    new Error('m', {cause: new TypeError('c')}), new AggregateError([new Error('x')], 'agg'),
    Object.assign(new Error('p'), {code: 'E1'}), new RangeError('r'), new Error(''),
    called, named, returning, fn(), spied,
    'multi\nline', 'back\x60tick \${x}', '\\backslash', '"quoted"', 'tab\there', '\u001b[31mred', /re[g]ex\//gi, /\\/,
    new Map([[{a: 1}, [1, 2]]]), new Set([new Set([1])]), new Map(), new Set(), new WeakMap(), new WeakSet(), Promise.resolve(1),
    [1, [2, [3, [4, [5, [6]]]]]], {a: {b: {c: {d: {e: {f: {g: 1}}}}}}}, [[]], [{}], {'': 1, 'a b': 2, 'é': 3, 1: 4},
    Buffer.from('ab'), new Int16Array([1, -1]), new ArrayBuffer(2), new DataView(new ArrayBuffer(1)), new BigInt64Array(1),
    new Date(0), new Date(NaN), new URL('https://a.b/c?d#e'), new URLSearchParams('a=1'),
    (function () { return arguments; })(1, 2), {[Symbol('k')]: 1, a: undefined}, {toJSON() { return 'json'; }}, {toJSON: () => ({x: 1})},
    Object.defineProperty({}, 'getter', {get: () => 1, enumerable: true}), new Proxy({a: 1}, {}),
  ];
})`;

const SNAPSHOT_FORMAT = {escapeString: false, printBasicPrototype: false};
const normalizeNewlines = string => string.replace(/\r\n|\r/g, '\n');

function jestSide() {
  const env = createEnvironment({testEnvironmentOptions: {}});
  const resolve = (request, from = RUNNERS) => require.resolve(request, {paths: [from]});
  const snapshotEntry = resolve('jest-snapshot');
  const {getSerializers} = loadInContext(env.context, snapshotEntry);
  const {format} = loadInContext(env.context, resolve('pretty-format', path.dirname(snapshotEntry)));
  const {ModuleMocker} = loadInContext(env.context, resolve('jest-mock'));
  const {expect} = loadInContext(env.context, resolve('expect'));
  const global = vm.runInContext('globalThis', env.context);
  const mocker = new ModuleMocker(global);
  return {
    context: env.context,
    expect,
    fn: implementation => mocker.fn(implementation),
    spyOn: (object, method) => mocker.spyOn(object, method),
    serialize: value =>
      normalizeNewlines(
        format(value, {escapeRegex: true, indent: 2, plugins: getSerializers(), printFunctionName: false, ...SNAPSHOT_FORMAT}),
      ),
  };
}

// opal's side, in the dialect of the side it is held to: the dialect
// decides only which mocks a file gets.
function opalSide(dialect) {
  const env = createEnvironment({testEnvironmentOptions: {}});
  const load = frameworkRequire(env.context);
  const {serialize} = load('jest-snapshot-utils.cjs');
  const mocks = load('mock.cjs');
  const Mocker = dialect === 'vitest' ? mocks.VitestModuleMocker : mocks.ModuleMocker;
  const mocker = new Mocker(vm.runInContext('globalThis', env.context));
  return {
    context: env.context,
    expect: load('expect.cjs').createExpect(),
    fn: implementation => mocker.fn(implementation),
    spyOn: (object, method) => mocker.spyOn(object, method),
    serialize: value => serialize(value, undefined, SNAPSHOT_FORMAT),
  };
}

async function vitestSide() {
  const importFrom = request => import(pathToFileURL(require.resolve(request, {paths: [RUNNERS]})).href);
  const {format} = await importFrom('@vitest/pretty-format');
  const {getSerializers} = await importFrom('@vitest/snapshot');
  const {fn, spyOn} = await importFrom('@vitest/spy');
  const vitest = await importFrom('@vitest/expect');
  const {chai} = vitest;
  chai.use(vitest.JestExtend);
  chai.use(vitest.JestChaiExpect);
  chai.use(vitest.JestAsymmetricMatchers);
  const expect = value => chai.expect(value);
  Object.assign(expect, chai.expect);
  // @vitest/snapshot's serialize and SnapshotState's snapshotFormat.
  const options = {escapeRegex: true, indent: 2, printFunctionName: false, ...SNAPSHOT_FORMAT, maxOutputLength: 2 ** 27};
  return {
    context: null,
    expect,
    fn,
    spyOn,
    serialize: value => normalizeNewlines(format(value, {...options, plugins: getSerializers()})),
  };
}

// Every value of a side, in order: the extras, then each case's two.
function valuesOf(side, runs, seed) {
  const run = code => (side.context ? vm.runInContext(code, side.context) : vm.runInThisContext(code));
  const host = side.context ? {...run('({Date, Error, Map, Object, Set})'), Buffer} : {Buffer, Date, Error, Map, Object, Set};
  const runCase = run(GENERATOR)(side.expect, host);
  const values = [...run(EXTRAS)(side.fn, side.spyOn)];
  for (let i = 0; i < runs; i++) values.push(...runCase.values(seed * 1_000_003 + i));
  return values;
}

function printed(side, value) {
  try {
    return side.serialize(value);
  } catch (error) {
    return `throws: ${error.message}`;
  }
}

async function main() {
  const args = process.argv.slice(2);
  const option = (name, fallback) => {
    const index = args.indexOf(name);
    return index >= 0 ? Number(args[index + 1]) : fallback;
  };
  const runs = option('--runs', 10000);
  const seed = option('--seed', 1);
  const against = args.includes('--against') ? args[args.indexOf('--against') + 1] : 'jest';
  if (against !== 'jest' && against !== 'vitest') throw new Error('--against takes jest or vitest');

  const reference = against === 'vitest' ? await vitestSide() : jestSide();
  const opal = opalSide(against);
  const expected = valuesOf(reference, runs, seed);
  const actual = valuesOf(opal, runs, seed);

  let differ = 0;
  for (let i = 0; i < expected.length; i++) {
    const a = printed(reference, expected[i]);
    const b = printed(opal, actual[i]);
    if (a === b) continue;
    differ++;
    if (differ <= (against === 'vitest' ? 40 : 5)) {
      console.log(`\n=== value ${i}`);
      console.log(`${against}:\n${a}`);
      console.log(`opal:\n${b}`);
    }
  }
  console.log(`\n${expected.length} values: ${differ} print differently from ${against === 'vitest' ? "Vitest's" : "Jest's"} (seed ${seed})`);
  process.exit(against === 'jest' && differ > 0 ? 1 : 0);
}

main().catch(error => {
  console.error(error);
  process.exit(2);
});
