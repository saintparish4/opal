var __defProp = Object.defineProperty;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res, err) => function __init() {
  if (err) throw err[0];
  try {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  } catch (e) {
    throw err = [e], e;
  }
};
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

// b.mjs
var b_exports = {};
__export(b_exports, {
  fromB: () => fromB,
  late: () => late
});
function fromB() {
  return "b";
}
var late;
var init_b = __esm({
  "b.mjs"() {
    init_a();
    console.log("b body runs first", typeof fromA);
    try {
      console.log(value);
    } catch (error) {
      console.log("TDZ:", error.constructor.name, error.message);
    }
    late = "late";
  }
});

// a.mjs
var a_exports = {};
__export(a_exports, {
  fromA: () => fromA,
  value: () => value
});
function fromA() {
  return "a sees " + late;
}
var value;
var init_a = __esm({
  "a.mjs"() {
    init_b();
    console.log("a body", fromB());
    value = 1;
  }
});

// main.mjs
var a = await Promise.resolve().then(() => (init_a(), a_exports));
console.log("a:", a.fromA(), Object.keys(a));
var b = await Promise.resolve().then(() => (init_b(), b_exports));
console.log("b:", b.fromB(), b.late);
