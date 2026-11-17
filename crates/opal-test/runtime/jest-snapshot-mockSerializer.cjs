// Ported from jest/packages/jest-snapshot/src/mockSerializer.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "serialize", {enumerable: true, get: () => serialize});
Object.defineProperty(exports, "test", {enumerable: true, get: () => test});
Object.defineProperty(exports, 'default', {enumerable: true, get: () => _default});


/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

                                             

const serialize                         = (
  val,
  config,
  indentation,
  depth,
  refs,
  printer,
)         => {
  // Serialize a non-default name, even if config.printFunctionName is false.
  const name = val.getMockName();
  const nameString = name === 'jest.fn()' ? '' : ` ${name}`;

  let callsString = '';
  if (val.mock.calls.length > 0) {
    const indentationNext = indentation + config.indent;
    callsString = ` {${config.spacingOuter}${indentationNext}"calls": ${printer(
      val.mock.calls,
      config,
      indentationNext,
      depth,
      refs,
    )}${config.min ? ', ' : ','}${
      config.spacingOuter
    }${indentationNext}"results": ${printer(
      val.mock.results,
      config,
      indentationNext,
      depth,
      refs,
    )}${config.min ? '' : ','}${config.spacingOuter}${indentation}}`;
  }

  return `[MockFunction${nameString}]${callsString}`;
};

const test                    = val => val && !!val._isMockFunction;

const plugin            = {serialize, test};

const _default = plugin;
