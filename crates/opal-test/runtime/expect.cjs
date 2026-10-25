// The `expect` a test file sees: Jest's own (jest-expect*.cjs) with the
// snapshot matchers added, as @jest/expect adds them (jest-expect-snapshot.cjs).
// Evaluated once per test file inside its context, so its state (assertion
// counts, custom matchers, serializers, the current test) is that file's alone.
'use strict';

const {jestExpect} = require('./jest-expect-snapshot.cjs');

function createExpect() {
  return jestExpect;
}

module.exports = {createExpect};
