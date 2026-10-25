'use strict';

// Opal's internal/options. Node's reads the command line's --flags out of
// C++ (internalBinding('options')); `opal run` takes no Node flags, so every
// option has its default value. An option not listed here is one no
// vendored file asks for yet, and asking is an error, so a new one is added
// with its real default rather than read as undefined.

const defaults = {
  __proto__: null,
  '--diagnostic-dir': '',
  '--disable-warning': [],
  '--expose-internals': false,
  '--frozen-intrinsics': false,
  '--no-deprecation': false,
  '--pending-deprecation': false,
  '--preserve-symlinks': false,
  '--preserve-symlinks-main': false,
  '--redirect-warnings': '',
  '--throw-deprecation': false,
  '--trace-deprecation': false,
  '--trace-warnings': false,
  '--warnings': true,
};

function getOptionValue(name) {
  if (!(name in defaults)) {
    throw new Error(`Opal has no default for the Node option ${name}`);
  }
  return defaults[name];
}

function getEmbedderOptions() {
  return {
    shouldNotRegisterESMLoader: true,
    noGlobalSearchPaths: true,
    hasEmbedderPreload: false,
  };
}

module.exports = {
  getOptionValue,
  getEmbedderOptions,
  getAllowUnauthorized: () => false,
  refreshOptions() {},
};
