// Ported from jest/packages/jest-snapshot/src/plugins.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "addSerializer", {enumerable: true, get: () => addSerializer});
Object.defineProperty(exports, "getSerializers", {enumerable: true, get: () => getSerializers});
const _m0 = require("./format.cjs");
const _m1 = require("./jest-snapshot-mockSerializer.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */




const {
  DOMCollection,
  DOMElement,
  Immutable,
  ReactElement,
  ReactTestComponent,
  AsymmetricMatcher,
} = _m0.plugins;

let PLUGINS                      = [
  ReactTestComponent,
  ReactElement,
  DOMElement,
  DOMCollection,
  Immutable,
  (_m1.default ?? _m1),
  AsymmetricMatcher,
];

// Prepend to list so the last added is the first tested.
const addSerializer = (plugin                    )       => {
  PLUGINS = [plugin, ...PLUGINS];
};

const getSerializers = ()                      => PLUGINS;
