// Minimal Jest-shaped harness for the engine-floor measurement ONLY.
//
// It exists so both engine prototypes run the same JavaScript, and so their
// timings include a describe/test/hook/expect layer of realistic size. It is
// not the start of opal's `expect`: equality here is a rough toEqual, and
// nothing in it is checked against Jest. Step 1 builds the oracle that the real
// implementation is held to.
//
// Host contract: the host defines `globalThis.__opal_print(string)` before this
// script runs, evaluates test files as classic scripts, then awaits
// `globalThis.__opal_run()`, which resolves to the number of failed tests.
(function () {
  "use strict";
  const print = (s) => globalThis.__opal_print(String(s));
  if (typeof globalThis.console === "undefined" || !globalThis.console.log) {
    const fmt = (args) => args.map((a) => (typeof a === "string" ? a : JSON.stringify(a))).join(" ");
    globalThis.console = { log: (...a) => print(fmt(a)), error: (...a) => print(fmt(a)) };
  }

  const root = { name: "", tests: [], children: [], before: [], after: [], beforeAll: [], afterAll: [] };
  let current = root;

  globalThis.describe = (name, fn) => {
    const block = { name, tests: [], children: [], before: [], after: [], beforeAll: [], afterAll: [], parent: current };
    current.children.push(block);
    const prev = current;
    current = block;
    try { fn(); } finally { current = prev; }
  };
  globalThis.test = globalThis.it = (name, fn) => current.tests.push({ name, fn });
  globalThis.beforeEach = (fn) => current.before.push(fn);
  globalThis.afterEach = (fn) => current.after.push(fn);
  globalThis.beforeAll = (fn) => current.beforeAll.push(fn);
  globalThis.afterAll = (fn) => current.afterAll.push(fn);

  function equals(a, b) {
    if (Object.is(a, b)) return true;
    if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    const ka = Object.keys(a).filter((k) => a[k] !== undefined);
    const kb = Object.keys(b).filter((k) => b[k] !== undefined);
    if (ka.length !== kb.length) return false;
    return ka.every((k) => equals(a[k], b[k]));
  }

  function matchers(actual, negate) {
    const check = (pass, msg) => {
      if (pass === negate) throw new Error((negate ? "not " : "") + msg);
    };
    return {
      toBe: (e) => check(Object.is(actual, e), `expected ${actual} to be ${e}`),
      toEqual: (e) => check(equals(actual, e), "expected values to be equal"),
      toThrow: (m) => {
        let threw = false, err;
        try { actual(); } catch (x) { threw = true; err = x; }
        check(threw && (m === undefined || String(err && err.message).includes(m)), "expected function to throw");
      },
      toMatch: (re) => check(re.test(actual), `expected ${actual} to match ${re}`),
      toContain: (x) => check(actual.includes(x), "expected to contain"),
      toHaveLength: (n) => check(actual.length === n, "expected length"),
      toBeTruthy: () => check(!!actual, "expected truthy"),
      toBeFalsy: () => check(!actual, "expected falsy"),
      toBeCloseTo: (e, d = 2) => check(Math.abs(actual - e) < Math.pow(10, -d) / 2, "expected close"),
      toBeGreaterThan: (e) => check(actual > e, "expected greater"),
      toBeNull: () => check(actual === null, "expected null"),
      toBeUndefined: () => check(actual === undefined, "expected undefined"),
    };
  }
  globalThis.expect = (actual) => {
    const m = matchers(actual, false);
    m.not = matchers(actual, true);
    return m;
  };

  function hooks(block, key) {
    const chain = [];
    for (let b = block; b; b = b.parent) chain.unshift(b);
    const fns = chain.flatMap((b) => b[key]);
    return key === "after" ? fns.reverse() : fns;
  }

  async function runBlock(block, path, stats) {
    for (const h of block.beforeAll) await h();
    for (const t of block.tests) {
      const name = [...path, t.name].join(" > ");
      try {
        for (const h of hooks(block, "before")) await h();
        await t.fn();
        for (const h of hooks(block, "after")) await h();
        stats.passed++;
      } catch (e) {
        stats.failed++;
        print(`FAIL ${name}: ${e && e.message}`);
      }
    }
    for (const c of block.children) await runBlock(c, [...path, c.name], stats);
    for (const h of block.afterAll) await h();
  }

  // Reset between files when a host reuses one context for several files.
  globalThis.__opal_reset = () => {
    root.tests = []; root.children = []; root.before = []; root.after = [];
    root.beforeAll = []; root.afterAll = [];
    current = root;
  };

  globalThis.__opal_run = async (label) => {
    const stats = { passed: 0, failed: 0 };
    await runBlock(root, [], stats);
    if (label !== undefined) print(`${label}: ${stats.passed} passed, ${stats.failed} failed`);
    return stats.failed;
  };
})();
