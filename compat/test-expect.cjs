#!/usr/bin/env node
// opal test's expect, held to Jest's own on random values.
//
//   node compat/test-expect.cjs [--runs N] [--seed N] [--color] [--against vitest]
//
// Jest's real `expect` (the build installed in compat/test-runners/runners)
// runs in one fresh test context, opal's port in another, each context built
// by opal's environment.cjs the way a test file's is. One seeded generator
// runs in both, so every case is the same structure in each context, made of
// that context's own built-ins, as a test file's values are. For each pair
// of values the matchers that compare structure (toEqual, toStrictEqual,
// toMatchObject, toContainEqual, toContain) are run both ways, and the two
// expects have to agree on pass or fail and on the failure message, byte for
// byte. The message check covers what the verdicts don't: opal's own
// pretty-format port, which the matchers print values with.
//
// The values are what equality has edge cases for: -0 and NaN, sparse arrays
// and holes beside explicit undefined, keys holding undefined, symbol keys,
// non-enumerable properties, Maps and Sets with object members, Dates
// (invalid ones too), RegExps, Errors with causes, two classes of the same
// shape, Object.create(null), typed arrays, ArrayBuffers and DataViews, boxed
// primitives, cycles, values from the host realm, and Jest's asymmetric
// matchers inside them. Half the pairs are a value and a copy with one
// change; the rest are a value and a plain copy, or two unrelated values.
//
// --color runs both with FORCE_COLOR=1, as a Jest worker runs when the
// terminal supports color or `--color` is given, so the messages carry
// chalk's codes and have to carry the same ones.
//
// --against vitest holds opal to Vitest's expect instead, to measure the
// distance rather than to close it: opal's expect is Jest's, in Vitest's
// dialect too. Vitest's real expect (chai with @vitest/expect's plugins, put
// together as Vitest's createExpect does) runs in this realm, since it
// claims the same global as Jest's, and opal's in a test context. Both
// build the generator's values from their own realm's built-ins. Only
// verdicts are compared: Vitest's messages are chai's, so all of them
// differ. The cases that differ are printed, for COMPATIBILITY.md.
'use strict';

process.env.FORCE_COLOR = process.argv.includes('--color') ? '1' : '0';

const fs = require('fs');
const path = require('path');
const util = require('util');
const vm = require('vm');
const {pathToFileURL} = require('url');
const {builtinModules} = require('module');

const RUNTIME = path.join(__dirname, '..', 'crates', 'opal-test', 'runtime');
const RUNNERS = path.join(__dirname, 'test-runners', 'runners');
const {createEnvironment} = require(path.join(RUNTIME, 'environment.cjs'));
const {frameworkRequire} = require(path.join(RUNTIME, 'runner.cjs'));

const builtins = new Set(builtinModules);
const isBuiltin = request => request.startsWith('node:') || builtins.has(request.split('/')[0]);

// Jest's build, evaluated inside the context the way jest-runtime loads its
// internal modules.
function loadInContext(context, entry) {
  const cache = new Map();
  const load = filename => {
    const cached = cache.get(filename);
    if (cached) return cached.exports;
    const module = {exports: {}, filename, id: filename};
    cache.set(filename, module);
    if (filename.endsWith('.json')) {
      module.exports = JSON.parse(fs.readFileSync(filename, 'utf8'));
      return module.exports;
    }
    const code = fs.readFileSync(filename, 'utf8');
    const wrapper = new vm.Script(`(function (exports, require, module, __filename, __dirname) {${code}\n})`, {
      filename,
    }).runInContext(context);
    const dir = path.dirname(filename);
    const localRequire = request =>
      isBuiltin(request) ? require(request) : load(require.resolve(request, {paths: [dir]}));
    localRequire.resolve = request => require.resolve(request, {paths: [dir]});
    wrapper.call(module.exports, module.exports, localRequire, module, filename, dir);
    return module.exports;
  };
  return load(entry);
}

