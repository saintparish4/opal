// Rule 7: an ES module importing CommonJS sees what Node's synthetic module
// record holds: default is module.exports, named exports are the lexer's
// names read once after the module ran, and the namespace is fixed.
import lib, * as ns from "./lib.cjs";
import { counter, bump, fromGetter, viaDefine } from "./lib.cjs";
import esModuleDefault, { named as esNamed } from "./transpiled.cjs";
import * as reexporting from "./reexporting.cjs";

console.log(Object.keys(ns));
console.log(ns.default === lib, ns["module.exports"] === lib);
console.log("snapshot:", counter);
bump();
console.log("after bump:", counter, ns.counter, lib.counter);
console.log(fromGetter, viaDefine, "unrecognizedGetter" in ns, lib.unrecognizedGetter);
console.log("__esModule default is still module.exports:", typeof esModuleDefault, esModuleDefault.default, esNamed);
console.log("re-exported names:", Object.keys(reexporting));
console.log(Object.getOwnPropertyDescriptor(ns, "counter"));
