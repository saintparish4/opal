// Ported from jest/packages/jest-diff/src/escapeControlCharacters.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "escapeControlCharacters", {enumerable: true, get: () => escapeControlCharacters});


/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// Escape control characters to make them visible in diffs
const escapeControlCharacters = (str        )         =>
  str.replaceAll(
    /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F]/g,
    (match        ) => {
      switch (match) {
        case '\b':
          return '\\b';
        case '\f':
          return '\\f';
        case '\v':
          return '\\v';
        default: {
          const code = match.codePointAt(0);
          return `\\x${code .toString(16).padStart(2, '0')}`;
        }
      }
    },
  );