// Runs inside each context. Everything random comes from the seeded rng, so
// both contexts build the same values.
const GENERATOR = String.raw`
(function (expect, host) {
  function rngFor(seed) {
    let a = seed >>> 0;
    return () => {
      a |= 0; a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  class A { constructor(x) { this.x = x; } }
  class B { constructor(x) { this.x = x; } }
  const sym = Symbol.for('opal.key');
  const PRIMITIVES = [0, -0, 1, 1.5, NaN, Infinity, -Infinity, '', 'a', 'b', 'ab', true, false, null, undefined, 10n, sym, Symbol.for('other')];
  const KEYS = ['a', 'b', 'c', 'x', '0', 'length'];

  function make(rng, depth) {
    const pick = list => list[Math.floor(rng() * list.length)];
    const r = rng();
    if (depth > 3 || r < 0.35) return pick(PRIMITIVES);
    const kind = Math.floor(rng() * 24);
    const child = () => make(rng, depth + 1);
    switch (kind) {
      case 0: case 1: case 2: {
        const o = {};
        const n = Math.floor(rng() * 4);
        for (let i = 0; i < n; i++) o[pick(KEYS)] = child();
        if (rng() < 0.15) o[sym] = child();
        if (rng() < 0.1) Object.defineProperty(o, 'hidden', {value: child(), enumerable: false});
        return o;
      }
      case 3: case 4: {
        const n = Math.floor(rng() * 4);
        const arr = [];
        for (let i = 0; i < n; i++) arr.push(child());
        if (rng() < 0.15) arr.length += 1;
        if (rng() < 0.1) arr.extra = child();
        return arr;
      }
      case 5: return new Map([[child(), child()], [pick(KEYS), child()]]);
      case 6: return new Set([child(), child()]);
      case 7: return new Date(rng() < 0.1 ? NaN : Math.floor(rng() * 4) * 1000);
      case 8: return new RegExp(pick(['a', 'b+', '^x$']), pick(['', 'g', 'i']));
      case 9: {
        const error = new (pick([Error, TypeError]))(pick(['m', 'n']));
        if (rng() < 0.3) error.cause = child();
        return error;
      }
      case 10: return new A(child());
      case 11: return new B(child());
      case 12: { const o = Object.create(null); o.a = child(); return o; }
      case 13: return new Uint8Array([Math.floor(rng() * 3), 1]);
      case 14: return new Float64Array([rng() < 0.5 ? 0 : -0, NaN]);
      case 15: return new Uint8Array([1, 2]).buffer;
      case 16: return new DataView(new Uint8Array([1, Math.floor(rng() * 2)]).buffer);
      case 17: return pick([new Number(1), new String('a'), new Boolean(true), Object(1n)]);
      case 18: { const o = {a: 1}; o.self = o; return o; }
      case 19: return pick([new host.Map([[1, 2]]), new host.Set([1]), host.Buffer.from('ab'), new host.Date(0), new host.Error('m'), host.Object.assign(host.Object.create(host.Object.prototype), {a: 1})]);
      case 20: return pick([expect.any(Number), expect.any(Object), expect.anything(), expect.any(A)]);
      case 21: return pick([expect.objectContaining({a: child()}), expect.arrayContaining([child()]), expect.stringContaining('a'), expect.stringMatching(/^a/)]);
      case 22: { const args = (function () { return arguments; })(child(), child()); return args; }
      default: return pick([[undefined], [, 1], {a: undefined}, {}, [], [[]], [{}]]);
    }
  }

  // A copy that equals the original in every sense the matchers check.
  function clone(value, seen = new Map()) {
    if (value === null || typeof value !== 'object') return value;
    if (seen.has(value)) return seen.get(value);
    if (value.asymmetricMatch || ArrayBuffer.isView(value) || value instanceof ArrayBuffer || value instanceof host.Map || value instanceof host.Set) return value;
    if (Array.isArray(value)) {
      const copy = new Array(value.length);
      seen.set(value, copy);
      for (const key of Object.keys(value)) copy[key] = clone(value[key], seen);
      return copy;
    }
    if (value instanceof Map) { const copy = new Map(); seen.set(value, copy); for (const [k, v] of value) copy.set(k, clone(v, seen)); return copy; }
    if (value instanceof Set) { const copy = new Set(); seen.set(value, copy); for (const v of value) copy.add(clone(v, seen)); return copy; }
    if (value instanceof Date) return new Date(value.getTime());
    if (value instanceof RegExp) return new RegExp(value.source, value.flags);
    if (value instanceof Error) { const copy = new value.constructor(value.message); if ('cause' in value) copy.cause = clone(value.cause, seen); return copy; }
    if (Object.prototype.toString.call(value) === '[object Arguments]' || typeof value.valueOf === 'function' && value !== value.valueOf()) return value;
    const copy = Object.create(Object.getPrototypeOf(value));
    seen.set(value, copy);
    for (const key of Reflect.ownKeys(value)) {
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      if ('value' in descriptor) descriptor.value = clone(descriptor.value, seen);
      Object.defineProperty(copy, key, descriptor);
    }
    return copy;
  }

  // A copy with one thing changed, somewhere in the structure.
  function mutate(rng, value, depth = 0) {
    const pick = list => list[Math.floor(rng() * list.length)];
    // An asymmetric matcher is a leaf: its fields are the implementation's,
    // and the two builds lay them out in different orders.
    if (value === null || typeof value !== 'object' || value.asymmetricMatch || depth > 3 || rng() < 0.25) {
      return pick([undefined, null, 0, -0, NaN, 'a', 1, {}, [], [undefined], new A(1), new B(1), {a: undefined}]);
    }
    const copy = clone(value);
    if (Array.isArray(copy)) {
      if (copy.length === 0 || rng() < 0.2) { copy.push(undefined); return copy; }
      if (rng() < 0.2) { const i = Math.floor(rng() * copy.length); delete copy[i]; return copy; }
      const i = Math.floor(rng() * copy.length);
      copy[i] = mutate(rng, copy[i], depth + 1);
      return copy;
    }
    if (copy instanceof Map) { copy.set(pick(['a', 'z']), mutate(rng, undefined, depth + 1)); return copy; }
    if (copy instanceof Set) { copy.add(pick([1, 'z', undefined])); return copy; }
    if (copy instanceof Date) return new Date(copy.getTime() + 1);
    const keys = Object.keys(copy);
    if (keys.length === 0 || rng() < 0.2) { copy[pick(['a', 'q'])] = pick([undefined, 1]); return copy; }
    const key = pick(keys);
    if (rng() < 0.2) { delete copy[key]; return copy; }
    copy[key] = mutate(rng, copy[key], depth + 1);
    return copy;
  }

  const MATCHERS = ['toEqual', 'toStrictEqual', 'toMatchObject', 'toContainEqual', 'toContain'];

  // The case's two values, from its seed alone.
  function values(seed) {
    const rng = rngFor(seed);
    const a = make(rng, 0);
    const r = rng();
    const b = r < 0.5 ? mutate(rng, a) : r < 0.75 ? clone(a) : make(rng, 0);
    return [a, b];
  }

  function runCase(seed) {
    const [a, b] = values(seed);
    const outcome = {};
    for (const matcher of MATCHERS) {
      const received = matcher === 'toContainEqual' || matcher === 'toContain' ? [1, a] : a;
      try {
        expect(received)[matcher](b);
        outcome[matcher] = 'pass';
      } catch (error) {
        outcome[matcher] = 'fail: ' + error.message;
      }
    }
    return outcome;
  }
  runCase.values = values;
  return runCase;
})
`;

