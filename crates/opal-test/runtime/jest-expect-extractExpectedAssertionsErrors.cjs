// Ported from jest/packages/expect/src/extractExpectedAssertionsErrors.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, 'default', {enumerable: true, get: () => _default});
const _m0 = require("./jest-matcher-utils.cjs");
const _m1 = require("./jest-expect-jestMatchersObject.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 */



                                                              

const resetAssertionsLocalState = () => {
  _m1.setState({
    assertionCalls: 0,
    expectedAssertionsNumber: null,
    isExpectingAssertions: false,
    numPassingAsserts: 0,
  });
};

// Create and format all errors related to the mismatched number of `expect`
// calls and reset the matcher's state.
const extractExpectedAssertionsErrors                                            =
  () => {
    const result                           = [];
    const {
      assertionCalls,
      expectedAssertionsNumber,
      expectedAssertionsNumberError,
      isExpectingAssertions,
      isExpectingAssertionsError,
    } = _m1.getState();

    resetAssertionsLocalState();

    if (
      typeof expectedAssertionsNumber === 'number' &&
      assertionCalls !== expectedAssertionsNumber
    ) {
      const numOfAssertionsExpected = _m0.EXPECTED_COLOR(
        _m0.pluralize('assertion', expectedAssertionsNumber),
      );

      expectedAssertionsNumberError .message =
        `${_m0.matcherHint('.assertions', '', expectedAssertionsNumber.toString(), {
          isDirectExpectCall: true,
        })}\n\n` +
        `Expected ${numOfAssertionsExpected} to be called but received ${_m0.RECEIVED_COLOR(
          _m0.pluralize('assertion call', assertionCalls || 0),
        )}.`;

      result.push({
        actual: assertionCalls.toString(),
        error: expectedAssertionsNumberError ,
        expected: expectedAssertionsNumber.toString(),
      });
    }
    if (isExpectingAssertions && assertionCalls === 0) {
      const expected = _m0.EXPECTED_COLOR('at least one assertion');
      const received = _m0.RECEIVED_COLOR('received none');

      isExpectingAssertionsError .message = `${_m0.matcherHint(
        '.hasAssertions',
        '',
        '',
        {isDirectExpectCall: true},
      )}\n\nExpected ${expected} to be called but ${received}.`;

      result.push({
        actual: 'none',
        error: isExpectingAssertionsError ,
        expected: 'at least one',
      });
    }

    return result;
  };

const _default = extractExpectedAssertionsErrors;
