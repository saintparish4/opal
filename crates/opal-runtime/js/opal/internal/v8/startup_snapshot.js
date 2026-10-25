'use strict';

// Opal's internal/v8/startup_snapshot, for the part Node's library uses:
// a module loaded while build.rs makes the snapshot can ask to run code
// again when a run starts (lib/buffer.js re-creates its pool this way).
// The callbacks run from main.js's start, before the main module.

const internals = require('internal/opal/internals');

module.exports = {
  namespace: {
    isBuildingSnapshot() {
      return internals.buildingSnapshot;
    },
    addDeserializeCallback(callback, data) {
      internals.deserializeCallbacks.push([callback, data]);
    },
    addSerializeCallback() {},
    setDeserializeMainFunction() {
      throw new Error('setDeserializeMainFunction is not supported by Opal');
    },
  },
};
