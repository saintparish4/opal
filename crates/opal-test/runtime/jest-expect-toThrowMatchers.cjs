// Ported from jest/packages/expect/src/toThrowMatchers.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "createMatcher", {enumerable: true, get: () => createMatcher});
Object.defineProperty(exports, 'default', {enumerable: true, get: () => _default});
const _m0 = require("./jest-expect-utils.cjs");
const _m1 = require("./jest-matcher-utils.cjs");
const _m2 = require("./support.cjs");
const _m3 = require("./jest-expect-print.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 */





             
                    
                  
                 
                        
                 

const DID_NOT_THROW = 'Received function did not throw';

             
     
                       
                    
                      
                   
     
     
                          
                     
                      
                 
      

const getThrown = (e     )         => {
  const hasMessage =
    e !== null && e !== undefined && typeof e.message === 'string';

  if (hasMessage && typeof e.name === 'string' && typeof e.stack === 'string') {
    return {
      hasMessage,
      isError: true,
      message: e.message,
      value: e,
    };
  }

  return {
    hasMessage,
    isError: false,
    message: hasMessage ? e.message : String(e),
    value: e,
  };
};

const createMatcher = (
  matcherName        ,
  fromPromise          ,
)                         =>
  function (received, expected)                    {
    const options = {
      isNot: this.isNot,
      promise: this.promise,
    };

    let thrown = null;

    if (fromPromise && _m0.isError(received)) {
      thrown = getThrown(received);
    } else {
      if (typeof received === 'function') {
        try {
          received();
        } catch (error) {
          thrown = getThrown(error);
        }
      } else {
        if (!fromPromise) {
          const placeholder = expected === undefined ? '' : 'expected';
          throw new Error(
            _m1.matcherErrorMessage(
              _m1.matcherHint(matcherName, undefined, placeholder, options),
              `${_m1.RECEIVED_COLOR('received')} value must be a function`,
              _m1.printWithType('Received', received, _m1.printReceived),
            ),
          );
        }
      }
    }

    if (expected === undefined) {
      return toThrow(matcherName, options, thrown);
    } else if (typeof expected === 'function') {
      return toThrowExpectedClass(matcherName, options, thrown, expected);
    } else if (typeof expected === 'string') {
      return toThrowExpectedString(matcherName, options, thrown, expected);
    } else if (expected !== null && typeof expected.test === 'function') {
      return toThrowExpectedRegExp(matcherName, options, thrown, expected);
    } else if (
      expected !== null &&
      typeof expected.asymmetricMatch === 'function'
    ) {
      return toThrowExpectedAsymmetric(matcherName, options, thrown, expected);
    } else if (expected !== null && typeof expected === 'object') {
      return toThrowExpectedObject(matcherName, options, thrown, expected);
    } else {
      throw new Error(
        _m1.matcherErrorMessage(
          _m1.matcherHint(matcherName, undefined, undefined, options),
          `${_m1.EXPECTED_COLOR(
            'expected',
          )} value must be a string or regular expression or class or error`,
          _m1.printWithType('Expected', expected, _m1.printExpected),
        ),
      );
    }
  };

const matchers                 = {
  toThrow: createMatcher('toThrow'),
};

const toThrowExpectedRegExp = (
  matcherName        ,
  options                    ,
  thrown               ,
  expected        ,
)                        => {
  const pass = thrown !== null && expected.test(thrown.message);

  const message = pass
    ? () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        formatExpected('Expected pattern: not ', expected) +
        (thrown !== null && thrown.hasMessage
          ? formatReceived(
              'Received message:     ',
              thrown,
              'message',
              expected,
            ) + formatStack(thrown)
          : formatReceived('Received value:       ', thrown, 'value'))
    : () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        formatExpected('Expected pattern: ', expected) +
        (thrown === null
          ? `\n${DID_NOT_THROW}`
          : thrown.hasMessage
            ? formatReceived('Received message: ', thrown, 'message') +
              formatStack(thrown)
            : formatReceived('Received value:   ', thrown, 'value'));

  return {message, pass};
};

                          
                                                  
  

const toThrowExpectedAsymmetric = (
  matcherName        ,
  options                    ,
  thrown               ,
  expected                   ,
)                        => {
  const pass = thrown !== null && expected.asymmetricMatch(thrown.value);

  const message = pass
    ? () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        formatExpected('Expected asymmetric matcher: not ', expected) +
        '\n' +
        (thrown !== null && thrown.hasMessage
          ? formatReceived('Received name:    ', thrown, 'name') +
            formatReceived('Received message: ', thrown, 'message') +
            formatStack(thrown)
          : formatReceived('Thrown value: ', thrown, 'value'))
    : () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        formatExpected('Expected asymmetric matcher: ', expected) +
        '\n' +
        (thrown === null
          ? DID_NOT_THROW
          : thrown.hasMessage
            ? formatReceived('Received name:    ', thrown, 'name') +
              formatReceived('Received message: ', thrown, 'message') +
              formatStack(thrown)
            : formatReceived('Thrown value: ', thrown, 'value'));

  return {message, pass};
};

const toThrowExpectedObject = (
  matcherName        ,
  options                    ,
  thrown               ,
  expected       ,
)                        => {
  const expectedMessageAndCause = createMessageAndCause(expected);
  const thrownMessageAndCause =
    thrown === null ? null : createMessageAndCause(thrown.value);
  const isCompareErrorInstance = thrown?.isError && expected instanceof Error;
  const isExpectedCustomErrorInstance =
    expected.constructor.name !== Error.name;

  const pass =
    thrown !== null &&
    thrown.message === expected.message &&
    thrownMessageAndCause === expectedMessageAndCause &&
    (!isCompareErrorInstance ||
      !isExpectedCustomErrorInstance ||
      thrown.value instanceof expected.constructor);

  const message = pass
    ? () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        formatExpected(
          `Expected ${messageAndCause(expected)}: not `,
          expectedMessageAndCause,
        ) +
        (thrown !== null && thrown.hasMessage
          ? formatStack(thrown)
          : formatReceived('Received value:       ', thrown, 'value'))
    : () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        (thrown === null
          ? // eslint-disable-next-line prefer-template
            formatExpected(
              `Expected ${messageAndCause(expected)}: `,
              expectedMessageAndCause,
            ) +
            '\n' +
            DID_NOT_THROW
          : thrown.hasMessage
            ? // eslint-disable-next-line prefer-template
              _m1.printDiffOrStringify(
                expectedMessageAndCause,
                thrownMessageAndCause,
                `Expected ${messageAndCause(expected)}`,
                `Received ${messageAndCause(thrown.value)}`,
                true,
              ) +
              '\n' +
              formatStack(thrown)
            : formatExpected(
                `Expected ${messageAndCause(expected)}: `,
                expectedMessageAndCause,
              ) + formatReceived('Received value:   ', thrown, 'value'));

  return {message, pass};
};

const toThrowExpectedClass = (
  matcherName        ,
  options                    ,
  thrown               ,
  expected          ,
)                        => {
  const pass = thrown !== null && thrown.value instanceof expected;

  const message = pass
    ? () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        _m3.printExpectedConstructorNameNot('Expected constructor', expected) +
        (thrown !== null &&
        thrown.value != null &&
        typeof thrown.value.constructor === 'function' &&
        thrown.value.constructor !== expected
          ? _m3.printReceivedConstructorNameNot(
              'Received constructor',
              thrown.value.constructor,
              expected,
            )
          : '') +
        '\n' +
        (thrown !== null && thrown.hasMessage
          ? formatReceived('Received message: ', thrown, 'message') +
            formatStack(thrown)
          : formatReceived('Received value: ', thrown, 'value'))
    : () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        _m3.printExpectedConstructorName('Expected constructor', expected) +
        (thrown === null
          ? `\n${DID_NOT_THROW}`
          : `${
              thrown.value != null &&
              typeof thrown.value.constructor === 'function'
                ? _m3.printReceivedConstructorName(
                    'Received constructor',
                    thrown.value.constructor,
                  )
                : ''
            }\n${
              thrown.hasMessage
                ? formatReceived('Received message: ', thrown, 'message') +
                  formatStack(thrown)
                : formatReceived('Received value: ', thrown, 'value')
            }`);

  return {message, pass};
};

const toThrowExpectedString = (
  matcherName        ,
  options                    ,
  thrown               ,
  expected        ,
)                        => {
  const pass = thrown !== null && thrown.message.includes(expected);

  const message = pass
    ? () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        formatExpected('Expected substring: not ', expected) +
        (thrown !== null && thrown.hasMessage
          ? formatReceived(
              'Received message:       ',
              thrown,
              'message',
              expected,
            ) + formatStack(thrown)
          : formatReceived('Received value:         ', thrown, 'value'))
    : () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, undefined, options) +
        '\n\n' +
        formatExpected('Expected substring: ', expected) +
        (thrown === null
          ? `\n${DID_NOT_THROW}`
          : thrown.hasMessage
            ? formatReceived('Received message:   ', thrown, 'message') +
              formatStack(thrown)
            : formatReceived('Received value:     ', thrown, 'value'));

  return {message, pass};
};

