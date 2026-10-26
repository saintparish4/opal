var __defProp = Object.defineProperty;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res, err) => function __init() {
  if (err) throw err[0];
  try {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  } catch (e2) {
    throw err = [e2], e2;
  }
};
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

// eager.mjs
var eager_exports = {};
__export(eager_exports, {
  e: () => e
});
var e;
var init_eager = __esm({
  "eager.mjs"() {
    e = 1;
  }
});

// lazy-dep.mjs
var helper;
var init_lazy_dep = __esm({
  "lazy-dep.mjs"() {
    console.log("lazy-dep body runs");
    helper = () => "from dep";
  }
});

// lazy.mjs
var lazy_exports = {};
__export(lazy_exports, {
  later: () => later,
  value: () => value
});
function later() {
  return "later";
}
var value;
var init_lazy = __esm({
  "lazy.mjs"() {
    init_lazy_dep();
    console.log("lazy body runs");
    value = helper();
  }
});

// throws-partner.mjs
var throws_partner_exports = {};
var init_throws_partner = __esm({
  "throws-partner.mjs"() {
    init_throws();
    console.log("partner runs first");
  }
});

// throws.mjs
var throws_exports = {};
var init_throws = __esm({
  "throws.mjs"() {
    init_throws_partner();
    throw new Error("lazy module failed");
  }
});

// main.mjs
init_eager();
console.log("main start");
var first = Promise.resolve().then(() => (init_lazy(), lazy_exports));
console.log("after import() call, before await");
var ns = await first;
console.log("namespace:", Object.keys(ns), ns.value, ns.later());
console.log("same namespace twice:", await Promise.resolve().then(() => (init_lazy(), lazy_exports)) === ns);
var firstError;
try {
  await Promise.resolve().then(() => (init_throws(), throws_exports));
} catch (error) {
  firstError = error;
  console.log("throws:", error.message);
}
try {
  await Promise.resolve().then(() => (init_throws(), throws_exports));
} catch (error) {
  console.log("same error again:", error === firstError);
}
try {
  await Promise.resolve().then(() => (init_throws_partner(), throws_partner_exports));
} catch (error) {
  console.log("cycle partner has it too:", error === firstError);
}
console.log("eager namespace:", await Promise.resolve().then(() => (init_eager(), eager_exports)) === eager_exports);
