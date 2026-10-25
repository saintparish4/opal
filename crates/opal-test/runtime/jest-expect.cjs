// Ported from jest/packages/expect/src/index.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "JestAssertionError", {enumerable: true, get: () => JestAssertionError});
Object.defineProperty(exports, "expect", {enumerable: true, get: () => expect});
Object.defineProperty(exports, 'default', {enumerable: true, get: () => _default});
const _m0 = require("./jest-expect-utils.cjs");
const _m1 = require("./jest-matcher-utils.cjs");
const _m2 = require("./support.cjs");
const _m3 = require("./jest-expect-asymmetricMatchers.cjs");
const _m4 = require("./jest-expect-extractExpectedAssertionsErrors.cjs");
const _m5 = require("./jest-expect-jestMatchersObject.cjs");
const _m6 = require("./jest-expect-matchers.cjs");
const _m7 = require("./jest-expect-spyMatchers.cjs");
const _m8 = require("./jest-expect-toThrowMatchers.cjs");
const _m9 = require("./jest-expect-asymmetricMatchers.cjs");
Object.defineProperty(exports, "AsymmetricMatcher", {enumerable: true, get: () => _m9.AsymmetricMatcher});
/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 */

/* eslint-disable local/prefer-spread-eventually */










             
         
                    
                 
               
               
                 
                   
               
                        
                    
                 

                                                              

             
                         
                     
             
         
                    
          
                 
                  
                             
               
               
           
                        
                 

class JestAssertionError extends Error {
  matcherResult                                                             ;
}

const createToThrowErrorMatchingSnapshotMatcher = function (
  matcher              ,
) {
  return function (
                         
    received     ,
    testNameOrInlineSnapshot         ,
  ) {
    return matcher.apply(this, [received, testNameOrInlineSnapshot, true]);
  };
};

const getPromiseMatcher = (name        , matcher              ) => {
  if (name === 'toThrow') {
    return _m8.createMatcher(name, true);
  } else if (
    name === 'toThrowErrorMatchingSnapshot' ||
    name === 'toThrowErrorMatchingInlineSnapshot'
  ) {
    return createToThrowErrorMatchingSnapshotMatcher(matcher);
  }

  return null;
};

const expect         = (actual     , ...rest            ) => {
  if (rest.length > 0) {
    throw new Error('Expect takes at most one argument.');
  }

  const allMatchers = _m5.getMatchers();
  const expectation      = {
    not: {},
    rejects: {not: {}},
    resolves: {not: {}},
  };

  const err = new JestAssertionError();

  for (const name of Object.keys(allMatchers)) {
    const matcher = allMatchers[name];
    const promiseMatcher = getPromiseMatcher(name, matcher) || matcher;
    expectation[name] = makeThrowingMatcher(matcher, false, '', actual);
    expectation.not[name] = makeThrowingMatcher(matcher, true, '', actual);

    expectation.resolves[name] = makeResolveMatcher(
      name,
      promiseMatcher,
      false,
      actual,
      err,
    );
    expectation.resolves.not[name] = makeResolveMatcher(
      name,
      promiseMatcher,
      true,
      actual,
      err,
    );

    expectation.rejects[name] = makeRejectMatcher(
      name,
      promiseMatcher,
      false,
      actual,
      err,
    );
    expectation.rejects.not[name] = makeRejectMatcher(
      name,
      promiseMatcher,
      true,
      actual,
      err,
    );
  }

  return expectation;
};

const getMessage = (message               ) =>
  (message && message()) ||
  _m1.RECEIVED_COLOR('No message was specified for this matcher.');

