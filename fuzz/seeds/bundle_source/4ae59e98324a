"use strict";
const a = require("./a.cjs");
exports.sawName = a.name + ":" + Object.keys(a).join(",");
exports.loadedAtEnd = false;
process.nextTick(() => {
  exports.loadedAtEnd = module.loaded;
});
