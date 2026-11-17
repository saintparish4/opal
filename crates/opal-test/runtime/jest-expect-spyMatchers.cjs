// Ported from jest/packages/expect/src/spyMatchers.ts at Jest v30.5.2 by
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
const _m3 = require("./jest-expect-jestMatchersObject.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/* eslint-disable unicorn/consistent-function-scoping */





             
                  
                 
                        
                 

// The optional property of matcher context is true if undefined.
const isExpand = (expand          )          => expand !== false;

const PRINT_LIMIT = 3;

const NO_ARGUMENTS = 'called with 0 arguments';

const printExpectedArgs = (expected                )         =>
  expected.length === 0
    ? NO_ARGUMENTS
    : expected.map(arg => _m2.printExpected(arg)).join(', ');

const printReceivedArgs = (
  received                ,
  expected                 ,
)         =>
  received.length === 0
    ? NO_ARGUMENTS
    : received
        .map((arg, i) =>
          Array.isArray(expected) &&
          i < expected.length &&
          isEqualValue(expected[i], arg)
            ? printCommon(arg)
            : _m2.printReceived(arg),
        )
        .join(', ');

const printCommon = (val         ) => _m2.DIM_COLOR(_m2.stringify(val));

const isEqualValue = (expected         , received         )          =>
  _m0.equals(expected, received, [..._m3.getCustomEqualityTesters(), _m0.iterableEquality]);

const isEqualCall = (
  expected                ,
  received                ,
)          =>
  received.length === expected.length && isEqualValue(expected, received);

const isEqualReturn = (expected         , result     )          =>
  result.type === 'return' && isEqualValue(expected, result.value);

const countReturns = (results            )         =>
  results.reduce(
    (n        , result     ) => (result.type === 'return' ? n + 1 : n),
    0,
  );

const printNumberOfReturns = (
  countReturns        ,
  countCalls        ,
)         =>
  `\nNumber of returns: ${_m2.printReceived(countReturns)}${
    countCalls === countReturns
      ? ''
      : `\nNumber of calls:   ${_m2.printReceived(countCalls)}`
  }`;

                                                                      

// Given a label, return a function which given a string,
// right-aligns it preceding the colon in the label.
const getRightAlignedPrinter = (label        )             => {
  // Assume that the label contains a colon.
  const index = label.indexOf(':');
  const suffix = label.slice(index);

  return (string        , isExpectedCall         ) =>
    (isExpectedCall
      ? `->${' '.repeat(Math.max(0, index - 2 - string.length))}`
      : ' '.repeat(Math.max(index - string.length))) +
    string +
    suffix;
};

                                            

const printReceivedCallsNegative = (
  expected                ,
  indexedCalls                    ,
  isOnlyCall         ,
  iExpectedCall         ,
) => {
  if (indexedCalls.length === 0) {
    return '';
  }

  const label = 'Received:     ';
  if (isOnlyCall) {
    return `${label + printReceivedArgs(indexedCalls[0], expected)}\n`;
  }

  const printAligned = getRightAlignedPrinter(label);

  return `Received\n${indexedCalls.reduce(
    (printed        , [i, args]             ) =>
      `${
        printed +
        printAligned(String(i + 1), i === iExpectedCall) +
        printReceivedArgs(args, expected)
      }\n`,
    '',
  )}`;
};

const printExpectedReceivedCallsPositive = (
  expected                ,
  indexedCalls                    ,
  expand         ,
  isOnlyCall         ,
  iExpectedCall         ,
) => {
  const expectedLine = `Expected: ${printExpectedArgs(expected)}\n`;
  if (indexedCalls.length === 0) {
    return expectedLine;
  }

  const label = 'Received: ';
  if (isOnlyCall && (iExpectedCall === 0 || iExpectedCall === undefined)) {
    const received = indexedCalls[0][1];

    if (isLineDiffableCall(expected, received)) {
      // Display diff without indentation.
      const lines = [
        _m2.EXPECTED_COLOR('- Expected'),
        _m2.RECEIVED_COLOR('+ Received'),
        '',
      ];

      const length = Math.max(expected.length, received.length);
      for (let i = 0; i < length; i += 1) {
        if (i < expected.length && i < received.length) {
          if (isEqualValue(expected[i], received[i])) {
            lines.push(`  ${printCommon(received[i])},`);
            continue;
          }

          if (isLineDiffableArg(expected[i], received[i])) {
            const difference = _m2.diff(expected[i], received[i], {expand});
            if (
              typeof difference === 'string' &&
              difference.includes('- Expected') &&
              difference.includes('+ Received')
            ) {
              // Omit annotation in case multiple args have diff.
              lines.push(`${difference.split('\n').slice(3).join('\n')},`);
              continue;
            }
          }
        }

        if (i < expected.length) {
          lines.push(`${_m2.EXPECTED_COLOR(`- ${_m2.stringify(expected[i])}`)},`);
        }
        if (i < received.length) {
          lines.push(`${_m2.RECEIVED_COLOR(`+ ${_m2.stringify(received[i])}`)},`);
        }
      }

      return `${lines.join('\n')}\n`;
    }

    return `${expectedLine + label + printReceivedArgs(received, expected)}\n`;
  }

  const printAligned = getRightAlignedPrinter(label);

  return (
    // eslint-disable-next-line prefer-template
    expectedLine +
    'Received\n' +
    indexedCalls.reduce((printed        , [i, received]             ) => {
      const aligned = printAligned(String(i + 1), i === iExpectedCall);
      return `${
        printed +
        ((i === iExpectedCall || iExpectedCall === undefined) &&
        isLineDiffableCall(expected, received)
          ? aligned.replace(': ', '\n') +
            printDiffCall(expected, received, expand)
          : aligned + printReceivedArgs(received, expected))
      }\n`;
    }, '')
  );
};

const indentation = 'Received'.replaceAll(/\w/g, ' ');

const printDiffCall = (
  expected                ,
  received                ,
  expand         ,
) =>
  received
    .map((arg, i) => {
      if (i < expected.length) {
        if (isEqualValue(expected[i], arg)) {
          return `${indentation}  ${printCommon(arg)},`;
        }

        if (isLineDiffableArg(expected[i], arg)) {
          const difference = _m2.diff(expected[i], arg, {expand});

          if (
            typeof difference === 'string' &&
            difference.includes('- Expected') &&
            difference.includes('+ Received')
          ) {
            // Display diff with indentation.
            // Omit annotation in case multiple args have diff.
            return `${difference
              .split('\n')
              .slice(3)
              .map(line => indentation + line)
              .join('\n')},`;
          }
        }
      }

      // Display + only if received arg has no corresponding expected arg.
      return `${
        indentation +
        (i < expected.length
          ? `  ${_m2.printReceived(arg)}`
          : _m2.RECEIVED_COLOR(`+ ${_m2.stringify(arg)}`))
      },`;
    })
    .join('\n');

const isLineDiffableCall = (
  expected                ,
  received                ,
)          =>
  expected.some(
    (arg, i) => i < received.length && isLineDiffableArg(arg, received[i]),
  );

// Almost redundant with function in jest-matcher-utils,
// except no line diff for any strings.
const isLineDiffableArg = (expected         , received         )          => {
  const expectedType = _m1.getType(expected);
  const receivedType = _m1.getType(received);

  if (expectedType !== receivedType) {
    return false;
  }

  if (_m1.isPrimitive(expected)) {
    return false;
  }

  if (
    expectedType === 'date' ||
    expectedType === 'function' ||
    expectedType === 'regexp'
  ) {
    return false;
  }

  if (expected instanceof Error && received instanceof Error) {
    return false;
  }

  if (
    expectedType === 'object' &&
    typeof (expected       ).asymmetricMatch === 'function'
  ) {
    return false;
  }

  if (
    receivedType === 'object' &&
    typeof (received       ).asymmetricMatch === 'function'
  ) {
    return false;
  }

  return true;
};

const printResult = (result     , expected         ) =>
  result.type === 'throw'
    ? 'function call threw an error'
    : result.type === 'incomplete'
      ? 'function call has not returned yet'
      : isEqualValue(expected, result.value)
        ? printCommon(result.value)
        : _m2.printReceived(result.value);

                                   

// Return either empty string or one line per indexed result,
// so additional empty line can separate from `Number of returns` which follows.
const printReceivedResults = (
  label        ,
  expected         ,
  indexedResults                      ,
  isOnlyCall         ,
  iExpectedCall         ,
) => {
  if (indexedResults.length === 0) {
    return '';
  }

  if (isOnlyCall && (iExpectedCall === 0 || iExpectedCall === undefined)) {
    return `${label + printResult(indexedResults[0][1], expected)}\n`;
  }

  const printAligned = getRightAlignedPrinter(label);

  return (
    // eslint-disable-next-line prefer-template
    label.replace(':', '').trim() +
    '\n' +
    indexedResults.reduce(
      (printed        , [i, result]               ) =>
        `${
          printed +
          printAligned(String(i + 1), i === iExpectedCall) +
          printResult(result, expected)
        }\n`,
      '',
    )
  );
};

const createToHaveBeenCalledMatcher = ()                             =>
  function (received     , expected         )                        {
    const expectedArgument = '';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureNoExpected(expected, 'toHaveBeenCalled', options);
    ensureMockOrSpy(received, 'toHaveBeenCalled', expectedArgument, options);

    const receivedIsSpy = isSpy(received);
    const receivedName = receivedIsSpy ? 'spy' : received.getMockName();
    const count = receivedIsSpy
      ? received.calls.count()
      : received.mock.calls.length;
    const calls = receivedIsSpy
      ? received.calls.all().map((x     ) => x.args)
      : received.mock.calls;
    const pass = count > 0;
    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(
            'toHaveBeenCalled',
            receivedName,
            expectedArgument,
            options,
          ) +
          '\n\n' +
          `Expected number of calls: ${_m2.printExpected(0)}\n` +
          `Received number of calls: ${_m2.printReceived(count)}\n\n` +
          calls
            .reduce((lines               , args     , i        ) => {
              if (lines.length < PRINT_LIMIT) {
                lines.push(`${i + 1}: ${printReceivedArgs(args)}`);
              }

              return lines;
            }, [])
            .join('\n')
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(
            'toHaveBeenCalled',
            receivedName,
            expectedArgument,
            options,
          ) +
          '\n\n' +
          `Expected number of calls: >= ${_m2.printExpected(1)}\n` +
          `Received number of calls:    ${_m2.printReceived(count)}`;

    return {message, pass};
  };

const createToHaveReturnedMatcher = ()                             =>
  function (received     , expected)                        {
    const expectedArgument = '';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureNoExpected(expected, 'toHaveReturned', options);
    ensureMock(received, 'toHaveReturned', expectedArgument, options);

    const receivedName = received.getMockName();

    // Count return values that correspond only to calls that returned
    const count = received.mock.results.reduce(
      (n        , result     ) => (result.type === 'return' ? n + 1 : n),
      0,
    );

    const pass = count > 0;

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(
            'toHaveReturned',
            receivedName,
            expectedArgument,
            options,
          ) +
          '\n\n' +
          `Expected number of returns: ${_m2.printExpected(0)}\n` +
          `Received number of returns: ${_m2.printReceived(count)}\n\n` +
          received.mock.results
            .reduce((lines               , result     , i        ) => {
              if (result.type === 'return' && lines.length < PRINT_LIMIT) {
                lines.push(`${i + 1}: ${_m2.printReceived(result.value)}`);
              }

              return lines;
            }, [])
            .join('\n') +
          (received.mock.calls.length === count
            ? ''
            : `\n\nReceived number of calls:   ${_m2.printReceived(
                received.mock.calls.length,
              )}`)
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(
            'toHaveReturned',
            receivedName,
            expectedArgument,
            options,
          ) +
          '\n\n' +
          `Expected number of returns: >= ${_m2.printExpected(1)}\n` +
          `Received number of returns:    ${_m2.printReceived(count)}` +
          (received.mock.calls.length === count
            ? ''
            : `\nReceived number of calls:      ${_m2.printReceived(
                received.mock.calls.length,
              )}`);

    return {message, pass};
  };

const createToHaveBeenCalledTimesMatcher = ()                            =>
  function (received     , expected)                        {
    const expectedArgument = 'expected';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureExpectedIsNonNegativeInteger(
      expected,
      'toHaveBeenCalledTimes',
      options,
    );
    ensureMockOrSpy(
      received,
      'toHaveBeenCalledTimes',
      expectedArgument,
      options,
    );

    const receivedIsSpy = isSpy(received);
    const receivedName = receivedIsSpy ? 'spy' : received.getMockName();
    const count = receivedIsSpy
      ? received.calls.count()
      : received.mock.calls.length;

    const pass = count === expected;

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(
            'toHaveBeenCalledTimes',
            receivedName,
            expectedArgument,
            options,
          ) +
          '\n\n' +
          `Expected number of calls: not ${_m2.printExpected(expected)}`
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(
            'toHaveBeenCalledTimes',
            receivedName,
            expectedArgument,
            options,
          ) +
          '\n\n' +
          `Expected number of calls: ${_m2.printExpected(expected)}\n` +
          `Received number of calls: ${_m2.printReceived(count)}`;

    return {message, pass};
  };

const createToHaveReturnedTimesMatcher = ()                            =>
  function (received     , expected)                        {
    const expectedArgument = 'expected';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    _m2.ensureExpectedIsNonNegativeInteger(
      expected,
      'toHaveReturnedTimes',
      options,
    );
    ensureMock(received, 'toHaveReturnedTimes', expectedArgument, options);

    const receivedName = received.getMockName();

    // Count return values that correspond only to calls that returned
    const count = received.mock.results.reduce(
      (n        , result     ) => (result.type === 'return' ? n + 1 : n),
      0,
    );

    const pass = count === expected;

    const message = pass
      ? () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(
            'toHaveReturnedTimes',
            receivedName,
            expectedArgument,
            options,
          ) +
          '\n\n' +
          `Expected number of returns: not ${_m2.printExpected(expected)}` +
          (received.mock.calls.length === count
            ? ''
            : `\n\nReceived number of calls:       ${_m2.printReceived(
                received.mock.calls.length,
              )}`)
      : () =>
          // eslint-disable-next-line prefer-template
          _m2.matcherHint(
            'toHaveReturnedTimes',
            receivedName,
            expectedArgument,
            options,
          ) +
          '\n\n' +
          `Expected number of returns: ${_m2.printExpected(expected)}\n` +
          `Received number of returns: ${_m2.printReceived(count)}` +
          (received.mock.calls.length === count
            ? ''
            : `\nReceived number of calls:   ${_m2.printReceived(
                received.mock.calls.length,
              )}`);

    return {message, pass};
  };

const createToHaveBeenCalledWithMatcher = ()                                  =>
  function (received     , ...expected)                        {
    const expectedArgument = '...expected';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    ensureMockOrSpy(
      received,
      'toHaveBeenCalledWith',
      expectedArgument,
      options,
    );

    const receivedIsSpy = isSpy(received);
    const receivedName = receivedIsSpy ? 'spy' : received.getMockName();

    const calls = receivedIsSpy
      ? received.calls.all().map((x     ) => x.args)
      : received.mock.calls;

    const pass = calls.some((call     ) => isEqualCall(expected, call));

    const message = pass
      ? () => {
          // Some examples of calls that are equal to expected value.
          const indexedCalls                     = [];
          let i = 0;
          while (i < calls.length && indexedCalls.length < PRINT_LIMIT) {
            if (isEqualCall(expected, calls[i])) {
              indexedCalls.push([i, calls[i]]);
            }
            i += 1;
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveBeenCalledWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `Expected: not ${printExpectedArgs(expected)}\n` +
            (calls.length === 1 && _m2.stringify(calls[0]) === _m2.stringify(expected)
              ? ''
              : printReceivedCallsNegative(
                  expected,
                  indexedCalls,
                  calls.length === 1,
                )) +
            `\nNumber of calls: ${_m2.printReceived(calls.length)}`
          );
        }
      : () => {
          // Some examples of calls that are not equal to expected value.
          const indexedCalls                     = [];
          let i = 0;
          while (i < calls.length && indexedCalls.length < PRINT_LIMIT) {
            indexedCalls.push([i, calls[i]]);
            i += 1;
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveBeenCalledWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            printExpectedReceivedCallsPositive(
              expected,
              indexedCalls,
              isExpand(this.expand),
              calls.length === 1,
            ) +
            `\nNumber of calls: ${_m2.printReceived(calls.length)}`
          );
        };

    return {message, pass};
  };

const createToHaveReturnedWithMatcher = ()                             =>
  function (received     , expected)                        {
    const expectedArgument = 'expected';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    ensureMock(received, 'toHaveReturnedWith', expectedArgument, options);

    const receivedName = received.getMockName();
    const {calls, results} = received.mock;

    const pass = results.some((result     ) => isEqualReturn(expected, result));

    const message = pass
      ? () => {
          // Some examples of results that are equal to expected value.
          const indexedResults                       = [];
          let i = 0;
          while (i < results.length && indexedResults.length < PRINT_LIMIT) {
            if (isEqualReturn(expected, results[i])) {
              indexedResults.push([i, results[i]]);
            }
            i += 1;
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveReturnedWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `Expected: not ${_m2.printExpected(expected)}\n` +
            (results.length === 1 &&
            results[0].type === 'return' &&
            _m2.stringify(results[0].value) === _m2.stringify(expected)
              ? ''
              : printReceivedResults(
                  'Received:     ',
                  expected,
                  indexedResults,
                  results.length === 1,
                )) +
            printNumberOfReturns(countReturns(results), calls.length)
          );
        }
      : () => {
          // Some examples of results that are not equal to expected value.
          const indexedResults                       = [];
          let i = 0;
          while (i < results.length && indexedResults.length < PRINT_LIMIT) {
            indexedResults.push([i, results[i]]);
            i += 1;
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveReturnedWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `Expected: ${_m2.printExpected(expected)}\n` +
            printReceivedResults(
              'Received: ',
              expected,
              indexedResults,
              results.length === 1,
            ) +
            printNumberOfReturns(countReturns(results), calls.length)
          );
        };

    return {message, pass};
  };

const createToHaveBeenLastCalledWithMatcher = (                   
                
) =>
  function (received     , ...expected)                        {
    const expectedArgument = '...expected';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    ensureMockOrSpy(
      received,
      'toHaveBeenLastCalledWith',
      expectedArgument,
      options,
    );

    const receivedIsSpy = isSpy(received);
    const receivedName = receivedIsSpy ? 'spy' : received.getMockName();

    const calls = receivedIsSpy
      ? received.calls.all().map((x     ) => x.args)
      : received.mock.calls;
    const iLast = calls.length - 1;

    const pass = iLast >= 0 && isEqualCall(expected, calls[iLast]);

    const message = pass
      ? () => {
          const indexedCalls                     = [];
          if (iLast > 0) {
            // Display preceding call as context.
            indexedCalls.push([iLast - 1, calls[iLast - 1]]);
          }
          indexedCalls.push([iLast, calls[iLast]]);

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveBeenLastCalledWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `Expected: not ${printExpectedArgs(expected)}\n` +
            (calls.length === 1 && _m2.stringify(calls[0]) === _m2.stringify(expected)
              ? ''
              : printReceivedCallsNegative(
                  expected,
                  indexedCalls,
                  calls.length === 1,
                  iLast,
                )) +
            `\nNumber of calls: ${_m2.printReceived(calls.length)}`
          );
        }
      : () => {
          const indexedCalls                     = [];
          if (iLast >= 0) {
            if (iLast > 0) {
              let i = iLast - 1;
              // Is there a preceding call that is equal to expected args?
              while (i >= 0 && !isEqualCall(expected, calls[i])) {
                i -= 1;
              }
              if (i < 0) {
                i = iLast - 1; // otherwise, preceding call
              }

              indexedCalls.push([i, calls[i]]);
            }

            indexedCalls.push([iLast, calls[iLast]]);
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveBeenLastCalledWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            printExpectedReceivedCallsPositive(
              expected,
              indexedCalls,
              isExpand(this.expand),
              calls.length === 1,
              iLast,
            ) +
            `\nNumber of calls: ${_m2.printReceived(calls.length)}`
          );
        };

    return {message, pass};
  };

