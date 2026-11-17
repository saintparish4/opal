// Ported from jest/packages/jest-matcher-utils/src/Replaceable.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, 'default', {enumerable: true, get: () => Replaceable});
const _m0 = require("./jest-get-type.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */



const supportTypes = new Set(['map', 'array', 'object']);

                                   
                 
               
                  
          

/* eslint-disable @typescript-eslint/explicit-module-boundary-types */
class Replaceable {
  object     ;
  type        ;

  constructor(object     ) {
    this.object = object;
    this.type = _m0.getType(object);
    if (!supportTypes.has(this.type)) {
      throw new Error(`Type ${this.type} is not support in Replaceable!`);
    }
  }

  static isReplaceable(obj1         , obj2         )          {
    const obj1Type = _m0.getType(obj1);
    const obj2Type = _m0.getType(obj2);
    return obj1Type === obj2Type && supportTypes.has(obj1Type);
  }

  forEach(cb                            )       {
    if (this.type === 'object') {
      const descriptors = Object.getOwnPropertyDescriptors(this.object);
      for (const key of [
        ...Object.keys(descriptors),
        ...Object.getOwnPropertySymbols(descriptors),
      ]
        //@ts-expect-error because typescript do not support symbol key in object
        //https://github.com/microsoft/TypeScript/issues/1863
        .filter(key => descriptors[key].enumerable)) {
        cb(this.object[key], key, this.object);
      }
    } else {
      // eslint-disable-next-line unicorn/no-array-for-each
      this.object.forEach(cb);
    }
  }

  get(key     )      {
    if (this.type === 'map') {
      return this.object.get(key);
    }
    return this.object[key];
  }

  set(key     , value     )       {
    if (this.type === 'map') {
      this.object.set(key, value);
    } else {
      this.object[key] = value;
    }
  }
}
/* eslint-enable */
