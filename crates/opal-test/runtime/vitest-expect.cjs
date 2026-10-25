// What Vitest's `expect` has beyond Jest's, added with expect.extend to the
// `expect` of a file in Vitest's dialect. The definitions follow
// @vitest/expect (jest-expect.ts and custom-matchers.ts) for what passes;
// failure messages are written the way Jest writes its own.
//
//   - matchers Jest doesn't have: toHaveBeenCalledOnce,
//     toHaveBeenCalledExactlyOnceWith, toHaveBeenCalledBefore and After,
//     the toHaveResolved family (reading mock.settledResults, which the
//     Vitest dialect's mocks record; see mock.cjs), toBeTypeOf, toBeOneOf,
//     toSatisfy and toBeNullable;
//   - the aliases Jest 30 removed and Vitest keeps: toBeCalled,
//     toBeCalledTimes, toBeCalledWith, lastCalledWith, nthCalledWith,
//     toReturn, toReturnTimes, toReturnWith, lastReturnedWith,
//     nthReturnedWith and toThrowError;
//   - the snapshot matchers for what's thrown, which snapshot the thrown
//     value itself where Jest snapshots its message (getError in
//     @vitest/expect);
//   - expect.soft and expect.poll.
'use strict';

const util = require('util');
const {getMatchers} = require('./jest-expect-jestMatchersObject.cjs');
const {createMatcher: createThrowMatcher} = require('./jest-expect-toThrowMatchers.cjs');
const snapshot = require('./jest-snapshot.cjs');

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

const ordinal = n => {
  const tens = n % 100;
  if (tens >= 11 && tens <= 13) return `${n}th`;
  return `${n}${['th', 'st', 'nd', 'rd'][n % 10] || 'th'}`;
};