const createToHaveLastReturnedWithMatcher = ()                             =>
  function (received     , expected)                        {
    const expectedArgument = 'expected';
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    ensureMock(received, 'toHaveLastReturnedWith', expectedArgument, options);

    const receivedName = received.getMockName();

    const {calls, results} = received.mock;
    const iLast = results.length - 1;

    const pass = iLast >= 0 && isEqualReturn(expected, results[iLast]);

    const message = pass
      ? () => {
          const indexedResults                       = [];
          if (iLast > 0) {
            // Display preceding result as context.
            indexedResults.push([iLast - 1, results[iLast - 1]]);
          }
          indexedResults.push([iLast, results[iLast]]);

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveLastReturnedWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `Expected: not ${_m2.printExpected(expected)}\n` +
            (results.length === 1 &&
            results[0].type === 'return' &&
            _m2.stringify(results[0].value) === _m2.stringify(expected)
              ? ''
              : printReceivedResults(
                  'Received:     ',
                  expected,
                  indexedResults,
                  results.length === 1,
                  iLast,
                )) +
            printNumberOfReturns(countReturns(results), calls.length)
          );
        }
      : () => {
          const indexedResults                       = [];
          if (iLast >= 0) {
            if (iLast > 0) {
              let i = iLast - 1;
              // Is there a preceding result that is equal to expected value?
              while (i >= 0 && !isEqualReturn(expected, results[i])) {
                i -= 1;
              }
              if (i < 0) {
                i = iLast - 1; // otherwise, preceding result
              }

              indexedResults.push([i, results[i]]);
            }

            indexedResults.push([iLast, results[iLast]]);
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveLastReturnedWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `Expected: ${_m2.printExpected(expected)}\n` +
            printReceivedResults(
              'Received: ',
              expected,
              indexedResults,
              results.length === 1,
              iLast,
            ) +
            printNumberOfReturns(countReturns(results), calls.length)
          );
        };

    return {message, pass};
  };

