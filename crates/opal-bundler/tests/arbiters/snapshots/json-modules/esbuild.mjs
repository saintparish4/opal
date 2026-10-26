var __defProp = Object.defineProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

// data.json
var data_exports = {};
__export(data_exports, {
  default: () => data_default
});
var data_default = { a: 1, ["__proto__"]: { polluted: true }, list: [true, null] };

// main.mjs
import { createRequire } from "node:module";
var require2 = createRequire(import.meta.url);
console.log(Object.keys(data_default), Object.getPrototypeOf(data_default) === Object.prototype);
console.log(Object.keys(data_exports), data_default === data_default);
console.log("shared with require:", require2("./data.json") === data_default);
