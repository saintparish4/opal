'use strict';

// Opal's internal/console/global: the global console, which is the
// bootstrap's (js/bootstrap/console.js) rather than Node's stream-backed one.

module.exports = require('internal/opal/internals').console;
