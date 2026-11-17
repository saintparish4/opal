// Ported from jest/packages/expect-utils/src/utils.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "getObjectKeys", {enumerable: true, get: () => getObjectKeys});
Object.defineProperty(exports, "getPath", {enumerable: true, get: () => getPath});
Object.defineProperty(exports, "getObjectSubset", {enumerable: true, get: () => getObjectSubset});
Object.defineProperty(exports, "iterableEquality", {enumerable: true, get: () => iterableEquality});
Object.defineProperty(exports, "subsetEquality", {enumerable: true, get: () => subsetEquality});
Object.defineProperty(exports, "typeEquality", {enumerable: true, get: () => typeEquality});
Object.defineProperty(exports, "arrayBufferEquality", {enumerable: true, get: () => arrayBufferEquality});
Object.defineProperty(exports, "sparseArrayEquality", {enumerable: true, get: () => sparseArrayEquality});
Object.defineProperty(exports, "partition", {enumerable: true, get: () => partition});
Object.defineProperty(exports, "pathAsArray", {enumerable: true, get: () => pathAsArray});
Object.defineProperty(exports, "isError", {enumerable: true, get: () => isError});
Object.defineProperty(exports, "emptyObject", {enumerable: true, get: () => emptyObject});
Object.defineProperty(exports, "isOneline", {enumerable: true, get: () => isOneline});
const _m0 = require("./jest-get-type.cjs");
const _m1 = require("./jest-expect-utils-immutableUtils.cjs");
const _m2 = require("./jest-expect-utils-jasmineUtils.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 */




                                    

                
                       
                             
                               
                               
                  
  

/**
 * Checks if `hasOwnProperty(object, key)` up the prototype chain, stopping at `Object.prototype`.
 */
const hasPropertyInObject = (object        , key                 )          => {
  const shouldTerminate =
    !object || typeof object !== 'object' || object === Object.prototype;

  if (shouldTerminate) {
    return false;
  }

  return (
    Object.prototype.hasOwnProperty.call(object, key) ||
    hasPropertyInObject(Object.getPrototypeOf(object), key)
  );
};

// Retrieves an object's keys for evaluation by getObjectSubset.  This evaluates
// the prototype chain for string keys but not for non-enumerable symbols.
// (Otherwise, it could find values such as a Set or Map's Symbol.toStringTag,
// with unexpected results.)
const getObjectKeys = (object        )                         => {
  return [
    ...Object.keys(object),
    ...Object.getOwnPropertySymbols(object).filter(
      s => Object.getOwnPropertyDescriptor(object, s)?.enumerable,
    ),
  ];
};

const getPath = (
  object                     ,
  propertyPath                        ,
)          => {
  if (!Array.isArray(propertyPath)) {
    propertyPath = pathAsArray(propertyPath);
  }

  if (propertyPath.length > 0) {
    const lastProp = propertyPath.length === 1;
    const prop = propertyPath[0];
    const newObject = object[prop];

    if (!lastProp && (newObject === null || newObject === undefined)) {
      // This is not the last prop in the chain. If we keep recursing it will
      // hit a `can't access property X of undefined | null`. At this point we
      // know that the chain has broken and we can return right away.
      return {
        hasEndProp: false,
        lastTraversedObject: object,
        traversedPath: [],
      };
    }

    const result = getPath(newObject, propertyPath.slice(1));

    if (result.lastTraversedObject === null) {
      result.lastTraversedObject = object;
    }

    result.traversedPath.unshift(prop);

    if (lastProp) {
      // Does object have the property with an undefined value?
      // Although primitive values support bracket notation (above)
      // they would throw TypeError for in operator (below).
      result.endPropIsDefined = !_m0.isPrimitive(object) && prop in object;
      result.hasEndProp = newObject !== undefined || result.endPropIsDefined;

      if (!result.hasEndProp) {
        result.traversedPath.shift();
      }
    }

    return result;
  }

  return {
    lastTraversedObject: null,
    traversedPath: [],
    value: object,
  };
};

// Strip properties from object that are not present in the subset. Useful for
// printing the diff for toMatchObject() without adding unrelated noise.
/* eslint-disable @typescript-eslint/explicit-module-boundary-types */
const getObjectSubset = (
  object     ,
  subset     ,
  customTesters                = [],
  seenReferences                           = new WeakMap(),
)      => {
  /* eslint-enable @typescript-eslint/explicit-module-boundary-types */
  if (Array.isArray(object)) {
    if (Array.isArray(subset) && subset.length === object.length) {
      // The map method returns correct subclass of subset.
      return subset.map((sub     , i        ) =>
        getObjectSubset(object[i], sub, customTesters),
      );
    }
  } else if (object instanceof Date) {
    return object;
  } else if (isObject(object) && isObject(subset)) {
    if (
      _m2.equals(object, subset, [
        ...customTesters,
        iterableEquality,
        subsetEquality,
      ])
    ) {
      // Avoid unnecessary copy which might return Object instead of subclass.
      return subset;
    }

    const trimmed      = {};
    seenReferences.set(object, trimmed);

    for (const key of getObjectKeys(object)) {
      if (!hasPropertyInObject(subset, key)) {
        continue;
      }

      trimmed[key] = seenReferences.has(object[key])
        ? seenReferences.get(object[key])
        : getObjectSubset(
            object[key],
            subset[key],
            customTesters,
            seenReferences,
          );
    }

    if (getObjectKeys(trimmed).length > 0) {
      return trimmed;
    }
  }
  return object;
};

const IteratorSymbol = Symbol.iterator;

const hasIterator = (object     ) =>
  !!(object != null && object[IteratorSymbol]);

/* eslint-disable @typescript-eslint/explicit-module-boundary-types */
const iterableEquality = (
  a     ,
  b     ,
  customTesters                = [],
  /* eslint-enable @typescript-eslint/explicit-module-boundary-types */
  aStack             = [],
  bStack             = [],
)                      => {
  if (
    typeof a !== 'object' ||
    typeof b !== 'object' ||
    Array.isArray(a) ||
    Array.isArray(b) ||
    ArrayBuffer.isView(a) ||
    ArrayBuffer.isView(b) ||
    !hasIterator(a) ||
    !hasIterator(b)
  ) {
    return undefined;
  }
  if (a.constructor !== b.constructor) {
    // Same cross-realm constructor check as typeEquality — see #14011.
    // https://github.com/jestjs/jest/issues/14011
    if (
      a.constructor == null ||
      b.constructor == null ||
      a.constructor.name !== b.constructor.name ||
      !isNativeFunction(a.constructor) ||
      !isNativeFunction(b.constructor)
    ) {
      return false;
    }
  }
  let length = aStack.length;
  while (length--) {
    // Linear search. Performance is inversely proportional to the number of
    // unique nested structures.
    // circular references at same depth are equal
    // circular reference is not equal to non-circular one
    if (aStack[length] === a) {
      return bStack[length] === b;
    }
  }
  aStack.push(a);
  bStack.push(b);

  const iterableEqualityWithStack = (a     , b     ) =>
    iterableEquality(
      a,
      b,
      [...filteredCustomTesters],
      [...aStack],
      [...bStack],
    );

  // Replace any instance of iterableEquality with the new
  // iterableEqualityWithStack so we can do circular detection
  const filteredCustomTesters                = [
    ...customTesters.filter(t => t !== iterableEquality),
    iterableEqualityWithStack,
  ];

  if (a.size !== undefined) {
    if (a.size !== b.size) {
      return false;
    } else if (_m2.isA              ('Set', a) || _m1.isImmutableUnorderedSet(a)) {
      let allFound = true;
      for (const aValue of a) {
        if (!b.has(aValue)) {
          let has = false;
          for (const bValue of b) {
            const isEqual = _m2.equals(aValue, bValue, filteredCustomTesters);
            if (isEqual === true) {
              has = true;
            }
          }

          if (has === false) {
            allFound = false;
            break;
          }
        }
      }
      // Remove the first value from the stack of traversed values.
      aStack.pop();
      bStack.pop();
      return allFound;
    } else if (
      _m2.isA                       ('Map', a) ||
      _m1.isImmutableUnorderedKeyed(a)
    ) {
      let allFound = true;
      for (const aEntry of a) {
        if (
          !b.has(aEntry[0]) ||
          !_m2.equals(aEntry[1], b.get(aEntry[0]), filteredCustomTesters)
        ) {
          let has = false;
          for (const bEntry of b) {
            const matchedKey = _m2.equals(
              aEntry[0],
              bEntry[0],
              filteredCustomTesters,
            );

            let matchedValue = false;
            if (matchedKey === true) {
              matchedValue = _m2.equals(
                aEntry[1],
                bEntry[1],
                filteredCustomTesters,
              );
            }
            if (matchedValue === true) {
              has = true;
            }
          }

          if (has === false) {
            allFound = false;
            break;
          }
        }
      }
      // Remove the first value from the stack of traversed values.
      aStack.pop();
      bStack.pop();
      return allFound;
    }
  }

  let aIterator                   ;
  let bIterator                   ;
  try {
    aIterator = a[IteratorSymbol]();
    bIterator = b[IteratorSymbol]();
  } catch {
    // If the iterator factory itself throws (e.g. a TypedArray method used as
    // [Symbol.iterator] on a plain object), we cannot compare as iterables.
    // Return undefined so equals() falls through to Object.is / property checks.
    aStack.pop();
    bStack.pop();
    return undefined;
  }

  let aStep = aIterator.next();
  while (!aStep.done) {
    const bStep = bIterator.next();
    if (
      bStep.done ||
      !_m2.equals(aStep.value, bStep.value, filteredCustomTesters)
    ) {
      return false;
    }
    aStep = aIterator.next();
  }
  if (!bIterator.next().done) {
    return false;
  }

  if (
    !_m1.isImmutableList(a) &&
    !_m1.isImmutableOrderedKeyed(a) &&
    !_m1.isImmutableOrderedSet(a) &&
    !_m1.isImmutableRecord(a)
  ) {
    const aEntries = entries(a);
    const bEntries = entries(b);
    if (!_m2.equals(aEntries, bEntries)) {
      return false;
    }
  }

  // Remove the first value from the stack of traversed values.
  aStack.pop();
  bStack.pop();
  return true;
};

const entries = (obj     ) => {
  if (!isObject(obj)) return [];

  const symbolProperties = Object.getOwnPropertySymbols(obj)
    .filter(key => key !== Symbol.iterator)
    .map(key => [key, obj[key]]);

  return [...symbolProperties, ...Object.entries(obj)];
};

const isObject = (a     ) => a !== null && typeof a === 'object';

const isObjectWithKeys = (a     ) =>
  isObject(a) &&
  !(a instanceof Error) &&
  !Array.isArray(a) &&
  !(a instanceof Date) &&
  !(a instanceof Set) &&
  !(a instanceof Map);

const subsetEquality = (
  object         ,
  subset         ,
  customTesters                = [],
)                      => {
  const filteredCustomTesters = customTesters.filter(t => t !== subsetEquality);

  // subsetEquality needs to keep track of the references
  // it has already visited to avoid infinite loops in case
  // there are circular references in the subset passed to it.
  const subsetEqualityWithContext =
    (seenReferences                           = new WeakMap()) =>
    (object     , subset     )                      => {
      if (!isObjectWithKeys(subset)) {
        return undefined;
      }

      if (seenReferences.has(subset)) return undefined;
      seenReferences.set(subset, true);

      const matchResult = getObjectKeys(subset).every(key => {
        if (isObjectWithKeys(subset[key])) {
          if (seenReferences.has(subset[key])) {
            return _m2.equals(object[key], subset[key], filteredCustomTesters);
          }
        }
        const result =
          object != null &&
          hasPropertyInObject(object, key) &&
          _m2.equals(object[key], subset[key], [
            ...filteredCustomTesters,
            subsetEqualityWithContext(seenReferences),
          ]);
        // The main goal of using seenReference is to avoid circular node on tree.
        // It will only happen within a parent and its child, not a node and nodes next to it (same level)
        // We should keep the reference for a parent and its child only
        // Thus we should delete the reference immediately so that it doesn't interfere
        // other nodes within the same level on tree.
        seenReferences.delete(subset[key]);
        return result;
      });
      seenReferences.delete(subset);
      return matchResult;
    };

  return subsetEqualityWithContext()(object, subset);
};

// Returns true if `fn` is a native function (its toString contains "[native code]").
function isNativeFunction(fn         )          {
  return (
    typeof fn === 'function' &&
    Function.prototype.toString.call(fn).includes('[native code]')
  );
}

// eslint-disable-next-line @typescript-eslint/explicit-module-boundary-types
const typeEquality = (a     , b     )                      => {
  if (
    a == null ||
    b == null ||
    a.constructor === b.constructor ||
    // Since Jest globals are different from Node globals,
    // constructors are different even between arrays when comparing properties of mock objects.
    // Both of them should be able to compare correctly when they are array-to-array.
    // https://github.com/jestjs/jest/issues/2549
    (Array.isArray(a) && Array.isArray(b))
  ) {
    return undefined;
  }

  // structuredClone (and other cross-realm calls) return objects whose
  // constructors come from a different VM context, so identity checks fail.
  // Fall back to comparing constructor names for native built-ins only —
  // user-defined classes still need identity equality.
  // https://github.com/jestjs/jest/issues/14011
  if (
    a.constructor != null &&
    b.constructor != null &&
    a.constructor.name === b.constructor.name &&
    isNativeFunction(a.constructor) &&
    isNativeFunction(b.constructor)
  ) {
    return undefined;
  }

  return false;
};

const arrayBufferEquality = (
  a         ,
  b         ,
)                      => {
  let dataViewA = a;
  let dataViewB = b;

  if (isArrayBuffer(a) && isArrayBuffer(b)) {
    dataViewA = new DataView(a);
    dataViewB = new DataView(b);
  } else if (ArrayBuffer.isView(a) && ArrayBuffer.isView(b)) {
    dataViewA = new DataView(a.buffer, a.byteOffset, a.byteLength);
    dataViewB = new DataView(b.buffer, b.byteOffset, b.byteLength);
  }

  if (!(dataViewA instanceof DataView && dataViewB instanceof DataView)) {
    return undefined;
  }

  // Buffers are not equal when they do not have the same byte length
  if (dataViewA.byteLength !== dataViewB.byteLength) {
    return false;
  }

  // Check if every byte value is equal to each other
  for (let i = 0; i < dataViewA.byteLength; i++) {
    if (dataViewA.getUint8(i) !== dataViewB.getUint8(i)) {
      return false;
    }
  }

  return true;
};

function isArrayBuffer(obj         )                     {
  return Object.prototype.toString.call(obj) === '[object ArrayBuffer]';
}

const sparseArrayEquality = (
  a         ,
  b         ,
  customTesters                = [],
)                      => {
  if (!Array.isArray(a) || !Array.isArray(b)) {
    return undefined;
  }

  // A sparse array [, , 1] will have keys ["2"] whereas [undefined, undefined, 1] will have keys ["0", "1", "2"]
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  return (
    _m2.equals(
      a,
      b,
      customTesters.filter(t => t !== sparseArrayEquality),
      true,
    ) && _m2.equals(aKeys, bKeys)
  );
};

const partition =    (
  items          ,
  predicate                     ,
)                       => {
  const result                       = [[], []];

  for (const item of items) result[predicate(item) ? 0 : 1].push(item);

  return result;
};

const pathAsArray = (propertyPath        )             => {
  const properties                = [];

  if (propertyPath === '') {
    properties.push('');
    return properties;
  }

  // will match everything that's not a dot or a bracket, and "" for consecutive dots.
  const pattern = new RegExp('[^.[\\]]+|(?=(?:\\.)(?:\\.|$))', 'g');

  // Because the regex won't match a dot in the beginning of the path, if present.
  if (propertyPath[0] === '.') {
    properties.push('');
  }

  propertyPath.replaceAll(pattern, match => {
    properties.push(match);
    return match;
  });

  return properties;
};

// Copied from https://github.com/graingert/angular.js/blob/a43574052e9775cbc1d7dd8a086752c979b0f020/src/Angular.js#L685-L693
const isError = (value         )                 => {
  switch (Object.prototype.toString.call(value)) {
    case '[object Error]':
    case '[object Exception]':
    case '[object DOMException]':
      return true;
    default:
      return value instanceof Error;
  }
};

function emptyObject(obj         )          {
  return obj && typeof obj === 'object' ? Object.keys(obj).length === 0 : false;
}

const MULTILINE_REGEXP = /[\n\r]/;

const isOneline = (expected         , received         )          =>
  typeof expected === 'string' &&
  typeof received === 'string' &&
  (!MULTILINE_REGEXP.test(expected) || !MULTILINE_REGEXP.test(received));
