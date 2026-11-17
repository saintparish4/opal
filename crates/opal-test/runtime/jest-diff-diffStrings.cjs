// Ported from jest/packages/jest-diff/src/diffStrings.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, 'default', {enumerable: true, get: () => _default});
const _m0 = require("./jest-diff-sequences.cjs");
const _m1 = require("./jest-diff-cleanupSemantic.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */




const diffStrings = (a        , b        )              => {
  const isCommon = (aIndex        , bIndex        ) => a[aIndex] === b[bIndex];

  let aIndex = 0;
  let bIndex = 0;
  const diffs              = [];

  const foundSubsequence = (
    nCommon        ,
    aCommon        ,
    bCommon        ,
  ) => {
    if (aIndex !== aCommon) {
      diffs.push(new _m1.Diff(_m1.DIFF_DELETE, a.slice(aIndex, aCommon)));
    }
    if (bIndex !== bCommon) {
      diffs.push(new _m1.Diff(_m1.DIFF_INSERT, b.slice(bIndex, bCommon)));
    }

    aIndex = aCommon + nCommon; // number of characters compared in a
    bIndex = bCommon + nCommon; // number of characters compared in b
    diffs.push(new _m1.Diff(_m1.DIFF_EQUAL, b.slice(bCommon, bIndex)));
  };

  (_m0.default ?? _m0)(a.length, b.length, isCommon, foundSubsequence);

  // After the last common subsequence, push remaining change items.
  if (aIndex !== a.length) {
    diffs.push(new _m1.Diff(_m1.DIFF_DELETE, a.slice(aIndex)));
  }
  if (bIndex !== b.length) {
    diffs.push(new _m1.Diff(_m1.DIFF_INSERT, b.slice(bIndex)));
  }

  return diffs;
};

const _default = diffStrings;
