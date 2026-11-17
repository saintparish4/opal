// Ported from jest/packages/jest-expect/src/index.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "jestExpect", {enumerable: true, get: () => jestExpect});
const _m0 = require("./jest-expect.cjs");
const _m1 = require("./jest-snapshot.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */



                                        

             
                     
           
                 
                  
                             
               
               
                
                                        

function createJestExpect()             {
  _m0.expect.extend({
    toMatchInlineSnapshot: _m1.toMatchInlineSnapshot,
    toMatchSnapshot: _m1.toMatchSnapshot,
    toThrowErrorMatchingInlineSnapshot: _m1.toThrowErrorMatchingInlineSnapshot,
    toThrowErrorMatchingSnapshot: _m1.toThrowErrorMatchingSnapshot,
  });

  _m0.expect.addSnapshotSerializer = _m1.addSerializer;

  return _m0.expect              ;
}

const jestExpect = createJestExpect();
