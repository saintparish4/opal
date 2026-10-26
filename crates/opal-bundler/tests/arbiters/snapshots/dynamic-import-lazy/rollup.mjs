var eager = /*#__PURE__*/Object.freeze({
  __proto__: null
});

console.log("lazy-dep body runs");
const helper = () => "from dep";

console.log("lazy body runs");
const value = helper();
function later() {
  return "later";
}

var lazy = /*#__PURE__*/Object.freeze({
  __proto__: null,
  later: later,
  value: value
});

console.log("partner runs first");

var throwsPartner = /*#__PURE__*/Object.freeze({
  __proto__: null
});

throw new Error("lazy module failed");

var throws = /*#__PURE__*/Object.freeze({
  __proto__: null
});

// Rule 10: a module reached only through import() runs lazily, once, after
// the current synchronous code, with rules 2-4 intact.
console.log("main start");
const first = Promise.resolve().then(function () { return lazy; });
console.log("after import() call, before await");
const ns = await first;
console.log("namespace:", Object.keys(ns), ns.value, ns.later());
console.log("same namespace twice:", (await Promise.resolve().then(function () { return lazy; })) === ns);
// An import of a module that throws rejects, and keeps rejecting with the
// same error; so does an import of the module it was in a cycle with.
let firstError;
try {
  await Promise.resolve().then(function () { return throws; });
} catch (error) {
  firstError = error;
  console.log("throws:", error.message);
}
try {
  await Promise.resolve().then(function () { return throws; });
} catch (error) {
  console.log("same error again:", error === firstError);
}
try {
  await Promise.resolve().then(function () { return throwsPartner; });
} catch (error) {
  console.log("cycle partner has it too:", error === firstError);
}
console.log("eager namespace:", (await Promise.resolve().then(function () { return eager; })) === eager);