const toThrow = (
  matcherName        ,
  options                    ,
  thrown               ,
)                        => {
  const pass = thrown !== null;

  const message = pass
    ? () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, '', options) +
        '\n\n' +
        (thrown !== null && thrown.hasMessage
          ? formatReceived('Error name:    ', thrown, 'name') +
            formatReceived('Error message: ', thrown, 'message') +
            formatStack(thrown)
          : formatReceived('Thrown value: ', thrown, 'value'))
    : () =>
        // eslint-disable-next-line prefer-template
        _m1.matcherHint(matcherName, undefined, '', options) +
        '\n\n' +
        DID_NOT_THROW;

  return {message, pass};
};

const formatExpected = (label        , expected         ) =>
  `${label + _m1.printExpected(expected)}\n`;

const formatReceived = (
  label        ,
  thrown               ,
  key        ,
  expected                  ,
) => {
  if (thrown === null) {
    return '';
  }

  if (key === 'message') {
    const message = thrown.message;

    if (typeof expected === 'string') {
      const index = message.indexOf(expected);
      if (index !== -1) {
        return `${
          label +
          _m3.printReceivedStringContainExpectedSubstring(
            message,
            index,
            expected.length,
          )
        }\n`;
      }
    } else if (expected instanceof RegExp) {
      return `${
        label +
        _m3.printReceivedStringContainExpectedResult(
          message,
          typeof expected.exec === 'function' ? expected.exec(message) : null,
        )
      }\n`;
    }

    return `${label + _m1.printReceived(message)}\n`;
  }

  if (key === 'name') {
    return thrown.isError
      ? `${label + _m1.printReceived(thrown.value.name)}\n`
      : '';
  }

  if (key === 'value') {
    return thrown.isError ? '' : `${label + _m1.printReceived(thrown.value)}\n`;
  }

  return '';
};

