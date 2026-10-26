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

// lazy.mjs
var lazy_exports = {};
__export(lazy_exports, {
  value: () => value
});
var value;
var init_lazy = __esm({
  async "lazy.mjs"() {
    await init_main();
    value = "lazy sees " + fromEntry;
  }
});

// main.mjs
var lazy, fromEntry;
var init_main = __esm({
  async "main.mjs"() {
    lazy = await init_lazy().then(() => lazy_exports);
    console.log(lazy.value);
    fromEntry = "entry";
  }
});
await init_main();
export {
  fromEntry
};
