// Ported from jest/packages/expect/src/jestMatchersObject.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "INTERNAL_MATCHER_FLAG", {enumerable: true, get: () => INTERNAL_MATCHER_FLAG});
Object.defineProperty(exports, "getState", {enumerable: true, get: () => getState});
Object.defineProperty(exports, "setState", {enumerable: true, get: () => setState});
Object.defineProperty(exports, "getMatchers", {enumerable: true, get: () => getMatchers});
Object.defineProperty(exports, "setMatchers", {enumerable: true, get: () => setMatchers});
Object.defineProperty(exports, "getCustomEqualityTesters", {enumerable: true, get: () => getCustomEqualityTesters});
Object.defineProperty(exports, "addCustomEqualityTesters", {enumerable: true, get: () => addCustomEqualityTesters});
const _m0 = require("./jest-get-type.cjs");
const _m1 = require("./jest-expect-asymmetricMatchers.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 */

                                               


             
         
               
                 
                        
                 

// Global matchers object holds the list of available matchers and
// the state, that can hold matcher specific values that change over time.
const JEST_MATCHERS_OBJECT = Symbol.for('$$jest-matchers-object');

// Notes a built-in/internal Jest matcher.
// Jest may override the stack trace of Errors thrown by internal matchers.
const INTERNAL_MATCHER_FLAG = Symbol.for('$$jest-internal-matcher');

if (!Object.prototype.hasOwnProperty.call(globalThis, JEST_MATCHERS_OBJECT)) {
  const defaultState               = {
    assertionCalls: 0,
    expectedAssertionsNumber: null,
    isExpectingAssertions: false,
    numPassingAsserts: 0,
    suppressedErrors: [], // errors that are not thrown immediately.
  };
  Object.defineProperty(globalThis, JEST_MATCHERS_OBJECT, {
    value: {
      customEqualityTesters: [],
      matchers: Object.create(null),
      state: defaultState,
    },
  });
}

const getState =                                            ()        =>
  (globalThis       )[JEST_MATCHERS_OBJECT].state;

const setState =                                            (
  state                ,
)       => {
  Object.assign((globalThis       )[JEST_MATCHERS_OBJECT].state, state);
};

const getMatchers = ()                 =>
  (globalThis       )[JEST_MATCHERS_OBJECT].matchers;

const setMatchers = (
  matchers                ,
  isInternal         ,
  expect        ,
)       => {
  for (const key of Object.keys(matchers)) {
    const matcher = matchers[key];

    if (typeof matcher !== 'function') {
      throw new TypeError(
        `expect.extend: \`${key}\` is not a valid matcher. Must be a function, is "${_m0.getType(
          matcher,
        )}"`,
      );
    }

    Object.defineProperty(matcher, INTERNAL_MATCHER_FLAG, {
      value: isInternal,
    });

    if (!isInternal) {
      // expect is defined

      class CustomMatcher extends _m1.AsymmetricMatcher 
                                    
        {
        constructor(inverse = false, ...sample                              ) {
          super(sample, inverse);
        }

        asymmetricMatch(other         ) {
          const {pass} = matcher.call(
            this.getMatcherContext(),
            other,
            ...this.sample,
          )                         ;

          return this.inverse ? !pass : pass;
        }

        toString() {
          return `${this.inverse ? 'not.' : ''}${key}`;
        }

                 getExpectedType() {
          return 'any';
        }

                 toAsymmetricMatcher() {
          return `${this.toString()}<${this.sample.map(String).join(', ')}>`;
        }
      }

      Object.defineProperty(expect, key, {
        configurable: true,
        enumerable: true,
        value: (...sample                              ) =>
          new CustomMatcher(false, ...sample),
        writable: true,
      });
      Object.defineProperty(expect.not, key, {
        configurable: true,
        enumerable: true,
        value: (...sample                              ) =>
          new CustomMatcher(true, ...sample),
        writable: true,
      });
    }
  }

  Object.assign((globalThis       )[JEST_MATCHERS_OBJECT].matchers, matchers);
};

const getCustomEqualityTesters = ()                =>
  (globalThis       )[JEST_MATCHERS_OBJECT].customEqualityTesters;

const addCustomEqualityTesters = (newTesters               )       => {
  if (!Array.isArray(newTesters)) {
    throw new TypeError(
      `expect.customEqualityTesters: Must be set to an array of Testers. Was given "${_m0.getType(
        newTesters,
      )}"`,
    );
  }

  (globalThis       )[JEST_MATCHERS_OBJECT].customEqualityTesters.push(
    ...newTesters,
  );
};
