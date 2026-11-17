// The matchers Jest 30 removed, which Jest 29 and Vitest still have: the spy
// aliases (toBeCalled for toHaveBeenCalled, and the rest) and toThrowError.
// A project on Jest 29 gets them as built-in matchers, as Jest 29 has them
// (framework.cjs), and Vitest's dialect gets them as Vitest's
// (vitest-expect.cjs). A spy alias is the matcher it stands for, so its
// failure message names that matcher, since the ported spy matchers name
// themselves; toThrowError is built under its own name.
'use strict';

const {getMatchers} = require('./jest-expect-jestMatchersObject.cjs');
const {createMatcher: createThrowMatcher} = require('./jest-expect-toThrowMatchers.cjs');

const ALIASES = {
  lastCalledWith: 'toHaveBeenLastCalledWith',
  lastReturnedWith: 'toHaveLastReturnedWith',
  nthCalledWith: 'toHaveBeenNthCalledWith',
  nthReturnedWith: 'toHaveNthReturnedWith',
  toBeCalled: 'toHaveBeenCalled',
  toBeCalledTimes: 'toHaveBeenCalledTimes',
  toBeCalledWith: 'toHaveBeenCalledWith',
  toReturn: 'toHaveReturned',
  toReturnTimes: 'toHaveReturnedTimes',
  toReturnWith: 'toHaveReturnedWith',
};

function removedAliases() {
  const builtIn = getMatchers();
  const aliases = {};
  for (const [alias, target] of Object.entries(ALIASES)) {
    const matcher = builtIn[target];
    aliases[alias] = function (...args) {
      return matcher.apply(this, args);
    };
  }
  // Jest 29 took `.rejects.toThrowError` from the promise, as `.rejects.toThrow`.
  aliases.toThrowError = function (received, expected) {
    return createThrowMatcher('toThrowError', Boolean(this.promise)).call(this, received, expected);
  };
  return aliases;
}

module.exports = {removedAliases};
