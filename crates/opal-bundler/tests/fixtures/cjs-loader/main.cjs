"use strict";
// SEMANTICS.md rule 6: the CommonJS wrapper, the module object, the cache,
// and require() of bundled, builtin, and missing modules.
const path = require("node:path");
const here = (p) => path.relative(process.cwd(), p);

console.log("main:", require.main === module, module.id, here(__filename), here(__dirname));
console.log("this is exports:", this === module.exports, typeof require.resolve, arguments.length);

const a = require("./a.cjs");
console.log("a:", a.name, a.fromB, "b loaded:", require("./b.cjs").loadedAtEnd);
console.log("same instance:", require("./a.cjs") === a);
try {
  // require() adds .js, .json, and .node, never .cjs.
  require("./a");
} catch (error) {
  console.log("no .cjs probing:", error.code);
}
console.log("children:", module.children.map((m) => here(m.filename)));
console.log("a.parent is main:", a.parentId);

// A module that throws is dropped from the cache, so it runs again.
for (let attempt = 0; attempt < 2; attempt++) {
  try {
    console.log("flaky:", require("./flaky.cjs"));
  } catch (error) {
    console.log("flaky threw:", error.message);
  }
}

// JSON through require: parsed once, the same object every time.
const data = require("./data.json");
console.log("json:", data.list.length, require("./data.json") === data, Object.keys(data));

// require() of a name only known at run time still finds the bundled module.
const which = "./a" + ".cjs";
console.log("dynamic require:", require(which) === a);

// A builtin through require.
console.log("builtin:", typeof require("node:os").platform, require("path") === path);

// A missing module throws Node's error.
try {
  require("./does-not-exist");
} catch (error) {
  console.log("missing:", error.code);
}
console.log("resolve:", here(require.resolve("./a.cjs")));
console.log("module keys:", Object.keys(module));
console.log("loaded during run:", module.loaded);
setTimeout(() => console.log("loaded after:", module.loaded), 0);
module.exports = { done: true };