async function againstVitest(runs, seed) {
  const vitest = await import(pathToFileURL(require.resolve('@vitest/expect', {paths: [RUNNERS]})).href);
  const {chai} = vitest;
  chai.use(vitest.JestExtend);
  chai.use(vitest.JestChaiExpect);
  chai.use(vitest.JestAsymmetricMatchers);
  const vitestExpect = value => chai.expect(value);
  Object.assign(vitestExpect, chai.expect);
  const vitestCase = vm.runInThisContext(GENERATOR)(vitestExpect, {Buffer, Date, Error, Map, Object, Set});

  const opalEnv = createEnvironment({testEnvironmentOptions: {}});
  const opalExpect = frameworkRequire(opalEnv.context)('expect.cjs').createExpect();
  const own = vm.runInContext('({Date, Error, Map, Object, Set})', opalEnv.context);
  const opalCase = vm.runInContext(GENERATOR, opalEnv.context)(opalExpect, {...own, Buffer});

  const differing = {};
  let calls = 0;
  let shown = 0;
  for (let i = 0; i < runs; i++) {
    const caseSeed = seed * 1_000_003 + i;
    const expected = vitestCase(caseSeed);
    const actual = opalCase(caseSeed);
    for (const matcher of Object.keys(expected)) {
      calls++;
      const vitestPass = expected[matcher] === 'pass';
      if (vitestPass === (actual[matcher] === 'pass')) continue;
      differing[matcher] = (differing[matcher] || 0) + 1;
      if (shown++ < 40) {
        const [a, b] = vitestCase.values(caseSeed);
        const show = value => util.inspect(value, {depth: 6, breakLength: Infinity});
        console.log(`\n=== case ${caseSeed}, ${matcher}: vitest ${vitestPass ? 'passes' : 'fails'}, opal ${vitestPass ? 'fails' : 'passes'}`);
        console.log(`received: ${show(a)}`);
        console.log(`expected: ${show(b)}`);
      }
    }
  }
  const total = Object.values(differing).reduce((sum, n) => sum + n, 0);
  console.log(`\n${runs} cases, ${calls} matcher calls: ${total} verdicts differ from Vitest's (seed ${seed}) ${JSON.stringify(differing)}`);
}

