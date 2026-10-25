var __getOwnPropNames = Object.getOwnPropertyNames;
var __commonJS = (cb, mod) => function __require() {
  try {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  } catch (e) {
    throw mod = 0, e;
  }
};

// main.cjs
var require_main = __commonJS({
  "main.cjs"() {
    process.on("exit", (code) => console.log("exit", code));
    Promise.resolve().then(() => console.log("microtask before the throw was handled"));
    console.log("about to throw");
    throw new RangeError("from the entry");
  }
});
export default require_main();
