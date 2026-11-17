// Ported from jest/packages/jest-diff/src/normalizeDiffOptions.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "noColor", {enumerable: true, get: () => noColor});
Object.defineProperty(exports, "normalizeDiffOptions", {enumerable: true, get: () => normalizeDiffOptions});
const _m0 = require("./support.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */


                                               
                                                                

const noColor = (string        )         => string;

const DIFF_CONTEXT_DEFAULT = 5;

const OPTIONS_DEFAULT                        = {
  aAnnotation: 'Expected',
  aColor: (_m0.default ?? _m0).green,
  aIndicator: '-',
  bAnnotation: 'Received',
  bColor: (_m0.default ?? _m0).red,
  bIndicator: '+',
  changeColor: (_m0.default ?? _m0).inverse,
  changeLineTrailingSpaceColor: noColor,
  commonColor: (_m0.default ?? _m0).dim,
  commonIndicator: ' ',
  commonLineTrailingSpaceColor: noColor,
  compareKeys: undefined,
  contextLines: DIFF_CONTEXT_DEFAULT,
  emptyFirstOrLastLinePlaceholder: '',
  expand: true,
  includeChangeCounts: false,
  omitAnnotationLines: false,
  patchColor: (_m0.default ?? _m0).yellow,
};

const getCompareKeys = (compareKeys              )              =>
  compareKeys && typeof compareKeys === 'function'
    ? compareKeys
    : OPTIONS_DEFAULT.compareKeys;

const getContextLines = (contextLines         )         =>
  typeof contextLines === 'number' &&
  Number.isSafeInteger(contextLines) &&
  contextLines >= 0
    ? contextLines
    : DIFF_CONTEXT_DEFAULT;

// Pure function returns options with all properties.
const normalizeDiffOptions = (
  options              = {},
)                        => ({
  ...OPTIONS_DEFAULT,
  ...options,
  compareKeys: getCompareKeys(options.compareKeys),
  contextLines: getContextLines(options.contextLines),
});
