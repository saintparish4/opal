// a.mjs
var shared2 = "a's";
function readEarly() {
  return shared2 + " " + shared;
}

// main.mjs
var shared = "main's";
console.log(readEarly());
export {
  shared
};
