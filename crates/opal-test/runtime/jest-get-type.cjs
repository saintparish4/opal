// Ported from jest/packages/jest-get-type/src/index.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "getType", {enumerable: true, get: () => getType});
Object.defineProperty(exports, "isPrimitive", {enumerable: true, get: () => isPrimitive});


/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

                
           
            
             
              
          
            
            
            
         
         
          
            
            
                

// get the type of a value with handling the edge cases like `typeof []`
// and `typeof null`
function getType(value         )            {
  if (value === undefined) {
    return 'undefined';
  } else if (value === null) {
    return 'null';
  } else if (Array.isArray(value)) {
    return 'array';
  } else if (typeof value === 'boolean') {
    return 'boolean';
  } else if (typeof value === 'function') {
    return 'function';
  } else if (typeof value === 'number') {
    return 'number';
  } else if (typeof value === 'string') {
    return 'string';
  } else if (typeof value === 'bigint') {
    return 'bigint';
  } else if (typeof value === 'object') {
    if (value.constructor === RegExp) {
      return 'regexp';
    } else if (value.constructor === Map) {
      return 'map';
    } else if (value.constructor === Set) {
      return 'set';
    } else if (value.constructor === Date) {
      return 'date';
    }
    return 'object';
  } else if (typeof value === 'symbol') {
    return 'symbol';
  }

  throw new Error(`value of unknown type: ${value}`);
}

const isPrimitive = (
  value         ,
)                                                                          =>
  Object(value) !== value;
