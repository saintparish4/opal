// Ported from jest/packages/jest-diff/src/printDiffs.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "diffStringsUnified", {enumerable: true, get: () => diffStringsUnified});
Object.defineProperty(exports, "diffStringsRaw", {enumerable: true, get: () => diffStringsRaw});
const _m0 = require("./jest-diff-cleanupSemantic.cjs");
const _m1 = require("./jest-diff-diffLines.cjs");
const _m2 = require("./jest-diff-diffStrings.cjs");
const _m3 = require("./jest-diff-getAlignedDiffs.cjs");
const _m4 = require("./jest-diff-normalizeDiffOptions.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */






                                         

const hasCommonDiff = (diffs             , isMultiline         )          => {
  if (isMultiline) {
    // Important: Ignore common newline that was appended to multiline strings!
    const iLast = diffs.length - 1;
    return diffs.some(
      (diff, i) => diff[0] === _m0.DIFF_EQUAL && (i !== iLast || diff[1] !== '\n'),
    );
  }

  return diffs.some(diff => diff[0] === _m0.DIFF_EQUAL);
};

// Compare two strings character-by-character.
// Format as comparison lines in which changed substrings have inverse colors.
const diffStringsUnified = (
  a        ,
  b        ,
  options              ,
)         => {
  if (a !== b && a.length > 0 && b.length > 0) {
    const isMultiline = a.includes('\n') || b.includes('\n');

    // getAlignedDiffs assumes that a newline was appended to the strings.
    const diffs = diffStringsRaw(
      isMultiline ? `${a}\n` : a,
      isMultiline ? `${b}\n` : b,
      true, // cleanupSemantic
    );

    if (hasCommonDiff(diffs, isMultiline)) {
      const optionsNormalized = _m4.normalizeDiffOptions(options);
      const lines = (_m3.default ?? _m3)(diffs, optionsNormalized.changeColor);
      return _m1.printDiffLines(lines, optionsNormalized);
    }
  }

  // Fall back to line-by-line diff.
  return _m1.diffLinesUnified(a.split('\n'), b.split('\n'), options);
};

// Compare two strings character-by-character.
// Optionally clean up small common substrings, also known as chaff.
const diffStringsRaw = (
  a        ,
  b        ,
  cleanup         ,
)              => {
  const diffs = (_m2.default ?? _m2)(a, b);

  if (cleanup) {
    _m0.cleanupSemantic(diffs); // impure function
  }

  return diffs;
};
