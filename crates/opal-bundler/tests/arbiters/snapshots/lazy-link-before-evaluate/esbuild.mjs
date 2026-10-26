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

// first.mjs
var first;
var init_first = __esm({
  "first.mjs"() {
    init_entry();
    console.log("first runs; hoisted is", typeof hoisted, hoisted());
    first = 1;
  }
});

// second.mjs
function hoisted() {
  return "made at link";
}
var late;
var init_second = __esm({
  "second.mjs"() {
    late = 1;
    console.log("second runs");
  }
});

// entry.mjs
var entry_exports = {};
__export(entry_exports, {
  first: () => first,
  hoisted: () => hoisted,
  late: () => late
});
var init_entry = __esm({
  "entry.mjs"() {
    init_first();
    init_second();
  }
});

// main.mjs
Promise.resolve().then(() => (init_entry(), entry_exports)).then((ns) => console.log("done", Object.keys(ns).join(",")));
