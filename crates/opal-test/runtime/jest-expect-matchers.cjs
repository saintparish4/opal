// Ported from jest/packages/expect/src/matchers.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, 'default', {enumerable: true, get: () => _default});
const _m0 = require("./jest-expect-utils.cjs");
const _m1 = require("./jest-get-type.cjs");
const _m2 = require("./jest-matcher-utils.cjs");
const _m3 = require("./jest-expect-print.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 */





                                            

// Omit colon and one or more spaces, so can call getLabelPrinter.
const EXPECTED_LABEL = 'Expected';
const RECEIVED_LABEL = 'Received';
const EXPECTED_VALUE_LABEL = 'Expected value';
const RECEIVED_VALUE_LABEL = 'Received value';

// The optional property of matcher context is true if undefined.
const isExpand = (expand          )          => expand !== false;

const toStrictEqualTesters = [
  _m0.iterableEquality,
  _m0.typeEquality,
  _m0.sparseArrayEquality,
  _m0.arrayBufferEquality,
];

                      
                  
                
                    
                
                          

const matchers                 = {
  toBe(received         , expected         ) {
    const matcherName = 'toBe';
    const options                     = {
      comment: 'Object.is equality',
      isNot: this.isNot,
      promise: this.promise,
    };

    const pass = Object.is(received, expected);

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          `Expected: not ${_m2.printExpected(expected)}`
      : () => {
          const expectedType = _m1.getType(expected);

          let deepEqualityName = null;
          if (expectedType !== 'map' && expectedType !== 'set') {
            // If deep equality passes when referential identity fails,
            // but exclude map and set until review of their equality logic.
            if (
              _m0.equals(
                received,
                expected,
                [...this.customTesters, ...toStrictEqualTesters],
                true,
              )
            ) {
              deepEqualityName = 'toStrictEqual';
            } else if (
              _m0.equals(received, expected, [
                ...this.customTesters,
                _m0.iterableEquality,
              ])
            ) {
              deepEqualityName = 'toEqual';
            }
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(matcherName, undefined, undefined, options) +
            '\n\n' +
            (deepEqualityName === null
              ? ''
              : `${_m2.DIM_COLOR(
                  `If it should pass with deep equality, replace "${matcherName}" with "${deepEqualityName}"`,
                )}\n\n`) +
            _m2.printDiffOrStringify(
              expected,
              received,
              EXPECTED_LABEL,
              RECEIVED_LABEL,
              isExpand(this.expand),
            )
          );
        };

    // Passing the actual and expected objects so that a custom reporter
    // could access them, for example in order to display a custom visual diff,
    // or create a different error message
    return {actual: received, expected, message, name: matcherName, pass};
  },

  toBeCloseTo(received        , expected        , precision = 2) {
    const matcherName = 'toBeCloseTo';
    const secondArgument = arguments.length === 3 ? 'precision' : undefined;
    const isNot = this.isNot;
    const options                     = {
      isNot,
      promise: this.promise,
      secondArgument,
      secondArgumentColor: (arg        ) => arg,
    };

    if (typeof expected !== 'number') {
      throw new TypeError(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.EXPECTED_COLOR('expected')} value must be a number`,
          _m2.printWithType('Expected', expected, _m2.printExpected),
        ),
      );
    }

    if (typeof received !== 'number') {
      throw new TypeError(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.RECEIVED_COLOR('received')} value must be a number`,
          _m2.printWithType('Received', received, _m2.printReceived),
        ),
      );
    }

    let pass         ;
    let expectedDiff = 0;
    let receivedDiff = 0;

    if (
      received === Number.POSITIVE_INFINITY &&
      expected === Number.POSITIVE_INFINITY
    ) {
      pass = true; // Infinity - Infinity is NaN
    } else if (
      received === Number.NEGATIVE_INFINITY &&
      expected === Number.NEGATIVE_INFINITY
    ) {
      pass = true; // -Infinity - -Infinity is NaN
    } else {
      expectedDiff = Math.pow(10, -precision) / 2;
      receivedDiff = Math.abs(expected - received);
      pass = receivedDiff < expectedDiff;
    }

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          `Expected: not ${_m2.printExpected(expected)}\n` +
          (receivedDiff === 0
            ? ''
            : `Received:     ${_m2.printReceived(received)}\n` +
              `\n${_m3.printCloseTo(receivedDiff, expectedDiff, precision, isNot)}`)
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          `Expected: ${_m2.printExpected(expected)}\n` +
          `Received: ${_m2.printReceived(received)}\n` +
          '\n' +
          _m3.printCloseTo(receivedDiff, expectedDiff, precision, isNot);

    return {message, pass};
  },

  toBeDefined(received         , expected      ) {
    const matcherName = 'toBeDefined';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureNoExpected(expected, matcherName, options);

    const pass = received !== void 0;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, '', options) +
      '\n\n' +
      `Received: ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeFalsy(received         , expected      ) {
    const matcherName = 'toBeFalsy';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureNoExpected(expected, matcherName, options);

    const pass = !received;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, '', options) +
      '\n\n' +
      `Received: ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeGreaterThan(received                 , expected                 ) {
    const matcherName = 'toBeGreaterThan';
    const isNot = this.isNot;
    const options                     = {
      isNot,
      promise: this.promise,
    };
    _m2.ensureNumbers(received, expected, matcherName, options);

    const pass = received > expected;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, undefined, options) +
      '\n\n' +
      `Expected:${isNot ? ' not' : ''} > ${_m2.printExpected(expected)}\n` +
      `Received:${isNot ? '    ' : ''}   ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeGreaterThanOrEqual(received                 , expected                 ) {
    const matcherName = 'toBeGreaterThanOrEqual';
    const isNot = this.isNot;
    const options                     = {
      isNot,
      promise: this.promise,
    };
    _m2.ensureNumbers(received, expected, matcherName, options);

    const pass = received >= expected;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, undefined, options) +
      '\n\n' +
      `Expected:${isNot ? ' not' : ''} >= ${_m2.printExpected(expected)}\n` +
      `Received:${isNot ? '    ' : ''}    ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeInstanceOf(received     , expected          ) {
    const matcherName = 'toBeInstanceOf';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };

    if (typeof expected !== 'function') {
      throw new TypeError(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.EXPECTED_COLOR('expected')} value must be a function`,
          _m2.printWithType('Expected', expected, _m2.printExpected),
        ),
      );
    }

    const pass = received instanceof expected;

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          _m3.printExpectedConstructorNameNot('Expected constructor', expected) +
          (typeof received.constructor === 'function' &&
          received.constructor !== expected
            ? _m3.printReceivedConstructorNameNot(
                'Received constructor',
                received.constructor,
                expected,
              )
            : '')
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          _m3.printExpectedConstructorName('Expected constructor', expected) +
          (_m1.isPrimitive(received) || Object.getPrototypeOf(received) === null
            ? `\nReceived value has no prototype\nReceived value: ${_m2.printReceived(
                received,
              )}`
            : typeof received.constructor === 'function'
              ? _m3.printReceivedConstructorName(
                  'Received constructor',
                  received.constructor,
                )
              : `\nReceived value: ${_m2.printReceived(received)}`);

    return {message, pass};
  },

  toBeLessThan(received                 , expected                 ) {
    const matcherName = 'toBeLessThan';
    const isNot = this.isNot;
    const options                     = {
      isNot,
      promise: this.promise,
    };
    _m2.ensureNumbers(received, expected, matcherName, options);

    const pass = received < expected;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, undefined, options) +
      '\n\n' +
      `Expected:${isNot ? ' not' : ''} < ${_m2.printExpected(expected)}\n` +
      `Received:${isNot ? '    ' : ''}   ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeLessThanOrEqual(received                 , expected                 ) {
    const matcherName = 'toBeLessThanOrEqual';
    const isNot = this.isNot;
    const options                     = {
      isNot,
      promise: this.promise,
    };
    _m2.ensureNumbers(received, expected, matcherName, options);

    const pass = received <= expected;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, undefined, options) +
      '\n\n' +
      `Expected:${isNot ? ' not' : ''} <= ${_m2.printExpected(expected)}\n` +
      `Received:${isNot ? '    ' : ''}    ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeNaN(received     , expected      ) {
    const matcherName = 'toBeNaN';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureNoExpected(expected, matcherName, options);

    const pass = Number.isNaN(received);

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, '', options) +
      '\n\n' +
      `Received: ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeNull(received         , expected      ) {
    const matcherName = 'toBeNull';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureNoExpected(expected, matcherName, options);

    const pass = received === null;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, '', options) +
      '\n\n' +
      `Received: ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeTruthy(received         , expected      ) {
    const matcherName = 'toBeTruthy';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureNoExpected(expected, matcherName, options);

    const pass = !!received;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, '', options) +
      '\n\n' +
      `Received: ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toBeUndefined(received         , expected      ) {
    const matcherName = 'toBeUndefined';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureNoExpected(expected, matcherName, options);

    const pass = received === void 0;

    const message = () =>
      // eslint-disable-next-line prefer-template
      _m2.matcherHint(matcherName, undefined, '', options) +
      '\n\n' +
      `Received: ${_m2.printReceived(received)}`;

    return {message, pass};
  },

  toContain(received                          , expected         ) {
    const matcherName = 'toContain';
    const isNot = this.isNot;
    const options                     = {
      comment: 'indexOf',
      isNot,
      promise: this.promise,
    };

    if (received == null) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.RECEIVED_COLOR('received')} value must not be null nor undefined`,
          _m2.printWithType('Received', received, _m2.printReceived),
        ),
      );
    }

    if (typeof received === 'string') {
      const wrongTypeErrorMessage = `${_m2.EXPECTED_COLOR(
        'expected',
      )} value must be a string if ${_m2.RECEIVED_COLOR(
        'received',
      )} value is a string`;

      if (typeof expected !== 'string') {
        throw new TypeError(
          _m2.matcherErrorMessage(
            _m2.matcherHint(matcherName, received, String(expected), options),
            wrongTypeErrorMessage,
            // eslint-disable-next-line prefer-template
            _m2.printWithType('Expected', expected, _m2.printExpected) +
              '\n' +
              _m2.printWithType('Received', received, _m2.printReceived),
          ),
        );
      }

      const index = received.indexOf(String(expected));
      const pass = index !== -1;

      const message = () => {
        const labelExpected = `Expected ${
          typeof expected === 'string' ? 'substring' : 'value'
        }`;
        const labelReceived = 'Received string';
        const printLabel = _m2.getLabelPrinter(labelExpected, labelReceived);

        return (
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          `${printLabel(labelExpected)}${isNot ? 'not ' : ''}${_m2.printExpected(
            expected,
          )}\n` +
          `${printLabel(labelReceived)}${isNot ? '    ' : ''}${
            isNot
              ? _m3.printReceivedStringContainExpectedSubstring(
                  received,
                  index,
                  String(expected).length,
                )
              : _m2.printReceived(received)
          }`
        );
      };

      return {message, pass};
    }

    const indexable = [...received];
    const index = indexable.indexOf(expected);
    const pass = index !== -1;

    const message = () => {
      const labelExpected = 'Expected value';
      const labelReceived = `Received ${_m1.getType(received)}`;
      const printLabel = _m2.getLabelPrinter(labelExpected, labelReceived);

      return (
        // eslint-disable-next-line prefer-template
        _m2.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        `${printLabel(labelExpected)}${isNot ? 'not ' : ''}${_m2.printExpected(
          expected,
        )}\n` +
        `${printLabel(labelReceived)}${isNot ? '    ' : ''}${
          isNot && Array.isArray(received)
            ? _m3.printReceivedArrayContainExpectedItem(received, index)
            : _m2.printReceived(received)
        }` +
        (!isNot &&
        indexable.some(item =>
          _m0.equals(item, expected, [...this.customTesters, _m0.iterableEquality]),
        )
          ? `\n\n${_m2.SUGGEST_TO_CONTAIN_EQUAL}`
          : '')
      );
    };

    return {message, pass};
  },

  toContainEqual(received                 , expected         ) {
    const matcherName = 'toContainEqual';
    const isNot = this.isNot;
    const options                     = {
      comment: 'deep equality',
      isNot,
      promise: this.promise,
    };

    if (received == null) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.RECEIVED_COLOR('received')} value must not be null nor undefined`,
          _m2.printWithType('Received', received, _m2.printReceived),
        ),
      );
    }

    const index = [...received].findIndex(item =>
      _m0.equals(item, expected, [...this.customTesters, _m0.iterableEquality]),
    );
    const pass = index !== -1;

    const message = () => {
      const labelExpected = 'Expected value';
      const labelReceived = `Received ${_m1.getType(received)}`;
      const printLabel = _m2.getLabelPrinter(labelExpected, labelReceived);

      return (
        // eslint-disable-next-line prefer-template
        _m2.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        `${printLabel(labelExpected)}${isNot ? 'not ' : ''}${_m2.printExpected(
          expected,
        )}\n` +
        `${printLabel(labelReceived)}${isNot ? '    ' : ''}${
          isNot && Array.isArray(received)
            ? _m3.printReceivedArrayContainExpectedItem(received, index)
            : _m2.printReceived(received)
        }`
      );
    };

    return {message, pass};
  },

  toEqual(received         , expected         ) {
    const matcherName = 'toEqual';
    const options                     = {
      comment: 'deep equality',
      isNot: this.isNot,
      promise: this.promise,
    };

    const pass = _m0.equals(received, expected, [
      ...this.customTesters,
      _m0.iterableEquality,
    ]);

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          `Expected: not ${_m2.printExpected(expected)}\n` +
          (_m2.stringify(expected) === _m2.stringify(received)
            ? ''
            : `Received:     ${_m2.printReceived(received)}`)
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          _m2.printDiffOrStringify(
            expected,
            received,
            EXPECTED_LABEL,
            RECEIVED_LABEL,
            isExpand(this.expand),
          );

    // Passing the actual and expected objects so that a custom reporter
    // could access them, for example in order to display a custom visual diff,
    // or create a different error message
    return {actual: received, expected, message, name: matcherName, pass};
  },

  toHaveLength(received     , expected        ) {
    const matcherName = 'toHaveLength';
    const isNot = this.isNot;
    const options                     = {
      isNot,
      promise: this.promise,
    };

    if (typeof received?.length !== 'number') {
      throw new TypeError(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.RECEIVED_COLOR(
            'received',
          )} value must have a length property whose value must be a number`,
          _m2.printWithType('Received', received, _m2.printReceived),
        ),
      );
    }

    _m2.ensureExpectedIsNonNegativeInteger(expected, matcherName, options);

    const pass = received.length === expected;

    const message = () => {
      const labelExpected = 'Expected length';
      const labelReceivedLength = 'Received length';
      const labelReceivedValue = `Received ${_m1.getType(received)}`;
      const printLabel = _m2.getLabelPrinter(
        labelExpected,
        labelReceivedLength,
        labelReceivedValue,
      );

      return (
        // eslint-disable-next-line prefer-template
        _m2.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        `${printLabel(labelExpected)}${isNot ? 'not ' : ''}${_m2.printExpected(
          expected,
        )}\n` +
        (isNot
          ? ''
          : `${printLabel(labelReceivedLength)}${_m2.printReceived(
              received.length,
            )}\n`) +
        `${printLabel(labelReceivedValue)}${isNot ? '    ' : ''}${_m2.printReceived(
          received,
        )}`
      );
    };

    return {message, pass};
  },

  toHaveProperty(
    received        ,
    expectedPath                        ,
    expectedValue          ,
  ) {
    const matcherName = 'toHaveProperty';
    const expectedArgument = 'path';
    const hasValue = arguments.length === 3;
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
      secondArgument: hasValue ? 'value' : '',
    };

    if (received === null || received === undefined) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, expectedArgument, options),
          `${_m2.RECEIVED_COLOR('received')} value must not be null nor undefined`,
          _m2.printWithType('Received', received, _m2.printReceived),
        ),
      );
    }

    const expectedPathType = _m1.getType(expectedPath);

    if (expectedPathType !== 'string' && expectedPathType !== 'array') {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, expectedArgument, options),
          `${_m2.EXPECTED_COLOR('expected')} path must be a string or array`,
          _m2.printWithType('Expected', expectedPath, _m2.printExpected),
        ),
      );
    }

    const expectedPathLength =
      typeof expectedPath === 'string'
        ? _m0.pathAsArray(expectedPath).length
        : expectedPath.length;

    if (expectedPathType === 'array' && expectedPathLength === 0) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, expectedArgument, options),
          `${_m2.EXPECTED_COLOR('expected')} path must not be an empty array`,
          _m2.printWithType('Expected', expectedPath, _m2.printExpected),
        ),
      );
    }

    const result = _m0.getPath(received, expectedPath);
    const {lastTraversedObject, endPropIsDefined, hasEndProp, value} = result;
    const receivedPath = result.traversedPath;
    const hasCompletePath = receivedPath.length === expectedPathLength;
    const receivedValue = hasCompletePath ? result.value : lastTraversedObject;

    const pass =
      hasValue && endPropIsDefined
        ? _m0.equals(value, expectedValue, [
            ...this.customTesters,
            _m0.iterableEquality,
          ])
        : Boolean(hasEndProp);

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, expectedArgument, options) +
          '\n\n' +
          (hasValue
            ? `Expected path: ${_m2.printExpected(expectedPath)}\n\n` +
              `Expected value: not ${_m2.printExpected(expectedValue)}${
                _m2.stringify(expectedValue) === _m2.stringify(receivedValue)
                  ? ''
                  : `\nReceived value:     ${_m2.printReceived(receivedValue)}`
              }`
            : `Expected path: not ${_m2.printExpected(expectedPath)}\n\n` +
              `Received value: ${_m2.printReceived(receivedValue)}`)
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, expectedArgument, options) +
          '\n\n' +
          `Expected path: ${_m2.printExpected(expectedPath)}\n` +
          (hasCompletePath
            ? `\n${_m2.printDiffOrStringify(
                expectedValue,
                receivedValue,
                EXPECTED_VALUE_LABEL,
                RECEIVED_VALUE_LABEL,
                isExpand(this.expand),
              )}`
            : `Received path: ${_m2.printReceived(
                expectedPathType === 'array' || receivedPath.length === 0
                  ? receivedPath
                  : receivedPath.join('.'),
              )}\n\n${
                hasValue
                  ? `Expected value: ${_m2.printExpected(expectedValue)}\n`
                  : ''
              }Received value: ${_m2.printReceived(receivedValue)}`);

    return {message, pass};
  },

  toMatch(received        , expected                 ) {
    const matcherName = 'toMatch';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };

    if (typeof received !== 'string') {
      throw new TypeError(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.RECEIVED_COLOR('received')} value must be a string`,
          _m2.printWithType('Received', received, _m2.printReceived),
        ),
      );
    }

    if (
      !(typeof expected === 'string') &&
      !(expected && typeof expected.test === 'function')
    ) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.EXPECTED_COLOR(
            'expected',
          )} value must be a string or regular expression`,
          _m2.printWithType('Expected', expected, _m2.printExpected),
        ),
      );
    }

    const pass =
      typeof expected === 'string'
        ? received.includes(expected)
        : new RegExp(expected).test(received);

    const message = pass
      ? () =>
          typeof expected === 'string'
            ? // eslint-disable-next-line prefer-template
              _m2.matcherHint(matcherName, undefined, undefined, options) +
              '\n\n' +
              `Expected substring: not ${_m2.printExpected(expected)}\n` +
              `Received string:        ${_m3.printReceivedStringContainExpectedSubstring(
                received,
                received.indexOf(expected),
                expected.length,
              )}`
            : // eslint-disable-next-line prefer-template
              _m2.matcherHint(matcherName, undefined, undefined, options) +
              '\n\n' +
              `Expected pattern: not ${_m2.printExpected(expected)}\n` +
              `Received string:      ${_m3.printReceivedStringContainExpectedResult(
                received,
                typeof expected.exec === 'function'
                  ? expected.exec(received)
                  : null,
              )}`
      : () => {
          const labelExpected = `Expected ${
            typeof expected === 'string' ? 'substring' : 'pattern'
          }`;
          const labelReceived = 'Received string';
          const printLabel = _m2.getLabelPrinter(labelExpected, labelReceived);

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(matcherName, undefined, undefined, options) +
            '\n\n' +
            `${printLabel(labelExpected)}${_m2.printExpected(expected)}\n` +
            `${printLabel(labelReceived)}${_m2.printReceived(received)}`
          );
        };

    return {message, pass};
  },

  toMatchObject(received        , expected        ) {
    const matcherName = 'toMatchObject';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };

    if (typeof received !== 'object' || received === null) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.RECEIVED_COLOR('received')} value must be a non-null object`,
          _m2.printWithType('Received', received, _m2.printReceived),
        ),
      );
    }

    if (typeof expected !== 'object' || expected === null) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, undefined, options),
          `${_m2.EXPECTED_COLOR('expected')} value must be a non-null object`,
          _m2.printWithType('Expected', expected, _m2.printExpected),
        ),
      );
    }

    const pass = _m0.equals(received, expected, [
      ...this.customTesters,
      _m0.iterableEquality,
      _m0.subsetEquality,
    ]);

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          `Expected: not ${_m2.printExpected(expected)}` +
          (_m2.stringify(expected) === _m2.stringify(received)
            ? ''
            : `\nReceived:     ${_m2.printReceived(received)}`)
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          _m2.printDiffOrStringify(
            expected,
            _m0.getObjectSubset(received, expected, this.customTesters),
            EXPECTED_LABEL,
            RECEIVED_LABEL,
            isExpand(this.expand),
          );

    return {message, pass};
  },

  toStrictEqual(received         , expected         ) {
    const matcherName = 'toStrictEqual';
    const options                     = {
      comment: 'deep equality',
      isNot: this.isNot,
      promise: this.promise,
    };

    const pass = _m0.equals(
      received,
      expected,
      [...this.customTesters, ...toStrictEqualTesters],
      true,
    );

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          `Expected: not ${_m2.printExpected(expected)}\n` +
          (_m2.stringify(expected) === _m2.stringify(received)
            ? ''
            : `Received:     ${_m2.printReceived(received)}`)
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(matcherName, undefined, undefined, options) +
          '\n\n' +
          _m2.printDiffOrStringify(
            expected,
            received,
            EXPECTED_LABEL,
            RECEIVED_LABEL,
            isExpand(this.expand),
          );

    // Passing the actual and expected objects so that a custom reporter
    // could access them, for example in order to display a custom visual diff,
    // or create a different error message
    return {actual: received, expected, message, name: matcherName, pass};
  },
};

const _default = matchers;
