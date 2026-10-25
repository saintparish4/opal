// Placeholder until expect is ported: the state circus reads, and matchers
// that say they aren't here rather than pass.
'use strict';

function createExpect() {
  let state = {
    assertionCalls: 0,
    currentTestName: undefined,
    expectedAssertionsNumber: null,
    isExpectingAssertions: false,
    numPassingAsserts: 0,
    suppressedErrors: [],
    testFailing: false,
  };
  const expect = () =>
    new Proxy({}, {
      get(_target, name) {
        return () => {
          throw new Error(`opal test: expect().${String(name)} is not implemented yet.`);
        };
      },
    });
  expect.getState = () => state;
  expect.setState = next => {
    state = {...state, ...next};
  };
  expect.extractExpectedAssertionsErrors = () => [];
  return expect;
}

module.exports = {createExpect};
