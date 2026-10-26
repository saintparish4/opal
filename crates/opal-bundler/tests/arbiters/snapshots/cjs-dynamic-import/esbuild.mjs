var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __esm = (fn, res, err) => function __init() {
  if (err) throw err[0];
  try {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  } catch (e) {
    throw err = [e], e;
  }
};
var __commonJS = (cb, mod) => function __require() {
  try {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  } catch (e) {
    throw mod = 0, e;
  }
};
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));

// esm.mjs
var esm_exports = {};
__export(esm_exports, {
  x: () => x
});
var x;
var init_esm = __esm({
  "esm.mjs"() {
    x = "esm x";
    console.log("esm runs");
  }
});

// other.cjs
var require_other = __commonJS({
  "other.cjs"(exports) {
    "use strict";
    exports.y = "cjs y";
  }
});

// main.cjs
var require_main = __commonJS({
  "main.cjs"() {
    Promise.resolve().then(() => (init_esm(), esm_exports)).then((ns) => {
      console.log("esm:", Object.keys(ns), ns.x);
      return Promise.resolve().then(() => __toESM(require_other()));
    }).then((ns) => {
      console.log("cjs:", Object.keys(ns), ns.default.y);
    });
    console.log("sync code first");
  }
});
export default require_main();