const makeResolveMatcher =
  (
    matcherName        ,
    matcher              ,
    isNot         ,
    actual                                     ,
    outerErr                    ,
  )                   =>
  (...args) => {
    const options = {
      isNot,
      promise: 'resolves',
    };

    const actualWrapper               =
      typeof actual === 'function' ? actual() : actual;

    if (!_m2.isPromise(actualWrapper)) {
      throw new JestAssertionError(
        _m1.matcherErrorMessage(
          _m1.matcherHint(matcherName, undefined, '', options),
          `${_m1.RECEIVED_COLOR(
            'received',
          )} value must be a promise or a function returning a promise`,
          _m1.printWithType(
            'Received',
            actual,
            _m1.printReceived,
          ),
        ),
      );
    }

    const innerErr = new JestAssertionError();

    return actualWrapper.then(
      result =>
        makeThrowingMatcher(matcher, isNot, 'resolves', result, innerErr).apply(
          null,
          args,
        ),
      error => {
        outerErr.message =
          `${_m1.matcherHint(
            matcherName,
            undefined,
            '',
            options,
          )}\n\n` +
          'Received promise rejected instead of resolved\n' +
          `Rejected to value: ${_m1.printReceived(error)}`;
        throw outerErr;
      },
    );
  };

const makeRejectMatcher =
  (
    matcherName        ,
    matcher              ,
    isNot         ,
    actual                                     ,
    outerErr                    ,
  )                   =>
  (...args) => {
    const options = {
      isNot,
      promise: 'rejects',
    };

    const actualWrapper               =
      typeof actual === 'function' ? actual() : actual;

    if (!_m2.isPromise(actualWrapper)) {
      throw new JestAssertionError(
        _m1.matcherErrorMessage(
          _m1.matcherHint(matcherName, undefined, '', options),
          `${_m1.RECEIVED_COLOR(
            'received',
          )} value must be a promise or a function returning a promise`,
          _m1.printWithType(
            'Received',
            actual,
            _m1.printReceived,
          ),
        ),
      );
    }

    const innerErr = new JestAssertionError();

    return actualWrapper.then(
      result => {
        outerErr.message =
          `${_m1.matcherHint(
            matcherName,
            undefined,
            '',
            options,
          )}\n\n` +
          'Received promise resolved instead of rejected\n' +
          `Resolved to value: ${_m1.printReceived(result)}`;
        throw outerErr;
      },
      error =>
        makeThrowingMatcher(matcher, isNot, 'rejects', error, innerErr).apply(
          null,
          args,
        ),
    );
  };

const makeThrowingMatcher = (
  matcher              ,
  isNot         ,
  promise        ,
  actual     ,
  err                     ,
)                    =>
  function throwingMatcher(...args)      {
    let throws = true;
    const utils                        = {
      ..._m1,
      iterableEquality: _m0.iterableEquality,
      subsetEquality: _m0.subsetEquality,
    };

    const matcherUtilsThing               = {
      customTesters: _m5.getCustomEqualityTesters(),
      // When throws is disabled, the matcher will not throw errors during test
      // execution but instead add them to the global matcher state. If a
      // matcher throws, test execution is normally stopped immediately. The
      // snapshot matcher uses it because we want to log all snapshot
      // failures in a test.
      dontThrow: () => (throws = false),
      equals: _m0.equals,
      utils,
    };

    const matcherContext                 = {
      ..._m5.getState              (),
      ...matcherUtilsThing,
      error: err,
      isNot,
      promise,
    };

    const processResult = (
      result                       ,
      asyncError                     ,
    ) => {
      _validateResult(result);

      _m5.getState().assertionCalls++;

      if ((result.pass && isNot) || (!result.pass && !isNot)) {
        // XOR
        const message = getMessage(result.message);
        let error;

        if (err) {
          error = err;
          error.message = message;
        } else if (asyncError) {
          error = asyncError;
          error.message = message;
        } else {
          error = new JestAssertionError(message);

          // Try to remove this function from the stack trace frame.
          // Guard for some environments (browsers) that do not support this feature.
          if (Error.captureStackTrace) {
            Error.captureStackTrace(error, throwingMatcher);
          }
        }
        // Passing the result of the matcher with the error so that a custom
        // reporter could access the actual and expected objects of the result
        // for example in order to display a custom visual diff
        error.matcherResult = {...result, message};

        if (throws) {
          throw error;
        } else {
          _m5.getState().suppressedErrors.push(error);
        }
      } else {
        _m5.getState().numPassingAsserts++;
      }
    };

    const handleError = (error       ) => {
      if (
        matcher[_m5.INTERNAL_MATCHER_FLAG] === true &&
        !(error instanceof JestAssertionError) &&
        error.name !== 'PrettyFormatPluginError' &&
        // Guard for some environments (browsers) that do not support this feature.
        Error.captureStackTrace
      ) {
        // Try to remove this and deeper functions from the stack trace frame.
        Error.captureStackTrace(error, throwingMatcher);
      }
      throw error;
    };

    let potentialResult                   ;

    try {
      potentialResult =
        matcher[_m5.INTERNAL_MATCHER_FLAG] === true
          ? matcher.call(matcherContext, actual, ...args)
          : // It's a trap specifically for inline snapshot to capture this name
            // in the stack trace, so that it can correctly get the custom matcher
            // function call.
            (function __EXTERNAL_MATCHER_TRAP__() {
              return matcher.call(matcherContext, actual, ...args);
            })();

      if (_m2.isPromise(potentialResult)) {
        const asyncError = new JestAssertionError();
        if (Error.captureStackTrace) {
          Error.captureStackTrace(asyncError, throwingMatcher);
        }

        return potentialResult
          .then(aResult => processResult(aResult, asyncError))
          .catch(handleError);
      } else {
        return processResult(potentialResult);
      }
    } catch (error     ) {
      return handleError(error);
    }
  };

