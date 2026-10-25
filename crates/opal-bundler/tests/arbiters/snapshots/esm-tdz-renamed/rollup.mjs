let shared$1 = "a's";
function readEarly() {
  return shared$1 + " " + shared;
}
try {
  shared;
} catch (error) {
  console.log(error.constructor.name, error.message);
}

// Two modules declare `let shared`; one is renamed in the bundle. A cycle
// reads the other module's binding before it is initialized.
let shared = "main's";
console.log(readEarly());

export { shared };