const createToHaveBeenNthCalledWithMatcher = (                   
                             
) =>
  function (received     , nth, ...expected)                        {
    const expectedArgument = 'n';
    const options                     = {
      expectedColor: (arg        ) => arg,
      isNot: this.isNot,
      promise: this.promise,
      secondArgument: '...expected',
    };
    ensureMockOrSpy(
      received,
      'toHaveBeenNthCalledWith',
      expectedArgument,
      options,
    );

    if (!Number.isSafeInteger(nth) || nth < 1) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(
            'toHaveBeenNthCalledWith',
            undefined,
            expectedArgument,
            options,
          ),
          `${expectedArgument} must be a positive integer`,
          _m2.printWithType(expectedArgument, nth, _m2.stringify),
        ),
      );
    }

    const receivedIsSpy = isSpy(received);
    const receivedName = receivedIsSpy ? 'spy' : received.getMockName();

    const calls = receivedIsSpy
      ? received.calls.all().map((x     ) => x.args)
      : received.mock.calls;
    const length = calls.length;
    const iNth = nth - 1;

    const pass = iNth < length && isEqualCall(expected, calls[iNth]);

    const message = pass
      ? () => {
          // Display preceding and following calls,
          // in case assertions fails because index is off by one.
          const indexedCalls                     = [];
          if (iNth - 1 >= 0) {
            indexedCalls.push([iNth - 1, calls[iNth - 1]]);
          }
          indexedCalls.push([iNth, calls[iNth]]);
          if (iNth + 1 < length) {
            indexedCalls.push([iNth + 1, calls[iNth + 1]]);
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveBeenNthCalledWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `n: ${nth}\n` +
            `Expected: not ${printExpectedArgs(expected)}\n` +
            (calls.length === 1 && _m2.stringify(calls[0]) === _m2.stringify(expected)
              ? ''
              : printReceivedCallsNegative(
                  expected,
                  indexedCalls,
                  calls.length === 1,
                  iNth,
                )) +
            `\nNumber of calls: ${_m2.printReceived(calls.length)}`
          );
        }
      : () => {
          // Display preceding and following calls:
          // * nearest call that is equal to expected args
          // * otherwise, adjacent call
          // in case assertions fails because of index, especially off by one.
          const indexedCalls                     = [];
          if (iNth < length) {
            if (iNth - 1 >= 0) {
              let i = iNth - 1;
              // Is there a preceding call that is equal to expected args?
              while (i >= 0 && !isEqualCall(expected, calls[i])) {
                i -= 1;
              }
              if (i < 0) {
                i = iNth - 1; // otherwise, adjacent call
              }

              indexedCalls.push([i, calls[i]]);
            }
            indexedCalls.push([iNth, calls[iNth]]);
            if (iNth + 1 < length) {
              let i = iNth + 1;
              // Is there a following call that is equal to expected args?
              while (i < length && !isEqualCall(expected, calls[i])) {
                i += 1;
              }
              if (i >= length) {
                i = iNth + 1; // otherwise, adjacent call
              }

              indexedCalls.push([i, calls[i]]);
            }
          } else if (length > 0) {
            // The number of received calls is fewer than the expected number.
            let i = length - 1;
            // Is there a call that is equal to expected args?
            while (i >= 0 && !isEqualCall(expected, calls[i])) {
              i -= 1;
            }
            if (i < 0) {
              i = length - 1; // otherwise, last call
            }

            indexedCalls.push([i, calls[i]]);
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveBeenNthCalledWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `n: ${nth}\n` +
            printExpectedReceivedCallsPositive(
              expected,
              indexedCalls,
              isExpand(this.expand),
              calls.length === 1,
              iNth,
            ) +
            `\nNumber of calls: ${_m2.printReceived(calls.length)}`
          );
        };

    return {message, pass};
  };

