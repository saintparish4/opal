// main.mjs's body placed before lib.mjs's: the order a bundler gets wrong
// when it emits modules in import order instead of post-order.
let count = 0;
function bump() {
  count++;
}
console.log("main start", count);
console.log("lib evaluated");
bump();
console.log("after bump", count);
console.error("to stderr");
throw new TypeError("done");
