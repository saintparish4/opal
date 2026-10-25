// Mock functions are Jest's own: jest-mock, ported in jest-mock.cjs.
'use strict';

const {types} = require('util');
const {ModuleMocker} = require('./jest-mock.cjs');

// The Vitest dialect's mocks are jest-mock's with two of @vitest/spy's
// additions, neither of which Jest's mocks have:
//   - mock.settledResults: for each call, what the promise it returned
//     settled to, or what it returned when that wasn't a promise. jest-mock
//     pushes a call's result before the call and fills it in after, so the
//     result's value is watched to learn what the call returned.
//   - a spy's name: the spied function's own name, which a snapshot of the
//     spy prints, and which mockReset keeps.
class VitestModuleMocker extends ModuleMocker {
  constructor(global) {
    super(global);
    this._spyNames = new WeakMap();
  }

  spyOn(object, methodKey, accessType) {
    let original;
    try {
      if (accessType) {
        for (let owner = object; owner && !original; owner = Object.getPrototypeOf(owner)) {
          original = Object.getOwnPropertyDescriptor(owner, methodKey)?.[accessType];
        }
      } else {
        original = object[methodKey];
      }
    } catch {}
    const spy = super.spyOn(object, methodKey, accessType);
    if (spy !== original && !this._spyNames.has(spy)) {
      const name = (typeof original === 'function' && original.name) || 'Mock';
      this._spyNames.set(spy, name);
      this._ensureMockConfig(spy).mockName = name;
    }
    return spy;
  }

  _ensureMockConfig(f) {
    const fresh = !this._mockConfigRegistry.has(f);
    const config = super._ensureMockConfig(f);
    if (fresh && this._spyNames.has(f)) config.mockName = this._spyNames.get(f);
    return config;
  }

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
