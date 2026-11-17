// Ported from jest/packages/expect/src/print.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "printReceivedStringContainExpectedSubstring", {enumerable: true, get: () => printReceivedStringContainExpectedSubstring});
Object.defineProperty(exports, "printReceivedStringContainExpectedResult", {enumerable: true, get: () => printReceivedStringContainExpectedResult});
Object.defineProperty(exports, "printReceivedArrayContainExpectedItem", {enumerable: true, get: () => printReceivedArrayContainExpectedItem});
Object.defineProperty(exports, "printCloseTo", {enumerable: true, get: () => printCloseTo});
Object.defineProperty(exports, "printExpectedConstructorName", {enumerable: true, get: () => printExpectedConstructorName});
Object.defineProperty(exports, "printExpectedConstructorNameNot", {enumerable: true, get: () => printExpectedConstructorNameNot});
Object.defineProperty(exports, "printReceivedConstructorName", {enumerable: true, get: () => printReceivedConstructorName});
Object.defineProperty(exports, "printReceivedConstructorNameNot", {enumerable: true, get: () => printReceivedConstructorNameNot});
const _m0 = require("./jest-matcher-utils.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 */



// Format substring but do not enclose in double quote marks.
// The replacement is compatible with pretty-format package.
const printSubstring = (val        )         => val.replaceAll(/"|\\/g, '\\$&');

const printReceivedStringContainExpectedSubstring = (
  received        ,
  start        ,
  length        , // not end
)         =>
  _m0.RECEIVED_COLOR(
    `"${printSubstring(received.slice(0, start))}${_m0.INVERTED_COLOR(
      printSubstring(received.slice(start, start + length)),
    )}${printSubstring(received.slice(start + length))}"`,
  );

const printReceivedStringContainExpectedResult = (
  received        ,
  result                        ,
)         =>
  result === null
    ? _m0.printReceived(received)
    : printReceivedStringContainExpectedSubstring(
        received,
        result.index,
        result[0].length,
      );

// The serialized array is compatible with pretty-format package min option.
// However, items have default stringify depth (instead of depth - 1)
// so expected item looks consistent by itself and enclosed in the array.
const printReceivedArrayContainExpectedItem = (
  received                ,
  index        ,
)         =>
  _m0.RECEIVED_COLOR(
    `[${received
      .map((item, i) => {
        const stringified = _m0.stringify(item);
        return i === index ? _m0.INVERTED_COLOR(stringified) : stringified;
      })
      .join(', ')}]`,
  );

const printCloseTo = (
  receivedDiff        ,
  expectedDiff        ,
  precision        ,
  isNot                     ,
)         => {
  const receivedDiffString = _m0.stringify(receivedDiff);
  const expectedDiffString = receivedDiffString.includes('e')
    ? // toExponential arg is number of digits after the decimal point.
      expectedDiff.toExponential(0)
    : 0 <= precision && precision < 20
      ? // toFixed arg is number of digits after the decimal point.
        // It may be a value between 0 and 20 inclusive.
        // Implementations may optionally support a larger range of values.
        expectedDiff.toFixed(precision + 1)
      : _m0.stringify(expectedDiff);

  return (
    `Expected precision:  ${isNot ? '    ' : ''}  ${_m0.stringify(precision)}\n` +
    `Expected difference: ${isNot ? 'not ' : ''}< ${_m0.EXPECTED_COLOR(
      expectedDiffString,
    )}\n` +
    `Received difference: ${isNot ? '    ' : ''}  ${_m0.RECEIVED_COLOR(
      receivedDiffString,
    )}`
  );
};

const printExpectedConstructorName = (
  label        ,
  expected          ,
)         => `${printConstructorName(label, expected, false, true)}\n`;

const printExpectedConstructorNameNot = (
  label        ,
  expected          ,
)         => `${printConstructorName(label, expected, true, true)}\n`;

const printReceivedConstructorName = (
  label        ,
  received          ,
)         => `${printConstructorName(label, received, false, false)}\n`;

// Do not call function if received is equal to expected.
const printReceivedConstructorNameNot = (
  label        ,
  received          ,
  expected          ,
)         =>
  typeof expected.name === 'string' &&
  expected.name.length > 0 &&
  typeof received.name === 'string' &&
  received.name.length > 0
    ? `${printConstructorName(label, received, true, false)} ${
        Object.getPrototypeOf(received) === expected
          ? 'extends'
          : 'extends … extends'
      } ${_m0.EXPECTED_COLOR(expected.name)}\n`
    : `${printConstructorName(label, received, false, false)}\n`;

const printConstructorName = (
  label        ,
  constructor          ,
  isNot         ,
  isExpected         ,
)         =>
  typeof constructor.name === 'string'
    ? constructor.name.length === 0
      ? `${label} name is an empty string`
      : `${label}: ${isNot ? (isExpected ? 'not ' : '    ') : ''}${
          isExpected
            ? _m0.EXPECTED_COLOR(constructor.name)
            : _m0.RECEIVED_COLOR(constructor.name)
        }`
    : `${label} name is not a string`;
