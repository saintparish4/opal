// console.log of values whose formatting has rules: quoting, escaping, line
// breaking at 128 columns, grouping of long arrays, depth, circularity.
// Every line must match node's byte for byte.
const cases = [
  "plain", 'it\'s', 'both \' and "', 'all \' " `', "tab\tnew\nline", "\x00\x1f\x7f\x9f", "\ud800 lone",
  42, -0, 0.1, NaN, Infinity, -Infinity, 1e21, 123n, true, false, null, undefined,
  Symbol(), Symbol("desc"), Symbol.iterator,
  [], [1, 2, 3], [, 1, , , 2, ,], new Array(5), ["a", { b: [1, { c: [2, { d: 3 }] }] }],
  {}, { a: 1 }, { "key-with-dash": 1, valid_id: 2, 3: "num", "": "empty" },
  { [Symbol("s")]: 1, regular: 2 },
  { nested: { deeper: { deepest: { gone: 1 } } } },
  Array.from({ length: 7 }, (_, i) => i),
  Array.from({ length: 26 }, (_, i) => i * 1000),
  Array.from({ length: 101 }, (_, i) => i),
  Array.from({ length: 30 }, (_, i) => "str" + i),
  Array.from({ length: 12 }, (_, i) => ({ i })),
  "x".repeat(200),
  { long: "y".repeat(130) },
  { a: "a".repeat(40), b: "b".repeat(40), c: "c".repeat(40) },
  { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9, j: 10, k: 11, l: 12, m: 13, n: 14, o: 15, p: 16, q: 17, r: 18, s: 19, t: 20, u: 21, v: 22, w: 23, x: 24, y: 25, z: 26 },
  new Map(), new Map([["a", 1], [{ k: 1 }, [2]]]), new Set(), new Set([1, "two", [3]]),
  new WeakMap(), new WeakSet(),
  function named() {}, function () {}, () => {}, async function af() {}, function* gen() {}, async function* ag() {},
  class A {}, class B extends Array {}, class {},
  Object.assign(function withProps() {}, { p: 1 }),
  new Date(0), new Date(NaN), /re/gi, Object.assign(/x/, { extra: 1 }),
  new Error("plain error").message,
  new Number(1), new String("boxed"), new Boolean(false), Object(10n), Object(Symbol("b")),
  Object.create(null), Object.assign(Object.create(null), { a: 1 }),
  new (class Foo { constructor() { this.x = 1; } })(),
  new (class Empty {})(),
  { [Symbol.toStringTag]: "Tagged", v: 1 },
  new Uint8Array([1, 2, 3]), new Float64Array(2), new BigInt64Array(1), new ArrayBuffer(3), new DataView(new ArrayBuffer(2)),
  Promise.resolve(1), Promise.reject(new Error("r")).catch(() => {}) && null, new Promise(() => {}),
  (function () { return arguments; })(1, "a"),
  { get g() { return 1; }, set s(v) {}, get gs() { return 1; }, set gs(v) {} },
  Object.defineProperty({}, "hidden", { value: 1, enumerable: false }),
  [1, 2, 3].concat(Object.assign([], { extra: true })),
  Object.assign([1, 2], { prop: "x" }),
  { __proto__: { inherited: 1 }, own: 2 },
  { "__proto__": 1 },
  { a: [[[[1]]]] },
  { f() {}, g: function () {}, h: () => {} },
  [undefined, null, NaN],
  { und: undefined },
  { [Symbol.for("nodejs.util.inspect.custom")]() { return "custom!"; } },
  { [Symbol.for("nodejs.util.inspect.custom")]() { return { replaced: true }; } },
  new Proxy({ target: 1 }, {}),
  new Proxy(function pf() {}, {}),
  globalThis.Math,
  JSON,
];
for (const value of cases) console.log(value);

const circular = { name: "c" };
circular.self = circular;
circular.list = [circular, { back: circular }];
console.log(circular);
const a = { n: "a" }, b = { n: "b", a };
a.b = b;
console.log([a, b]);

class MyError extends Error {}
const errors = [
  Object.assign(new Error("with props"), { code: "E_CODE", extra: 1 }),
  new TypeError("type"),
  new MyError("mine"),
  new Error("outer", { cause: new Error("inner") }),
  Object.assign(new Error("no stack"), { stack: undefined }),
  { e: new RangeError("nested") },
];
for (const e of errors) {
  // Stack frames name this file's path, which differs between the two
  // runtimes' runs only if the path does; the frames below the first are
  // the runtimes' own internals, so only the first line of each is compared.
  console.log(require("util").inspect(e).split("\n")[0]);
}

console.log("%s|%d|%i|%f|%j|%o|%O|%%|%c|%x", "s", 42.5, 42.5, "3.5", { j: 1 }, [1], { O: 1 }, "css");
console.log("%s", { obj: 1 }, "extra", 3);
console.log("%s %s", "only one");
console.log("%d", 10n, "%i", Symbol("x"));
console.log("no format", 1, "two", { three: 3 });
console.log("%s", null, undefined, Symbol("y"));
console.log("%s", new (class WithToString { toString() { return "custom toString"; } })());
console.log();
console.log("");
console.log("multi\nline", "args");
console.log(Object.assign(Object.create({ inheritedToString() {} }), { x: 1 }));
