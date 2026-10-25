#!/usr/bin/env node
// opal test's mock functions, held to Jest's own on random use.
//
//   node compat/test-mock.cjs [--runs N] [--seed N]
//
// Jest's real jest-mock (the build installed in compat/test-runners/runners)
// runs in one fresh test context and opal's port in another, each built by
// opal's environment.cjs as a test file's is. One seeded program of random
// operations runs in both: calls with arguments, `new`, calls with a `this`,
// return values and implementations (permanent and once), throwing
// implementations, mockClear, mockReset, mockRestore, names,
// withImplementation, spyOn and replaceProperty on objects, and automocks of
// random object shapes through getMetadata and generateFromMetadata. After
// every operation, the full recorded state of every mock is written down
// (calls, results, instances, contexts, the order of invocations, lastCall,
// the name, what the spied object now holds) and the two contexts have to
// write down the same thing.
'use strict';

const fs = require('fs');
const path = require('path');
const vm = require('vm');
const {builtinModules} = require('module');

const RUNTIME = path.join(__dirname, '..', 'crates', 'opal-test', 'runtime');
const RUNNERS = path.join(__dirname, 'test-runners', 'runners');
const {createEnvironment} = require(path.join(RUNTIME, 'environment.cjs'));
const {frameworkRequire} = require(path.join(RUNTIME, 'runner.cjs'));

const builtins = new Set(builtinModules);
const isBuiltin = request => request.startsWith('node:') || builtins.has(request.split('/')[0]);

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
    wrapper.call(module.exports, module.exports, localRequire, module, filename, dir);
    return module.exports;
  };
  return load(entry);
}

const PROGRAM = String.raw`
(function (ModuleMocker) {
  function rngFor(seed) {
    let a = seed >>> 0;
    return () => {
      a |= 0; a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  // A deterministic description of anything a mock can record.
  function describe(value, seen = new Map(), known) {
    if (value === null) return 'null';
    if (value === undefined) return 'undefined';
    const type = typeof value;
    if (type === 'number' || type === 'boolean') return type + ':' + String(value);
    if (type === 'string') return JSON.stringify(value);
    if (type === 'symbol') return String(value);
    if (type === 'bigint') return value + 'n';
    if (known && known.has(value)) return known.get(value);
    if (type === 'function') return value._isMockFunction ? 'mockfn(' + value.getMockName() + ')' : 'fn';
    if (seen.has(value)) return '#' + seen.get(value);
    seen.set(value, seen.size);
    if (value instanceof Promise) return 'promise';
    if (value instanceof Error) return 'error(' + value.message + ')';
    if (Array.isArray(value)) return '[' + value.map(v => describe(v, seen, known)).join(',') + ']';
    const proto = Object.getPrototypeOf(value);
    const name = proto && proto.constructor ? proto.constructor.name : 'null';
    return name + '{' + Object.keys(value).map(k => k + ':' + describe(value[k], seen, known)).join(',') + '}';
  }

  return function run(seed, steps) {
    const rng = rngFor(seed);
    const pick = list => list[Math.floor(rng() * list.length)];
    const mocker = new ModuleMocker(globalThis);
    const mocks = [];
    const objects = [];
    const known = new Map();
    const value = () => pick([0, 1, 'a', null, undefined, {x: 1}, [1, 2], true, -0, NaN]);
    const trace = [];
    const label = obj => {
      if (!known.has(obj)) known.set(obj, 'obj' + known.size);
      return obj;
    };

    function snapshot() {
      // Invocation order is a global counter; record it relative to the
      // first one either side saw.
      const orders = mocks.flatMap(m => m.mock.invocationCallOrder);
      const base = orders.length ? Math.min(...orders) : 0;
      return mocks.map(m => [
        m.getMockName(),
        m.mock.calls.length,
        describe(m.mock.calls, new Map(), known),
        describe(m.mock.results.map(r => [r.type, r.value]), new Map(), known),
        describe(m.mock.contexts, new Map(), known),
        m.mock.instances.length,
        describe(m.mock.lastCall, new Map(), known),
        m.mock.invocationCallOrder.map(o => o - base).join(','),
      ].join('|')).concat(objects.map(o => describe(o, new Map(), known))).join('\n');
    }

    for (let step = 0; step < steps; step++) {
      const op = Math.floor(rng() * 20);
      let outcome = '';
      try {
        if (op === 0 || mocks.length === 0) {
          const m = mocker.fn(rng() < 0.3 ? function (a) { return a; } : undefined);
          mocks.push(m);
          outcome = 'fn';
        } else if (op === 1) {
          const obj = label({method(a) { return 'orig:' + a; }, n: 1});
          objects.push(obj);
          const spy = mocker.spyOn(obj, 'method');
          mocks.push(spy);
          outcome = 'spy';
        } else {
          const m = pick(mocks);
          switch (op) {
            case 2: case 3: case 4: outcome = describe(m(value(), value()), new Map(), known); break;
            case 5: { const ctx = label({c: 1}); outcome = describe(m.call(ctx, value()), new Map(), known); break; }
            case 6: { const inst = new m(value()); outcome = 'new:' + typeof inst; break; }
            case 7: m.mockReturnValue(value()); break;
            case 8: m.mockReturnValueOnce(value()); break;
            case 9: m.mockImplementation(function (a) { return this === undefined ? a : 'ctx'; }); break;
            case 10: m.mockImplementationOnce(() => { throw new Error('once'); }); break;
            case 11: m.mockClear(); break;
            case 12: m.mockReset(); break;
            case 13: m.mockRestore(); break;
            case 14: m.mockName(pick(['a', 'b', ''])); break;
            case 15: m.withImplementation(() => 'within', () => { outcome = describe(m(1), new Map(), known); }); break;
            case 16: if (objects.length) outcome = describe(pick(objects).method(value()), new Map(), known); break;
            case 17: { const o = pick(objects); if (o) { const r = mocker.replaceProperty(o, 'n', pick([2, 3])); if (rng() < 0.5) r.restore(); } break; }
            case 18: {
              const shape = {a: 1, f() {}, nested: {g() {}, s: 'x'}, arr: [1, () => 2], cls: class K { m() {} }};
              const meta = mocker.getMetadata(shape);
              const generated = mocker.generateFromMetadata(meta);
              outcome = describe(meta) + ' ' + describe(generated);
              break;
            }
            default: mocker.clearAllMocks(); break;
          }
        }
      } catch (error) {
        outcome = 'threw:' + error.message;
      }
      trace.push(step + ' op' + op + ' ' + outcome + '\n' + snapshot());
    }
    return trace;
  };
})
`;

