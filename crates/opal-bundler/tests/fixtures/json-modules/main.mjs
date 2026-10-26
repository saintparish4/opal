// Rule 8: JSON through import and require shares one object, and
// "__proto__" is an own property because JSON.parse reads the text.
import data from "./data.json" with { type: "json" };
import * as ns from "./data.json" with { type: "json" };
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
console.log(Object.keys(data), Object.getPrototypeOf(data) === Object.prototype);
console.log(Object.keys(ns), ns.default === data);
console.log("shared with require:", require("./data.json") === data);
