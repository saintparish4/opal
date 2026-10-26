"use strict";
exports.name = "a";
// A cycle: b requires a while a is loading and sees a's partial exports.
const b = require("./b.cjs");
exports.fromB = b.sawName;
exports.parentId = module.parent.id;
