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

// one.mjs
var init_one = __esm({
  "one.mjs"() {
  }
});

// two.mjs
var init_two = __esm({
  "two.mjs"() {
  }
});

// mid.mjs
var mid_exports = {};
var init_mid = __esm({
  "mid.mjs"() {
    init_one();
    init_two();
  }
});

// other.mjs
var init_other = __esm({
  "other.mjs"() {
    init_one();
  }
});

// top.mjs
var top_exports = {};
__export(top_exports, {
  own: () => own
});
var own;
var init_top = __esm({
  "top.mjs"() {
    init_mid();
    init_other();
    own = 0;
  }
});

// main.mjs
init_top();
init_mid();
console.log(Object.keys(top_exports).join(","), "|", Object.keys(mid_exports).join(","), void 0);
var late = await Promise.resolve().then(() => (init_top(), top_exports));
console.log(Object.keys(late).join(","));