function installVitestExpect(expect, {currentTest}) {
  const builtIn = getMatchers();
  const aliases = Object.fromEntries(
    Object.entries(ALIASES).map(([alias, target]) => {
      const matcher = builtIn[target];
      return [alias, function (...args) {
        return matcher.apply(this, args);
      }];
    }),
  );

  function spyOf(context, received, matcherName) {
    if (!(received && received._isMockFunction === true && received.mock)) {
      throw new Error(
        context.utils.matcherErrorMessage(
          context.utils.matcherHint(matcherName, undefined, '', {isNot: context.isNot, promise: context.promise}),
          `${context.utils.RECEIVED_COLOR('received')} value must be a mock or spy function`,
          context.utils.printWithType('Received', received, context.utils.printReceived),
        ),
      );
    }
    return received;
  }

  // pass and the two messages, Jest's way: a hint, then the detail.
  const result = (context, matcherName, pass, expected, detail, args = 'expected') => ({
    message: () =>
      `${context.utils.matcherHint(`${context.isNot ? '.not' : ''}.${matcherName}`, 'received', args === '' ? '' : args)}\n\n${detail()}`,
    pass,
  });

  const settled = spy => spy.mock.settledResults || [];
  const fulfilled = spy => settled(spy).filter(({type}) => type === 'fulfilled');
  const name = spy => spy.getMockName();
  const printResults = (context, results) =>
    `Received: ${results.map(({type, value}) => `${type} ${context.utils.printReceived(value)}`).join(', ') || 'no calls'}`;

  function wasCalledBefore(before, after, failIfNoFirstInvocation) {
    const first = before.mock.invocationCallOrder;
    const second = after.mock.invocationCallOrder;
    if (first.length === 0) return !failIfNoFirstInvocation;
    if (second.length === 0) return false;
    return first[0] < second[0];
  }

  // getError in @vitest/expect.
  function thrownBy(received, fromPromise) {
    if (typeof received !== 'function') {
      if (!fromPromise) throw new Error(`expected must be a function, received ${typeof received}`);
      return received;
    }
    try {
      received();
    } catch (error) {
      return error;
    }
    throw new Error("snapshot function didn't throw");
  }

  expect.extend({
    ...aliases,
    toBeNullable(received) {
      return result(this, 'toBeNullable', received == null, null, () => `Received: ${this.utils.printReceived(received)}`, '');
    },
    toBeOneOf(received, expected) {
      let pass;
      if (Array.isArray(expected)) {
        pass = expected.length === 0 || expected.some(item => this.equals(item, received, this.customTesters));
      } else if (expected instanceof Set) {
        pass = expected.size === 0 || expected.has(received) || [...expected].some(item => this.equals(item, received, this.customTesters));
      } else {
        throw new TypeError(`You must provide an array or set to ${this.utils.matcherHint('.toBeOneOf')}, not '${typeof expected}'.`);
      }
      return result(this, 'toBeOneOf', pass, expected, () =>
        `Expected value to ${this.isNot ? 'not ' : ''}be one of: ${this.utils.printExpected(expected)}\nReceived: ${this.utils.printReceived(received)}`,
      );
    },
    toBeTypeOf(received, expected) {
      const actual = typeof received;
      return result(this, 'toBeTypeOf', actual === expected, expected, () =>
        `Expected type: ${this.isNot ? 'not ' : ''}${this.utils.printExpected(expected)}\nReceived type: ${this.utils.printReceived(actual)}`,
      );
    },
    toSatisfy(received, predicate, message) {
      const pass = Boolean(predicate(received));
      return result(this, 'toSatisfy', pass, predicate, () =>
        `Expected value to ${this.isNot ? 'not ' : ''}satisfy:\n${message || this.utils.printExpected(predicate)}\nReceived: ${this.utils.printReceived(received)}`,
      );
    },
    toHaveBeenCalledOnce(received) {
      const spy = spyOf(this, received, 'toHaveBeenCalledOnce');
      const count = spy.mock.calls.length;
      return result(this, 'toHaveBeenCalledOnce', count === 1, 1, () =>
        `Expected "${name(spy)}" to ${this.isNot ? 'not ' : ''}be called once\nReceived number of calls: ${this.utils.printReceived(count)}`,
      '');
    },
    toHaveBeenCalledExactlyOnceWith(received, ...expected) {
      const spy = spyOf(this, received, 'toHaveBeenCalledExactlyOnceWith');
      const calls = spy.mock.calls;
      const pass = calls.length === 1 && this.equals(calls[0], expected, [...this.customTesters, this.utils.iterableEquality]);
      return result(this, 'toHaveBeenCalledExactlyOnceWith', pass, expected, () =>
        `Expected "${name(spy)}" to ${this.isNot ? 'not ' : ''}be called once with: ${this.utils.printExpected(expected)}\nReceived calls: ${this.utils.printReceived(calls)}`,
      );
    },
    toHaveBeenCalledBefore(received, other, failIfNoFirstInvocation = true) {
      const spy = spyOf(this, received, 'toHaveBeenCalledBefore');
      if (!(other && other._isMockFunction === true)) throw new TypeError(`${util.inspect(other)} is not a spy or a call to a spy`);
      return result(this, 'toHaveBeenCalledBefore', wasCalledBefore(spy, other, failIfNoFirstInvocation), other, () =>
        `Expected "${name(spy)}" to ${this.isNot ? 'not ' : ''}have been called before "${name(other)}"`,
      );
    },
    toHaveBeenCalledAfter(received, other, failIfNoFirstInvocation = true) {
      const spy = spyOf(this, received, 'toHaveBeenCalledAfter');
      if (!(other && other._isMockFunction === true)) throw new TypeError(`${util.inspect(other)} is not a spy or a call to a spy`);
      return result(this, 'toHaveBeenCalledAfter', wasCalledBefore(other, spy, failIfNoFirstInvocation), other, () =>
        `Expected "${name(spy)}" to ${this.isNot ? 'not ' : ''}have been called after "${name(other)}"`,
      );
    },
    toHaveResolved(received) {
      const spy = spyOf(this, received, 'toHaveResolved');
      return result(this, 'toHaveResolved', fulfilled(spy).length > 0, undefined, () =>
        `Expected "${name(spy)}" to ${this.isNot ? 'not ' : ''}have resolved at least once\n${printResults(this, settled(spy))}`,
      '');
    },
    toHaveResolvedTimes(received, times) {
      const spy = spyOf(this, received, 'toHaveResolvedTimes');
      const count = fulfilled(spy).length;
      return result(this, 'toHaveResolvedTimes', count === times, times, () =>
        `Expected number of resolutions: ${this.isNot ? 'not ' : ''}${this.utils.printExpected(times)}\nReceived number of resolutions: ${this.utils.printReceived(count)}`,
      );
    },
    toHaveResolvedWith(received, expected) {
      const spy = spyOf(this, received, 'toHaveResolvedWith');
      const pass = fulfilled(spy).some(({value}) => this.equals(expected, value));
      return result(this, 'toHaveResolvedWith', pass, expected, () =>
        `Expected: ${this.isNot ? 'not ' : ''}${this.utils.printExpected(expected)}\n${printResults(this, settled(spy))}`,
      );
    },
    toHaveLastResolvedWith(received, expected) {
      const spy = spyOf(this, received, 'toHaveLastResolvedWith');
      const last = settled(spy).at(-1);
      const pass = Boolean(last && last.type === 'fulfilled' && this.equals(last.value, expected));
      return result(this, 'toHaveLastResolvedWith', pass, expected, () =>
        `Expected: ${this.isNot ? 'not ' : ''}${this.utils.printExpected(expected)}\n${printResults(this, last ? [last] : [])}`,
      );
    },
    toHaveNthResolvedWith(received, nth, expected) {
      const spy = spyOf(this, received, 'toHaveNthResolvedWith');
      const entry = settled(spy)[nth - 1];
      const pass = Boolean(entry && entry.type === 'fulfilled' && this.equals(entry.value, expected));
      return result(this, 'toHaveNthResolvedWith', pass, expected, () =>
        `${ordinal(nth)} call\nExpected: ${this.isNot ? 'not ' : ''}${this.utils.printExpected(expected)}\n${printResults(this, entry ? [entry] : [])}`,
      'n, expected');
    },
    toThrowError(received, expected) {
      return createThrowMatcher('toThrowError', Boolean(this.promise)).call(this, received, expected);
    },
    toThrowErrorMatchingSnapshot(received, hint, fromPromise) {
      return snapshot.toMatchSnapshot.call(this, thrownBy(received, fromPromise), ...(hint === undefined ? [] : [hint]));
    },
    toThrowErrorMatchingInlineSnapshot(received, inlineSnapshot, fromPromise) {
      return snapshot.toMatchInlineSnapshot.call(this, thrownBy(received, fromPromise), ...(inlineSnapshot === undefined ? [] : [inlineSnapshot]));
    },
  });

  // A soft assertion's failure fails the test without stopping it.
  expect.soft = (...args) => {
    const record = error => {
      const test = currentTest();
      if (!test) throw error;
      test.errors.push(error);
    };
    const soften = target =>
      new Proxy(target, {
        get(object, key) {
          const value = object[key];
          if (typeof value === 'function') {
            return (...matcherArgs) => {
              try {
                const outcome = value(...matcherArgs);
                return outcome && typeof outcome.then === 'function' ? outcome.catch(record) : outcome;
              } catch (error) {
                record(error);
              }
            };
          }
          return value && typeof value === 'object' ? soften(value) : value;
        },
      });
    return soften(expect(...args));
  };

  // Retries the assertion on the callback's latest value until it passes or
  // the time is up; every matcher returns a promise.
  expect.poll = (callback, options = {}) => {
    const {interval = 50, timeout = 1000} = typeof options === 'number' ? {timeout: options} : options;
    const chain = negated =>
      new Proxy({}, {
        get(_object, key) {
          if (key === 'not') return chain(!negated);
          if (key === 'then') return undefined;
          return async (...matcherArgs) => {
            const deadline = Date.now() + timeout;
            for (;;) {
              try {
                const expectation = expect(await callback());
                return await (negated ? expectation.not : expectation)[key](...matcherArgs);
              } catch (error) {
                if (Date.now() >= deadline) throw error;
              }
              await new Promise(resolve => setTimeout(resolve, interval));
            }
          };
        },
      });
    return chain(false);
  };
}

module.exports = {installVitestExpect};
