'use strict';

// Opal's node:util until Node's own lib/util.js can be vendored: that file
// requires buffer, internal/util/debuglog and internal/streams/utils as it
// loads, which Opal doesn't have yet. Everything exported here is Node's
// own code, from the internal modules that are vendored, or (inherits)
// copied from lib/util.js; what node:util has and this lacks is absent
// rather than approximated.

const {
  ArrayIsArray,
  ObjectDefineProperty,
  ObjectSetPrototypeOf,
  StringPrototypeToWellFormed,
} = primordials;

const {
  codes: { ERR_INVALID_ARG_TYPE },
} = require('internal/errors');
const {
  format,
  formatWithOptions,
  inspect,
  stripVTControlCharacters,
} = require('internal/util/inspect');
const { deprecate, promisify } = require('internal/util');
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

module.exports = {
  deprecate,
  format,
  formatWithOptions,
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
