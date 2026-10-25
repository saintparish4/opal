'use strict';

// Opal's internal/url, until node:url is implemented. Node's library asks
// it whether a value is a URL (util.inspect formats one by its href); with
// no URL class in the runtime yet, nothing is one. Any other use is an
// error naming what is missing.

class URL {}

// From lib/internal/url.js.
/**
 * Checks if a value has the shape of a WHATWG URL object.
 *
 * Using a symbol or instanceof would not be able to recognize URL objects
 * coming from other implementations (e.g. in Electron), so instead we are
 * checking some well known properties for a lack of a better test.
 *
 * We use `href` and `protocol` as they are the only properties that are
 * easy to retrieve and calculate due to the lazy nature of the getters.
 *
 * We check for `auth` and `path` attribute to distinguish legacy url instance with
 * WHATWG URL instance.
 * @param {*} self
 * @returns {self is URL}
 */
function isURL(self) {
  return Boolean(self?.href && self.protocol && self.auth === undefined && self.path === undefined);
}

// Functions a vendored file takes when it loads but only calls on a path
// Opal doesn't support yet (source maps, say): calling one is the error.
const missing = (name) => () => {
  throw new Error(`internal/url.${name} is not implemented by Opal yet`);
};

const fileURLToPath = missing('fileURLToPath');

// From lib/internal/url.js; a URL object reaches fileURLToPath, which says
// it isn't implemented yet.
function toPathIfFileURL(fileURLOrPath) {
  if (!isURL(fileURLOrPath))
    return fileURLOrPath;
  return fileURLToPath(fileURLOrPath);
}

module.exports = new Proxy({
  URL,
  isURL,
  toPathIfFileURL,
  fileURLToPath,
  pathToFileURL: missing('pathToFileURL'),
  URLParse: missing('URLParse'),
}, {
  get(target, key) {
    if (key in target) return target[key];
    throw new Error(`internal/url.${String(key)} is not implemented by Opal yet`);
  },
});