function main() {
  const args = process.argv.slice(2);
  const option = (name, fallback) => {
    const index = args.indexOf(name);
    return index >= 0 ? Number(args[index + 1]) : fallback;
  };
  const runs = option('--runs', 2000);
  const seed = option('--seed', 1);

  const jestEnv = createEnvironment({testEnvironmentOptions: {}});
  const opalEnv = createEnvironment({testEnvironmentOptions: {}});
  const jestMock = loadInContext(jestEnv.context, require.resolve('jest-mock', {paths: [RUNNERS]}));
  const opalMock = frameworkRequire(opalEnv.context)('mock.cjs');
  const jestRun = vm.runInContext(PROGRAM, jestEnv.context)(jestMock.ModuleMocker);
  const opalRun = vm.runInContext(PROGRAM, opalEnv.context)(opalMock.ModuleMocker);

  let mismatches = 0;
  let steps = 0;
  for (let i = 0; i < runs; i++) {
    const caseSeed = seed * 1_000_003 + i;
    const expected = jestRun(caseSeed, 30);
    const actual = opalRun(caseSeed, 30);
    for (let step = 0; step < expected.length; step++) {
      steps++;
      if (expected[step] === actual[step]) continue;
      mismatches++;
      if (mismatches <= 3) {
        console.log(`\n=== program ${caseSeed}, step ${step}`);
        console.log(`jest:\n${expected[step]}`);
        console.log(`opal:\n${actual[step]}`);
      }
      break;
    }
  }
  console.log(`\n${runs} programs, ${steps} operations: ${mismatches} programs differ (seed ${seed})`);
  process.exit(mismatches > 0 ? 1 : 0);
}

main();
