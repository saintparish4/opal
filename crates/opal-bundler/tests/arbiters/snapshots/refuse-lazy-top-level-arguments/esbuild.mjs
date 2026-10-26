var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res, err) => function __init() {
  if (err) throw err[0];
  try {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  } catch (e) {
    throw err = [e], e;
  }
};

// lazy.mjs
var lazy_exports = {};
var init_lazy = __esm({
  "lazy.mjs"() {
    console.log(typeof arguments);
  }
});

// main.mjs
Promise.resolve().then(() => (init_lazy(), lazy_exports)).catch((e) => console.log(e.constructor.name));
