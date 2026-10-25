// Placeholder until jest-mock is ported: enough of a mock function to call.
'use strict';

class ModuleMocker {
  constructor() {
    this.mocks = new Set();
  }

  fn(implementation) {
    const calls = [];
    const mock = function (...args) {
      calls.push(args);
      return implementation ? implementation.apply(this, args) : undefined;
    };
    mock._isMockFunction = true;
    mock.mock = {calls};
    this.mocks.add(mock);
    return mock;
  }

  isMockFunction(fn) {
    return typeof fn === 'function' && fn._isMockFunction === true;
  }

  spyOn() {
    throw new Error('opal test: jest.spyOn() is not implemented yet.');
  }

  clearAllMocks() {}
  resetAllMocks() {}
  restoreAllMocks() {}
}

module.exports = {ModuleMocker};
