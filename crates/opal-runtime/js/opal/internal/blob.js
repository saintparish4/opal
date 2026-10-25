'use strict';

// Opal's internal/blob, until Blob is implemented. lib/fs.js takes
// createBlobFromFilePath from it when it loads, for fs.openAsBlob; using
// it, or anything else here, is an error naming what is missing.

module.exports = new Proxy({}, {
  get(target, key) {
    return () => {
      throw new Error(`internal/blob.${String(key)} is not implemented by Opal yet`);
    };
  },
});
