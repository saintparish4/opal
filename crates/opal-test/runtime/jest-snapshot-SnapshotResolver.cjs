// Ported from jest/packages/jest-snapshot/src/SnapshotResolver.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "EXTENSION", {enumerable: true, get: () => EXTENSION});
Object.defineProperty(exports, "DOT_EXTENSION", {enumerable: true, get: () => DOT_EXTENSION});
Object.defineProperty(exports, "isSnapshotPath", {enumerable: true, get: () => isSnapshotPath});
Object.defineProperty(exports, "buildSnapshotResolver", {enumerable: true, get: () => buildSnapshotResolver});
const _m0 = require("node:path");
const _m1 = require("./support.cjs");
const _m2 = require("./support.cjs");
const _m3 = require("./support.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */




                                        


                                
                                                   
                                                                            
                                                   
                                                                            
                                                                                             
                                      
  

const EXTENSION = 'snap';
const DOT_EXTENSION = `.${EXTENSION}`;

const isSnapshotPath = (path        )          =>
  path.endsWith(DOT_EXTENSION);

const cache = new Map                          ();

                                                

const buildSnapshotResolver = async (
  config                      ,
  localRequire                                       = _m2.createTranspilingRequire(
    config,
  ),
)                            => {
  const key = config.rootDir;

  const resolver =
    cache.get(key) ??
    (await createSnapshotResolver(await localRequire, config.snapshotResolver));

  cache.set(key, resolver);

  return resolver;
};

async function createSnapshotResolver(
  localRequire              ,
  snapshotResolverPath                ,
)                            {
  return typeof snapshotResolverPath === 'string'
    ? createCustomSnapshotResolver(snapshotResolverPath, localRequire)
    : createDefaultSnapshotResolver();
}

function createDefaultSnapshotResolver()                   {
  return {
    resolveSnapshotPath: (testPath        ) =>
      _m0.join(
        _m0.join(_m0.dirname(testPath), '__snapshots__'),
        _m0.basename(testPath) + DOT_EXTENSION,
      ),

    resolveTestPath: (snapshotPath        ) =>
      _m0.resolve(
        _m0.dirname(snapshotPath),
        '..',
        _m0.basename(snapshotPath, DOT_EXTENSION),
      ),

    testPathForConsistencyCheck: _m0.posix.join(
      'consistency_check',
      '__tests__',
      'example.test.js',
    ),
  };
}

async function createCustomSnapshotResolver(
  snapshotResolverPath        ,
  localRequire              ,
)                            {
  const custom                   = _m3.interopRequireDefault(
    await localRequire(snapshotResolverPath),
  ).default;

  const keys                                          = [
    ['resolveSnapshotPath', 'function'],
    ['resolveTestPath', 'function'],
    ['testPathForConsistencyCheck', 'string'],
  ];
  for (const [propName, requiredType] of keys) {
    if (typeof custom[propName] !== requiredType) {
      throw new TypeError(mustImplement(propName, requiredType));
    }
  }

  const customResolver                   = {
    resolveSnapshotPath: (testPath        ) =>
      custom.resolveSnapshotPath(testPath, DOT_EXTENSION),
    resolveTestPath: (snapshotPath        ) =>
      custom.resolveTestPath(snapshotPath, DOT_EXTENSION),
    testPathForConsistencyCheck: custom.testPathForConsistencyCheck,
  };

  verifyConsistentTransformations(customResolver);

  return customResolver;
}

function mustImplement(propName        , requiredType        ) {
  return `${(_m1.default ?? _m1).bold(
    `Custom snapshot resolver must implement a \`${propName}\` as a ${requiredType}.`,
  )}\nDocumentation: https://jestjs.io/docs/configuration#snapshotresolver-string`;
}

function verifyConsistentTransformations(custom                  ) {
  const resolvedSnapshotPath = custom.resolveSnapshotPath(
    custom.testPathForConsistencyCheck,
  );
  const resolvedTestPath = custom.resolveTestPath(resolvedSnapshotPath);
  if (resolvedTestPath !== custom.testPathForConsistencyCheck) {
    throw new Error(
      (_m1.default ?? _m1).bold(
        `Custom snapshot resolver functions must transform paths consistently, i.e. expects resolveTestPath(resolveSnapshotPath('${custom.testPathForConsistencyCheck}')) === ${resolvedTestPath}`,
      ),
    );
  }
}
