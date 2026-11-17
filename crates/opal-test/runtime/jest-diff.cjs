// Ported from jest/packages/jest-diff/src/index.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "diffLinesRaw", {enumerable: true, get: () => _m5.diffLinesRaw});
Object.defineProperty(exports, "diffLinesUnified", {enumerable: true, get: () => _m5.diffLinesUnified});
Object.defineProperty(exports, "diffLinesUnified2", {enumerable: true, get: () => _m5.diffLinesUnified2});
Object.defineProperty(exports, "diffStringsRaw", {enumerable: true, get: () => _m8.diffStringsRaw});
Object.defineProperty(exports, "diffStringsUnified", {enumerable: true, get: () => _m8.diffStringsUnified});
Object.defineProperty(exports, "DIFF_DELETE", {enumerable: true, get: () => _m3.DIFF_DELETE});
Object.defineProperty(exports, "DIFF_EQUAL", {enumerable: true, get: () => _m3.DIFF_EQUAL});
Object.defineProperty(exports, "DIFF_INSERT", {enumerable: true, get: () => _m3.DIFF_INSERT});
Object.defineProperty(exports, "Diff", {enumerable: true, get: () => _m3.Diff});
Object.defineProperty(exports, "diff", {enumerable: true, get: () => diff});
const _m0 = require("./support.cjs");
const _m1 = require("./jest-get-type.cjs");
const _m2 = require("./format.cjs");
const _m3 = require("./jest-diff-cleanupSemantic.cjs");
const _m4 = require("./jest-diff-constants.cjs");
const _m5 = require("./jest-diff-diffLines.cjs");
const _m6 = require("./jest-diff-escapeControlCharacters.cjs");
const _m7 = require("./jest-diff-normalizeDiffOptions.cjs");
const _m8 = require("./jest-diff-printDiffs.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */










                                         

                                                           





const getCommonMessage = (message        , options              ) => {
  const {commonColor} = _m7.normalizeDiffOptions(options);
  return commonColor(message);
};

const {
  AsymmetricMatcher,
  DOMCollection,
  DOMElement,
  Immutable,
  ReactElement,
  ReactTestComponent,
} = _m2.plugins;

const PLUGINS = [
  ReactTestComponent,
  ReactElement,
  DOMElement,
  DOMCollection,
  Immutable,
  AsymmetricMatcher,
];
const FORMAT_OPTIONS = {
  plugins: PLUGINS,
};
const FALLBACK_FORMAT_OPTIONS = {
  callToJSON: false,
  maxDepth: 10,
  plugins: PLUGINS,
};

// Generate a string that will highlight the difference between two values
// with green and red. (similar to how github does code diffing)
// eslint-disable-next-line @typescript-eslint/explicit-module-boundary-types
function diff(a     , b     , options              )                {
  if (Object.is(a, b)) {
    return getCommonMessage(_m4.NO_DIFF_MESSAGE, options);
  }

  const aType = _m1.getType(a);
  let expectedType = aType;
  let omitDifference = false;
  if (aType === 'object' && typeof a.asymmetricMatch === 'function') {
    if (a.$$typeof !== Symbol.for('jest.asymmetricMatcher')) {
      // Do not know expected type of user-defined asymmetric matcher.
      return null;
    }
    if (typeof a.getExpectedType !== 'function') {
      // For example, expect.anything() matches either null or undefined
      return null;
    }
    expectedType = a.getExpectedType();
    // Primitive types boolean and number omit difference below.
    // For example, omit difference for expect.stringMatching(regexp)
    omitDifference = expectedType === 'string';
  }

  if (expectedType !== _m1.getType(b)) {
    return (
      '  Comparing two different types of values.' +
      ` Expected ${(_m0.default ?? _m0).green(expectedType)} but ` +
      `received ${(_m0.default ?? _m0).red(_m1.getType(b))}.`
    );
  }

  if (omitDifference) {
    return null;
  }

  switch (aType) {
    case 'string':
      return _m5.diffLinesUnified(
        _m6.escapeControlCharacters(a).split('\n'),
        _m6.escapeControlCharacters(b).split('\n'),
        options,
      );
    case 'boolean':
    case 'number':
      return comparePrimitive(a, b, options);
    case 'map':
      return compareObjects(sortMap(a), sortMap(b), options);
    case 'set':
      return compareObjects(sortSet(a), sortSet(b), options);
    default:
      return compareObjects(a, b, options);
  }
}

function comparePrimitive(
  a                  ,
  b                  ,
  options              ,
) {
  const aFormat = _m2.format(a, FORMAT_OPTIONS);
  const bFormat = _m2.format(b, FORMAT_OPTIONS);
  return aFormat === bFormat
    ? getCommonMessage(_m4.NO_DIFF_MESSAGE, options)
    : _m5.diffLinesUnified(aFormat.split('\n'), bFormat.split('\n'), options);
}

function sortMap(map                       ) {
  return new Map([...map].sort());
}

function sortSet(set              ) {
  return new Set([...set].sort());
}

function compareObjects(
  a                     ,
  b                     ,
  options              ,
) {
  let difference;
  let hasThrown = false;

  try {
    const formatOptions = getFormatOptions(FORMAT_OPTIONS, options);
    difference = getObjectsDifference(a, b, formatOptions, options);
  } catch {
    hasThrown = true;
  }

  const noDiffMessage = getCommonMessage(_m4.NO_DIFF_MESSAGE, options);
  // If the comparison yields no results, compare again but this time
  // without calling `toJSON`. It's also possible that toJSON might throw.
  if (difference === undefined || difference === noDiffMessage) {
    const formatOptions = getFormatOptions(FALLBACK_FORMAT_OPTIONS, options);
    difference = getObjectsDifference(a, b, formatOptions, options);

    if (difference !== noDiffMessage && !hasThrown) {
      difference = `${getCommonMessage(
        _m4.SIMILAR_MESSAGE,
        options,
      )}\n\n${difference}`;
    }
  }

  return difference;
}

function getFormatOptions(
  formatOptions                     ,
  options              ,
)                      {
  const {compareKeys} = _m7.normalizeDiffOptions(options);

  return {
    ...formatOptions,
    compareKeys,
  };
}

function getObjectsDifference(
  a                     ,
  b                     ,
  formatOptions                     ,
  options              ,
)         {
  const formatOptionsZeroIndent = {...formatOptions, indent: 0};
  const aCompare = _m2.format(a, formatOptionsZeroIndent);
  const bCompare = _m2.format(b, formatOptionsZeroIndent);

  if (aCompare === bCompare) {
    return getCommonMessage(_m4.NO_DIFF_MESSAGE, options);
  } else {
    const aDisplay = _m2.format(a, formatOptions);
    const bDisplay = _m2.format(b, formatOptions);

    return _m5.diffLinesUnified2(
      aDisplay.split('\n'),
      bDisplay.split('\n'),
      aCompare.split('\n'),
      bCompare.split('\n'),
      options,
    );
  }
}
