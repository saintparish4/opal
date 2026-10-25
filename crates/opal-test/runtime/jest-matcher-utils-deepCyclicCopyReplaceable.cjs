// Ported from jest/packages/jest-matcher-utils/src/deepCyclicCopyReplaceable.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "SERIALIZABLE_PROPERTIES", {enumerable: true, get: () => SERIALIZABLE_PROPERTIES});
Object.defineProperty(exports, 'default', {enumerable: true, get: () => deepCyclicCopyReplaceable});
const _m0 = require("./format.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */



const builtInObject                 = [
  Array,
  Date,
  Float32Array,
  Float64Array,
  Int16Array,
  Int32Array,
  Int8Array,
  Map,
  Set,
  RegExp,
  Uint16Array,
  Uint32Array,
  Uint8Array,
  Uint8ClampedArray,
];

if (typeof Buffer !== 'undefined') {
  builtInObject.push(Buffer);
}

if (typeof Window !== 'undefined') {
  builtInObject.push(Window);
}

const SERIALIZABLE_PROPERTIES = Symbol.for(
  '@jest/serializableProperties',
);

const isBuiltInObject = (object     ) =>
  builtInObject.includes(object.constructor);

const isMap = (value     )                                 =>
  value.constructor === Map;

function deepCyclicCopyReplaceable   (
  value   ,
  cycles = new WeakMap          (),
)    {
  if (typeof value !== 'object' || value === null) {
    return value;
  } else if (cycles.has(value)) {
    return cycles.get(value);
  } else if (Array.isArray(value)) {
    return deepCyclicCopyArray(value, cycles);
  } else if (isMap(value)) {
    return deepCyclicCopyMap(value, cycles);
  } else if (isBuiltInObject(value)) {
    return value;
  } else if (_m0.plugins.DOMElement.test(value)) {
    return (value                      ).cloneNode(true)                ;
  } else {
    return deepCyclicCopyObject(value, cycles);
  }
}

function deepCyclicCopyObject   (object   , cycles                       )    {
  const newObject = Object.create(Object.getPrototypeOf(object));
  let descriptors                                              = {};
  let obj = object;
  do {
    const serializableProperties = getSerializableProperties(obj);

    if (serializableProperties === undefined) {
      descriptors = Object.assign(
        Object.create(null),
        Object.getOwnPropertyDescriptors(obj),
        descriptors,
      );
    } else {
      for (const property of serializableProperties) {
        if (!descriptors[property]) {
          descriptors[property] = Object.getOwnPropertyDescriptor(
            obj,
            property,
          ) ;
        }
      }
    }
  } while (
    (obj = Object.getPrototypeOf(obj)) &&
    obj !== Object.getPrototypeOf({})
  );

  cycles.set(object, newObject);

  const newDescriptors = [
    ...Object.keys(descriptors),
    ...Object.getOwnPropertySymbols(descriptors),
  ].reduce(
    //@ts-expect-error because typescript do not support symbol key in object
    //https://github.com/microsoft/TypeScript/issues/1863
    (newDescriptors                                   , key        ) => {
      const enumerable = descriptors[key].enumerable;
      const descriptor = descriptors[key];

      let value;

      if (descriptor.get) {
        const getterRes = (object                                    )[key];
        const isSelfReferential =
          getterRes?.constructor === (object       ).constructor &&
          getterRes?.constructor !== Object;

        value = isSelfReferential
          ? '[Getter]'
          : deepCyclicCopyReplaceable(getterRes, cycles);
      } else {
        value = deepCyclicCopyReplaceable(
          (object                                    )[key],
          cycles,
        );
      }

      newDescriptors[key] = {
        configurable: true,
        enumerable,
        value,
        writable: true,
      };
      return newDescriptors;
    },
    Object.create(null),
  );
  //@ts-expect-error because typescript do not support symbol key in object
  //https://github.com/microsoft/TypeScript/issues/1863
  return Object.defineProperties(newObject, newDescriptors);
}

function deepCyclicCopyArray   (
  array          ,
  cycles                       ,
)    {
  const newArray = new (Object.getPrototypeOf(array).constructor)(array.length);
  const length = array.length;

  cycles.set(array, newArray);

  for (let i = 0; i < length; i++) {
    newArray[i] = deepCyclicCopyReplaceable(array[i], cycles);
  }

  return newArray;
}

function deepCyclicCopyMap   (
  map                       ,
  cycles                       ,
)    {
  const newMap = new Map();

  cycles.set(map, newMap);

  for (const [key, value] of map) {
    newMap.set(key, deepCyclicCopyReplaceable(value, cycles));
  }

  return newMap       ;
}

function getSerializableProperties   (
  obj   ,
)                                     {
  if (typeof obj !== 'object' || obj === null) {
    return;
  }

  const serializableProperties          = (obj                                )[
    SERIALIZABLE_PROPERTIES
  ];

  if (!Array.isArray(serializableProperties)) {
    return;
  }

  return serializableProperties.filter(
    (key)                         =>
      typeof key === 'string' || typeof key === 'symbol',
  );
}
