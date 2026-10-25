'use strict';

// The global DOMException, the class internalBinding('messaging') makes
// from Node's internal/per_context/domexception.js.

module.exports = { DOMException: internalBinding('messaging').DOMException };