expect.extend = (matchers                ) =>
  _m5.setMatchers(matchers, false, expect);

expect.addEqualityTesters = customTesters =>
  _m5.addCustomEqualityTesters(customTesters);

expect.anything = _m3.anything;
expect.any = _m3.any;

expect.not = {
  arrayContaining: _m3.arrayNotContaining,
  arrayOf: _m3.notArrayOf,
  closeTo: _m3.notCloseTo,
  objectContaining: _m3.objectNotContaining,
  stringContaining: _m3.stringNotContaining,
  stringMatching: _m3.stringNotMatching,
};

expect.arrayContaining = _m3.arrayContaining;
expect.arrayOf = _m3.arrayOf;
expect.closeTo = _m3.closeTo;
expect.objectContaining = _m3.objectContaining;
expect.stringContaining = _m3.stringContaining;
expect.stringMatching = _m3.stringMatching;

const _validateResult = (result     ) => {
  if (
    typeof result !== 'object' ||
    typeof result.pass !== 'boolean' ||
    (result.message &&
      typeof result.message !== 'string' &&
      typeof result.message !== 'function')
  ) {
    throw new Error(
      'Unexpected return from a matcher function.\n' +
        'Matcher functions should ' +
        'return an object in the following format:\n' +
        '  {message?: string | function, pass: boolean}\n' +
        `'${_m1.stringify(result)}' was returned`,
    );
  }
};

function assertions(expected        )       {
  const error = new _m2.ErrorWithStack(undefined, assertions);

  _m5.setState({
    expectedAssertionsNumber: expected,
    expectedAssertionsNumberError: error,
  });
}
function hasAssertions(...args                )       {
  const error = new _m2.ErrorWithStack(undefined, hasAssertions);

  _m1.ensureNoExpected(args[0], '.hasAssertions');
  _m5.setState({
    isExpectingAssertions: true,
    isExpectingAssertionsError: error,
  });
}

// add default jest matchers
_m5.setMatchers((_m6.default ?? _m6), true, expect);
_m5.setMatchers((_m7.default ?? _m7), true, expect);
_m5.setMatchers((_m8.default ?? _m8), true, expect);

expect.assertions = assertions;
expect.hasAssertions = hasAssertions;
expect.getState = _m5.getState;
expect.setState = _m5.setState;
expect.extractExpectedAssertionsErrors = (_m4.default ?? _m4);

const _default = expect;
