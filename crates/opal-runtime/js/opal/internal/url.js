'use strict';

// Opal's internal/url, until node:url is implemented. Node's library asks
// it whether a value is a URL (util.inspect formats one by its href); with
// no URL class in the runtime yet, nothing is one. Any other use is an
// error naming what is missing.

class URL {}

// With no URL objects, every path is a path.
const toPathIfFileURL = (fileURLOrPath) => fileURLOrPath;

module.exports = new Proxy({ URL, toPathIfFileURL }, {
  get(target, key) {
    if (key in target) return target[key];
    throw new Error(`internal/url.${String(key)} is not implemented by Opal yet`);
  },
});