const createToHaveNthReturnedWithMatcher = (                   
                   
) =>
  function (received     , nth, expected)                        {
    const expectedArgument = 'n';
    const options                     = {
      expectedColor: (arg        ) => arg,
      isNot: this.isNot,
      promise: this.promise,
      secondArgument: 'expected',
    };
    ensureMock(received, 'toHaveNthReturnedWith', expectedArgument, options);

    if (!Number.isSafeInteger(nth) || nth < 1) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(
            'toHaveNthReturnedWith',
            undefined,
            expectedArgument,
            options,
          ),
          `${expectedArgument} must be a positive integer`,
          _m2.printWithType(expectedArgument, nth, _m2.stringify),
        ),
      );
    }

    const receivedName = received.getMockName();
    const {calls, results} = received.mock;
    const length = results.length;
    const iNth = nth - 1;

    const pass = iNth < length && isEqualReturn(expected, results[iNth]);

    const message = pass
      ? () => {
          // Display preceding and following results,
          // in case assertions fails because index is off by one.
          const indexedResults                       = [];
          if (iNth - 1 >= 0) {
            indexedResults.push([iNth - 1, results[iNth - 1]]);
          }
          indexedResults.push([iNth, results[iNth]]);
          if (iNth + 1 < length) {
            indexedResults.push([iNth + 1, results[iNth + 1]]);
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveNthReturnedWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `n: ${nth}\n` +
            `Expected: not ${_m2.printExpected(expected)}\n` +
            (results.length === 1 &&
            results[0].type === 'return' &&
            _m2.stringify(results[0].value) === _m2.stringify(expected)
              ? ''
              : printReceivedResults(
                  'Received:     ',
                  expected,
                  indexedResults,
                  results.length === 1,
                  iNth,
                )) +
            printNumberOfReturns(countReturns(results), calls.length)
          );
        }
      : () => {
          // Display preceding and following results:
          // * nearest result that is equal to expected value
          // * otherwise, adjacent result
          // in case assertions fails because of index, especially off by one.
          const indexedResults                       = [];
          if (iNth < length) {
            if (iNth - 1 >= 0) {
              let i = iNth - 1;
              // Is there a preceding result that is equal to expected value?
              while (i >= 0 && !isEqualReturn(expected, results[i])) {
                i -= 1;
              }
              if (i < 0) {
                i = iNth - 1; // otherwise, adjacent result
              }

              indexedResults.push([i, results[i]]);
            }
            indexedResults.push([iNth, results[iNth]]);
            if (iNth + 1 < length) {
              let i = iNth + 1;
              // Is there a following result that is equal to expected value?
              while (i < length && !isEqualReturn(expected, results[i])) {
                i += 1;
              }
              if (i >= length) {
                i = iNth + 1; // otherwise, adjacent result
              }

              indexedResults.push([i, results[i]]);
            }
          } else if (length > 0) {
            // The number of received calls is fewer than the expected number.
            let i = length - 1;
            // Is there a result that is equal to expected value?
            while (i >= 0 && !isEqualReturn(expected, results[i])) {
              i -= 1;
            }
            if (i < 0) {
              i = length - 1; // otherwise, last result
            }

            indexedResults.push([i, results[i]]);
          }

          return (
            // eslint-disable-next-line prefer-template
            _m2.matcherHint(
              'toHaveNthReturnedWith',
              receivedName,
              expectedArgument,
              options,
            ) +
            '\n\n' +
            `n: ${nth}\n` +
            `Expected: ${_m2.printExpected(expected)}\n` +
            printReceivedResults(
              'Received: ',
              expected,
              indexedResults,
              results.length === 1,
              iNth,
            ) +
            printNumberOfReturns(countReturns(results), calls.length)
          );
        };

    return {message, pass};
  };

