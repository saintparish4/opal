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

// main.mjs
var main_exports = {};
__export(main_exports, {
  x: () => x
});
var self, x;
var init_main = __esm({
  async "main.mjs"() {
    console.log("before");
    self = await init_main().then(() => main_exports);
    console.log("never", Object.keys(self));
    x = 1;
  }
});
await init_main();
export {
  x
};
