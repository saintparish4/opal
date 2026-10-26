// b.mjs
function fromB() {
  return "b";
}
console.log("b body", typeof fromA);

// a.mjs
function fromA() {
  return "a:" + fromB();
}
console.log("a body");

// c.mjs
console.log("c body");

// main.mjs
console.log("main", fromA());
