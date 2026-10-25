exports.loaded = false; const b = require("./cycle-b"); exports.loaded = true; module.exports.fromB = b;
