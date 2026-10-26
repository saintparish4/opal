// Rule 10: a module reached only through import() runs lazily, once, after
// the current synchronous code, with rules 2-4 intact.
console.log("main start");
const first = import("./lazy.mjs");
console.log("after import() call, before await");
const ns = await first;
console.log("namespace:", Object.keys(ns), ns.value, ns.later());
console.log("same namespace twice:", (await import("./lazy.mjs")) === ns);
// An import of a module that throws rejects, and keeps rejecting with the
// same error; so does an import of the module it was in a cycle with.
let firstError;
try {
  await import("./throws.mjs");
} catch (error) {
  firstError = error;
  console.log("throws:", error.message);
}
try {
  await import("./throws.mjs");
} catch (error) {
  console.log("same error again:", error === firstError);
}
try {
  await import("./throws-partner.mjs");
} catch (error) {
  console.log("cycle partner has it too:", error === firstError);
}
// An eager module's namespace through import().
import * as eager from "./eager.mjs";
console.log("eager namespace:", (await import("./eager.mjs")) === eager);
