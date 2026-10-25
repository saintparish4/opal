// No "use strict": rule 6 keeps sloppy mode, which changes what these do.
var results = [];
function plain() {
  return this;
}
results.push(plain() === globalThis);
undeclared = 42; // creates a global in sloppy mode, throws in strict
results.push(globalThis.undeclared);
var frozen = Object.freeze({ x: 1 });
frozen.x = 2; // silently fails in sloppy mode
results.push(frozen.x);
function aliased(a) {
  arguments[0] = "changed";
  return a;
}
results.push(aliased("original"));
with ({ inside: "with works" }) {
  results.push(inside);
}
results.push(010);
results.push(typeof require("./strict.cjs").check());
console.log(results.join(" | "));