const spyMatchers                 = {
  toHaveBeenCalled: createToHaveBeenCalledMatcher(),
  toHaveBeenCalledTimes: createToHaveBeenCalledTimesMatcher(),
  toHaveBeenCalledWith: createToHaveBeenCalledWithMatcher(),
  toHaveBeenLastCalledWith: createToHaveBeenLastCalledWithMatcher(),
  toHaveBeenNthCalledWith: createToHaveBeenNthCalledWithMatcher(),
  toHaveLastReturnedWith: createToHaveLastReturnedWithMatcher(),
  toHaveNthReturnedWith: createToHaveNthReturnedWithMatcher(),
  toHaveReturned: createToHaveReturnedMatcher(),
  toHaveReturnedTimes: createToHaveReturnedTimesMatcher(),
  toHaveReturnedWith: createToHaveReturnedWithMatcher(),
};

const isMock = (received     ) =>
  received != null && received._isMockFunction === true;

const isSpy = (received     ) =>
  received != null &&
  received.calls != null &&
  typeof received.calls.all === 'function' &&
  typeof received.calls.count === 'function';

const ensureMockOrSpy = (
  received     ,
  matcherName        ,
  expectedArgument        ,
  options                    ,
) => {
  if (!isMock(received) && !isSpy(received)) {
    throw new Error(
      _m2.matcherErrorMessage(
        _m2.matcherHint(matcherName, undefined, expectedArgument, options),
        `${_m2.RECEIVED_COLOR('received')} value must be a mock or spy function`,
        _m2.printWithType('Received', received, _m2.printReceived),
      ),
    );
  }
};

const ensureMock = (
  received     ,
  matcherName        ,
  expectedArgument        ,
  options                    ,
) => {
  if (!isMock(received)) {
    throw new Error(
      _m2.matcherErrorMessage(
        _m2.matcherHint(matcherName, undefined, expectedArgument, options),
        `${_m2.RECEIVED_COLOR('received')} value must be a mock function`,
        _m2.printWithType('Received', received, _m2.printReceived),
      ),
    );
  }
};

const _default = spyMatchers;
