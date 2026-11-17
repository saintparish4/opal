// Ported from jest/packages/jest-snapshot-utils/src/utils.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "SNAPSHOT_VERSION", {enumerable: true, get: () => SNAPSHOT_VERSION});
Object.defineProperty(exports, "SNAPSHOT_GUIDE_LINK", {enumerable: true, get: () => SNAPSHOT_GUIDE_LINK});
Object.defineProperty(exports, "SNAPSHOT_VERSION_WARNING", {enumerable: true, get: () => SNAPSHOT_VERSION_WARNING});
Object.defineProperty(exports, "testNameToKey", {enumerable: true, get: () => testNameToKey});
Object.defineProperty(exports, "keyToTestName", {enumerable: true, get: () => keyToTestName});
Object.defineProperty(exports, "getSnapshotData", {enumerable: true, get: () => getSnapshotData});
Object.defineProperty(exports, "escapeBacktickString", {enumerable: true, get: () => escapeBacktickString});
Object.defineProperty(exports, "ensureDirectoryExists", {enumerable: true, get: () => ensureDirectoryExists});
Object.defineProperty(exports, "normalizeNewlines", {enumerable: true, get: () => normalizeNewlines});
Object.defineProperty(exports, "saveSnapshotFile", {enumerable: true, get: () => saveSnapshotFile});
const _m0 = require("node:path");
const _m1 = require("./support.cjs");
const _m2 = require("fs");
const _m3 = require("./natural-compare.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */





                                        
                                          

const SNAPSHOT_VERSION = '1';
const SNAPSHOT_HEADER_REGEXP = /^\/\/ Jest Snapshot v(.+), (.+)$/m;
const SNAPSHOT_GUIDE_LINK = 'https://jestjs.io/docs/snapshot-testing';
const SNAPSHOT_VERSION_WARNING = (_m1.default ?? _m1).yellow(
  `${(_m1.default ?? _m1).bold('Warning')}: Before you upgrade snapshots, ` +
    'we recommend that you revert any local changes to tests or other code, ' +
    'to ensure that you do not store invalid state.',
);

const writeSnapshotVersion = () =>
  `// Jest Snapshot v${SNAPSHOT_VERSION}, ${SNAPSHOT_GUIDE_LINK}`;

const validateSnapshotHeader = (snapshotContents        ) => {
  const headerTest = SNAPSHOT_HEADER_REGEXP.exec(snapshotContents);
  const version = headerTest && headerTest[1];
  const guideLink = headerTest && headerTest[2];

  if (!version) {
    return new Error(
      (_m1.default ?? _m1).red(
        `${(_m1.default ?? _m1).bold('Outdated snapshot')}: No snapshot header found. ` +
          'Jest 19 introduced versioned snapshots to ensure all developers ' +
          'on a project are using the same version of Jest. ' +
          'Please update all snapshots during this upgrade of Jest.\n\n',
      ) + SNAPSHOT_VERSION_WARNING,
    );
  }

  if (version < SNAPSHOT_VERSION) {
    return new Error(
      // eslint-disable-next-line prefer-template
      (_m1.default ?? _m1).red(
        `${(_m1.default ?? _m1).red.bold('Outdated snapshot')}: The version of the snapshot ` +
          'file associated with this test is outdated. The snapshot file ' +
          'version ensures that all developers on a project are using ' +
          'the same version of Jest. ' +
          'Please update all snapshots during this upgrade of Jest.',
      ) +
        '\n\n' +
        `Expected: v${SNAPSHOT_VERSION}\n` +
        `Received: v${version}\n\n` +
        SNAPSHOT_VERSION_WARNING,
    );
  }

  if (version > SNAPSHOT_VERSION) {
    return new Error(
      // eslint-disable-next-line prefer-template
      (_m1.default ?? _m1).red(
        `${(_m1.default ?? _m1).red.bold('Outdated Jest version')}: The version of this ` +
          'snapshot file indicates that this project is meant to be used ' +
          'with a newer version of Jest. The snapshot file version ensures ' +
          'that all developers on a project are using the same version of ' +
          'Jest. Please update your version of Jest and re-run the tests.',
      ) +
        '\n\n' +
        `Expected: v${SNAPSHOT_VERSION}\n` +
        `Received: v${version}`,
    );
  }

  if (guideLink !== SNAPSHOT_GUIDE_LINK) {
    return new Error(
      // eslint-disable-next-line prefer-template
      (_m1.default ?? _m1).red(
        `${(_m1.default ?? _m1).red.bold(
          'Outdated guide link',
        )}: The snapshot guide link at the top of this snapshot is outdated. ` +
          'Please update all snapshots during this upgrade of Jest.',
      ) +
        '\n\n' +
        `Expected: ${SNAPSHOT_GUIDE_LINK}\n` +
        `Received: ${guideLink}`,
    );
  }

  return null;
};

const normalizeTestNameForKey = (testName        )         =>
  testName.replaceAll(/\r\n|\r|\n/g, match => {
    switch (match) {
      case '\r\n':
        return '\\r\\n';
      case '\r':
        return '\\r';
      case '\n':
        return '\\n';
      default:
        return match;
    }
  });

const denormalizeTestNameFromKey = (key        )         =>
  key.replaceAll(/\\r\\n|\\r|\\n/g, match => {
    switch (match) {
      case '\\r\\n':
        return '\r\n';
      case '\\r':
        return '\r';
      case '\\n':
        return '\n';
      default:
        return match;
    }
  });

const testNameToKey = (testName        , count        )         =>
  `${normalizeTestNameForKey(testName)} ${count}`;

const keyToTestName = (key        )         => {
  if (!/ \d+$/.test(key)) {
    throw new Error('Snapshot keys must end with a number.');
  }
  const testNameWithoutCount = key.replace(/ \d+$/, '');
  return denormalizeTestNameFromKey(testNameWithoutCount);
};

const getSnapshotData = (
  snapshotPath        ,
  update                            ,
    
                     
                 
) => {
  const data = Object.create(null);
  let snapshotContents = '';
  let dirty = false;

  if (_m2.existsSync(snapshotPath)) {
    try {
      snapshotContents = _m2.readFileSync(snapshotPath, 'utf8');
      // eslint-disable-next-line no-new-func
      const populate = new Function('exports', snapshotContents);
      populate(data);
    } catch {}
  }

  const validationResult = validateSnapshotHeader(snapshotContents);
  const isInvalid = snapshotContents && validationResult;

  if (update === 'none' && isInvalid) {
    throw validationResult;
  }

  if ((update === 'all' || update === 'new') && isInvalid) {
    dirty = true;
  }

  return {data, dirty};
};

const escapeBacktickString = (str        )         =>
  str.replaceAll(/`|\\|\${/g, '\\$&');

const printBacktickString = (str        )         =>
  `\`${escapeBacktickString(str)}\``;

const ensureDirectoryExists = (filePath        )       => {
  try {
    _m2.mkdirSync(_m0.dirname(filePath), {recursive: true});
  } catch {}
};

const normalizeNewlines = (string        )         =>
  string.replaceAll(/\r\n|\r/g, '\n');

const saveSnapshotFile = (
  snapshotData              ,
  snapshotPath        ,
)       => {
  const snapshots = Object.keys(snapshotData)
    .sort((_m3.default ?? _m3))
    .map(
      key =>
        `exports[${printBacktickString(key)}] = ${printBacktickString(
          normalizeNewlines(snapshotData[key]),
        )};`,
    );

  ensureDirectoryExists(snapshotPath);
  _m2.writeFileSync(
    snapshotPath,
    `${writeSnapshotVersion()}\n\n${snapshots.join('\n\n')}\n`,
  );
};
