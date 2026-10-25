'use strict';

// Opal's internal/fs/read/context: fs.readFile's state machine, which runs
// on the event loop's file I/O. Opal's loop has no I/O yet, so making one
// fails the way every fs function with a callback does (realm.js's
// FSReqCallback), naming what is missing.

class ReadFileContext {
  constructor() {
    const error = new Error('Opal implements only the synchronous fs functions yet, not fs functions with a callback');
    error.code = 'ERR_OPAL_UNIMPLEMENTED';
    throw error;
  }
}

module.exports = ReadFileContext;
