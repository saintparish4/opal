// Ported from jest/packages/expect-utils/src/immutableUtils.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "isImmutableUnorderedKeyed", {enumerable: true, get: () => isImmutableUnorderedKeyed});
Object.defineProperty(exports, "isImmutableUnorderedSet", {enumerable: true, get: () => isImmutableUnorderedSet});
Object.defineProperty(exports, "isImmutableList", {enumerable: true, get: () => isImmutableList});
Object.defineProperty(exports, "isImmutableOrderedKeyed", {enumerable: true, get: () => isImmutableOrderedKeyed});
Object.defineProperty(exports, "isImmutableOrderedSet", {enumerable: true, get: () => isImmutableOrderedSet});
Object.defineProperty(exports, "isImmutableRecord", {enumerable: true, get: () => isImmutableRecord});


/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 */

// SENTINEL constants are from https://github.com/immutable-js/immutable-js/tree/main/src/predicates
const IS_KEYED_SENTINEL = '@@__IMMUTABLE_KEYED__@@';
const IS_SET_SENTINEL = '@@__IMMUTABLE_SET__@@';
const IS_LIST_SENTINEL = '@@__IMMUTABLE_LIST__@@';
const IS_ORDERED_SENTINEL = '@@__IMMUTABLE_ORDERED__@@';
const IS_RECORD_SYMBOL = '@@__IMMUTABLE_RECORD__@@';

function isObjectLiteral(source         )                                    {
  return source != null && typeof source === 'object' && !Array.isArray(source);
}

function isImmutableUnorderedKeyed(source         )          {
  return Boolean(
    source &&
    isObjectLiteral(source) &&
    source[IS_KEYED_SENTINEL] &&
    !source[IS_ORDERED_SENTINEL],
  );
}

function isImmutableUnorderedSet(source         )          {
  return Boolean(
    source &&
    isObjectLiteral(source) &&
    source[IS_SET_SENTINEL] &&
    !source[IS_ORDERED_SENTINEL],
  );
}

function isImmutableList(source         )          {
  return Boolean(source && isObjectLiteral(source) && source[IS_LIST_SENTINEL]);
}

function isImmutableOrderedKeyed(source         )          {
  return Boolean(
    source &&
    isObjectLiteral(source) &&
    source[IS_KEYED_SENTINEL] &&
    source[IS_ORDERED_SENTINEL],
  );
}

function isImmutableOrderedSet(source         )          {
  return Boolean(
    source &&
    isObjectLiteral(source) &&
    source[IS_SET_SENTINEL] &&
    source[IS_ORDERED_SENTINEL],
  );
}

function isImmutableRecord(source         )          {
  return Boolean(source && isObjectLiteral(source) && source[IS_RECORD_SYMBOL]);
}
