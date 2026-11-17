// Ported from jest/packages/jest-diff/src/constants.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "NO_DIFF_MESSAGE", {enumerable: true, get: () => NO_DIFF_MESSAGE});
Object.defineProperty(exports, "SIMILAR_MESSAGE", {enumerable: true, get: () => SIMILAR_MESSAGE});


/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const NO_DIFF_MESSAGE = 'Compared values have no visual difference.';

const SIMILAR_MESSAGE =
  'Compared values serialize to the same structure.\n' +
  'Printing internal object structure without calling `toJSON` instead.';
