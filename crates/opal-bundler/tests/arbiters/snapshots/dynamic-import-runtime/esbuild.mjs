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

// node_modules/pkg/index.js
var pkg_exports = {};
__export(pkg_exports, {
  counter: () => counter
});
var counter;
var init_pkg = __esm({
  "node_modules/pkg/index.js"() {
    counter = 0;
    console.log("pkg runs once");
  }
});

// data.json
var require_data = __commonJS({
  "data.json"(exports, module) {
    module.exports = { k: 1 };
  }
});

// main.mjs
init_pkg();

// dep.mjs
var dep_exports = {};
__export(dep_exports, {
  d: () => d
});
var d = 1;
console.log("dep runs once");

// main.mjs
var names = ["./dep.mjs", "pkg", "./not-imported.mjs", "node:path"];
for (const name of names) {
  const ns = await import(name);
  console.log(name, Object.keys(ns).join(","));
}
console.log("same bundled instance:", await import(names[0]) === dep_exports, (await Promise.resolve().then(() => (init_pkg(), pkg_exports))).counter === counter);
try {
  await import("./missing.mjs");
} catch (error) {
  console.log("missing:", error.constructor.name, error.code);
}
try {
  await Promise.resolve().then(() => __toESM(require_data(), 1));
} catch (error) {
  console.log("json without type:", error.constructor.name, error.code);
}
try {
  await import({ toString() {
    throw new RangeError("no string");
  } });
} catch (error) {
  console.log("toString throws:", error.constructor.name, error.message);
}
