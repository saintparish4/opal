// Mock functions are Jest's own: jest-mock, ported in jest-mock.cjs.
'use strict';

const {types} = require('util');
const {ModuleMocker} = require('./jest-mock.cjs');

// Vitest's mocks also record mock.settledResults: for each call, what the
// promise it returned settled to, or what it returned when that wasn't a
// promise, as @vitest/spy's createMock records them. Jest's mocks have no
// such field, so only the Vitest dialect's mocker records it. jest-mock
// pushes a call's result before the call and fills it in after, so the
// result's value is watched to learn what the call returned.
class VitestModuleMocker extends ModuleMocker {
  _defaultMockState() {
    const state = super._defaultMockState();
    const settledResults = [];
    Object.defineProperty(state.results, 'push', {
      configurable: true,
      value(result) {
        const settled = {type: 'incomplete', value: undefined};
        settledResults.push(settled);
        let value = result.value;
        Object.defineProperty(result, 'value', {
          configurable: true,
          enumerable: true,
          get: () => value,
          set(next) {
            value = next;
            if (result.type === 'throw') {
              settled.type = 'rejected';
              settled.value = next;
            } else if (types.isPromise(next)) {
              next.then(
                resolved => {
                  settled.type = 'fulfilled';
                  settled.value = resolved;
                },
                rejected => {
                  settled.type = 'rejected';
                  settled.value = rejected;
                },
              );
            } else {
              settled.type = 'fulfilled';
              settled.value = next;
            }
          },
        });
        return Array.prototype.push.call(this, result);
      },
    });
    state.settledResults = settledResults;
    return state;
  }
}

module.exports = {ModuleMocker, VitestModuleMocker};
