// The `expect` a test file sees: Jest's own, ported in jest-expect*.cjs,
// evaluated once per test file inside its context, so its state (assertion
// counts, custom matchers, the current test's name) is that file's alone.
// Snapshot matchers are added here once snapshots are.
'use strict';

const {expect} = require('./jest-expect.cjs');

function createExpect() {
  return expect;
}

module.exports = {createExpect};
