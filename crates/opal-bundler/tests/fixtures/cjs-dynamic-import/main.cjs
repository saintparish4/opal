"use strict";
// import() from a strict CommonJS module of an ES module and of CommonJS.
import("./esm.mjs").then((ns) => {
  console.log("esm:", Object.keys(ns), ns.x);
  return import("./other.cjs");
}).then((ns) => {
  console.log("cjs:", Object.keys(ns), ns.default.y);
});
console.log("sync code first");