function main() {
  const args = process.argv.slice(2);
  const option = (name, fallback) => {
    const index = args.indexOf(name);
    return index >= 0 ? Number(args[index + 1]) : fallback;
  };
  const runs = option('--runs', 20000);
  const seed = option('--seed', 1);
  if (args.includes('--against')) {
    if (args[args.indexOf('--against') + 1] !== 'vitest') throw new Error('--against takes vitest');
    againstVitest(runs, seed).catch(error => {
      console.error(error);
      process.exit(2);
    });
    return;
  }

  const host = {Buffer, Date, Error, Map, Object, Set};
  const jestEnv = createEnvironment({testEnvironmentOptions: {}});
  const opalEnv = createEnvironment({testEnvironmentOptions: {}});
  const jestExpect = loadInContext(jestEnv.context, require.resolve('expect', {paths: [RUNNERS]})).expect;
  const opalExpect = frameworkRequire(opalEnv.context)('expect.cjs').createExpect();
  const jestCase = vm.runInContext(GENERATOR, jestEnv.context)(jestExpect, host);
  const opalCase = vm.runInContext(GENERATOR, opalEnv.context)(opalExpect, host);

  let verdictMismatches = 0;
  let messageMismatches = 0;
  const counts = {pass: 0, fail: 0};
  for (let i = 0; i < runs; i++) {
    const caseSeed = seed * 1_000_003 + i;
    const expected = jestCase(caseSeed);
    const actual = opalCase(caseSeed);
    for (const matcher of Object.keys(expected)) {
      const a = expected[matcher];
      const b = actual[matcher];
      counts[a === 'pass' ? 'pass' : 'fail']++;
      if (a === b) continue;
      const verdictDiffers = (a === 'pass') !== (b === 'pass');
      if (verdictDiffers) verdictMismatches++;
      else messageMismatches++;
      if (verdictMismatches + messageMismatches <= 5) {
        console.log(`\n=== case ${caseSeed}, ${matcher}: ${verdictDiffers ? 'verdict' : 'message'} differs`);
        console.log(`jest: ${a}`);
        console.log(`opal: ${b}`);
      }
    }
  }
  console.log(
    `\n${runs} cases, ${counts.pass + counts.fail} matcher calls (${counts.pass} pass, ${counts.fail} fail): ` +
      `${verdictMismatches} verdicts and ${messageMismatches} messages differ (seed ${seed})`,
  );
  process.exit(verdictMismatches + messageMismatches > 0 ? 1 : 0);
}

if (require.main === module) main();

module.exports = {GENERATOR, RUNNERS, loadInContext};