const formatStack = (thrown               ) => {
  if (thrown === null || !thrown.isError) {
    return '';
  } else {
    const config = {
      rootDir: process.cwd(),
      testMatch: [],
    };
    const options = {
      noStackTrace: false,
    };
    if (thrown.value instanceof AggregateError) {
      return _m2.formatExecError(thrown.value, config, options);
    } else {
      return _m2.formatStackTrace(
        _m2.separateMessageFromStack(thrown.value.stack ).stack,
        config,
        options,
      );
    }
  }
};

function createMessageAndCause(error       ) {
  if (error.cause) {
    const seen = new WeakSet();
    return JSON.stringify(buildSerializeError(error), (_, value) => {
      if (isObject(value)) {
        if (seen.has(value)) return;
        seen.add(value); // stop circular references
      }
      if (typeof value === 'bigint' || value === undefined) {
        return String(value);
      }
      return value;
    });
  }

  return error.message;
}

function buildSerializeError(error                      ) {
  if (!isObject(error)) {
    return error;
  }

  const result                       = {};
  for (const name of Object.getOwnPropertyNames(error).sort()) {
    if (['stack', 'fileName', 'lineNumber'].includes(name)) {
      continue;
    }
    if (name === 'cause') {
      result[name] = buildSerializeError(error['cause']);
      continue;
    }
    result[name] = error[name];
  }

  return result;
}

function isObject(obj         ) {
  return obj != null && typeof obj === 'object';
}

function messageAndCause(error       ) {
  return error.cause === undefined ? 'message' : 'message and cause';
}

const _default = matchers;
