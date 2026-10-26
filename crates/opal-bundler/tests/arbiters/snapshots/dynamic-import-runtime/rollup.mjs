let counter = 0; console.log("pkg runs once");

var index = /*#__PURE__*/Object.freeze({
  __proto__: null,
  counter: counter
});

console.log("dep runs once");

var dep = /*#__PURE__*/Object.freeze({
  __proto__: null
});

var k = 1;
var data = {
	k: k
};

var data$1 = /*#__PURE__*/Object.freeze({
  __proto__: null,
  default: data,
  k: k
});

// Rule 10: import() of an expression, or of a literal the graph does not
// resolve, is resolved when it runs, and a module in the bundle is the
// bundled one.
const names = ["./dep.mjs", "pkg", "./not-imported.mjs", "node:path"];
for (const name of names) {
  const ns = await import(name);
  console.log(name, Object.keys(ns).join(","));
}
console.log("same bundled instance:", (await import(names[0])) === dep, (await Promise.resolve().then(function () { return index; })).counter === counter);
try {
  await import("./missing" + ".mjs");
} catch (error) {
  console.log("missing:", error.constructor.name, error.code);
}
try {
  await Promise.resolve().then(function () { return data$1; });
} catch (error) {
  console.log("json without type:", error.constructor.name, error.code);
}
try {
  await import({ toString() { throw new RangeError("no string"); } });
} catch (error) {
  console.log("toString throws:", error.constructor.name, error.message);
}
