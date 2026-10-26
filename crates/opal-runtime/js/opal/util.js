'use strict';

// Opal's node:util until Node's own lib/util.js can be vendored: that file
// requires buffer, internal/util/debuglog and internal/streams/utils as it
// loads, which Opal doesn't have yet. Everything exported here is Node's
// own code, from the internal modules that are vendored, or (inherits)
// copied from lib/util.js; what node:util has and this lacks is absent
// rather than approximated.

const {
  ArrayIsArray,
  NumberIsSafeInteger,
  ObjectDefineProperty,
  ObjectSetPrototypeOf,
  StringPrototypeToWellFormed,
} = primordials;

const {
  codes: { ERR_INVALID_ARG_TYPE, ERR_OUT_OF_RANGE },
} = require('internal/errors');
const {
  format,
  formatWithOptions,
  inspect,
  stripVTControlCharacters,
} = require('internal/util/inspect');
const {
  deprecate,
  getSystemErrorMap,
  getSystemErrorMessage: internalErrorMessage,
  getSystemErrorName: internalErrorName,
  promisify,
} = require('internal/util');
const {
  validateBoolean,
  validateInteger,
  validateNumber,
  validateObject,
} = require('internal/validators');
const { getOptionValue } = require('internal/options');
const binding = internalBinding('util');
const types = require('internal/util/types');

// From lib/util.js.
function inherits(ctor, superCtor) {

  if (ctor === undefined || ctor === null)
    throw new ERR_INVALID_ARG_TYPE('ctor', 'Function', ctor);

  if (superCtor === undefined || superCtor === null)
    throw new ERR_INVALID_ARG_TYPE('superCtor', 'Function', superCtor);

  if (superCtor.prototype === undefined) {
    throw new ERR_INVALID_ARG_TYPE('superCtor.prototype',
                                   'Object', superCtor.prototype);
  }
  ObjectDefineProperty(ctor, 'super_', {
    __proto__: null,
    value: superCtor,
    writable: true,
    configurable: true,
  });
  ObjectSetPrototypeOf(ctor.prototype, superCtor.prototype);
}

// From lib/util.js.
/**
 * @param {number} err
 * @returns {string}
 */
function getSystemErrorName(err) {
  validateNumber(err, 'err');
  if (err >= 0 || !NumberIsSafeInteger(err)) {
    throw new ERR_OUT_OF_RANGE('err', 'a negative integer', err);
  }
  return internalErrorName(err);
}

// From lib/util.js.
/**
 * @param {number} err
 * @returns {string}
 */
function getSystemErrorMessage(err) {
  validateNumber(err, 'err');
  if (err >= 0 || !NumberIsSafeInteger(err)) {
    throw new ERR_OUT_OF_RANGE('err', 'a negative integer', err);
  }
  return internalErrorMessage(err);
}

// From lib/util.js, without source maps, which Opal doesn't support: with
// none, mapCallSite is never reached.
/**
 * Returns the callSite
 * @param {number} frameCount
 * @param {CallSiteOptions} options
 * @returns {CallSite[]}
 */
function getCallSites(frameCount = 10, options) {
  // If options is not provided check if frameCount is an object
  if (options === undefined) {
    if (typeof frameCount === 'object') {
      // If frameCount is an object, it is the options object
      options = frameCount;
      validateObject(options, 'options');
      if (options.sourceMap !== undefined) {
        validateBoolean(options.sourceMap, 'options.sourceMap');
      }
      frameCount = 10;
    } else {
      // If options is not provided, set it to an empty object
      options = {};
    };
  } else {
    // If options is provided, validate it
    validateObject(options, 'options');
    if (options.sourceMap !== undefined) {
      validateBoolean(options.sourceMap, 'options.sourceMap');
    }
  }

  // Using kDefaultMaxCallStackSizeToCapture as reference
  validateInteger(frameCount, 'frameCount', 1, 200);
  if (options.sourceMap === true || (getOptionValue('--enable-source-maps') && options.sourceMap !== false)) {
    const error = new Error('util.getCallSites with source maps is not supported by Opal yet');
    error.code = 'ERR_OPAL_UNIMPLEMENTED';
    throw error;
  }
  return binding.getCallSites(frameCount);
};

module.exports = {
  deprecate,
  format,
  formatWithOptions,
  getCallSites,
  getSystemErrorMap,
  getSystemErrorMessage,
  getSystemErrorName,
  inherits,
  inspect,
  isArray: deprecate(ArrayIsArray,
                     'The `util.isArray` API is deprecated. Please use `Array.isArray()` instead.',
                     'DEP0044'),
  promisify,
  stripVTControlCharacters,
  toUSVString(input) {
    return StringPrototypeToWellFormed(`${input}`);
  },
  types,
};
