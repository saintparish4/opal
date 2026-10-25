// lib.mjs
let count = 0;
function bump() {
  count++;
}
console.log("lib evaluated");

// main.mjs
console.log("main start", count);
bump();
console.log("after bump", count);
console.error("to stderr");
throw new RangeError("done");
