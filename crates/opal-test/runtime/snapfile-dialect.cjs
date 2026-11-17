// Snapshot files in the test file's dialect. jest-snapshot reaches
// @jest/snapshot-utils through here: for Jest's dialect everything is Jest's
// own (jest-snapfile.cjs); for Vitest's, the header, the keys and the lack of
// a version check are Vitest's (packages/snapshot/src/port/utils.ts), so a
// Vitest project's committed snapshots read back and are written as Vitest
// writes them. port.mjs also patches jest-snapshot to ask here how a hint
// joins the test's name and whether a snapshot matches, the two places
// Vitest's port/state.ts and client.ts differ inside jest-snapshot itself.
'use strict';

const fs = require('fs');
const jest = require('./jest-snapfile.cjs');
const naturalCompare = require('./natural-compare.cjs');

const VITEST_HEADER = '// Vitest Snapshot v1, https://vitest.dev/guide/snapshot.html';

let dialect = 'jest';
const setDialect = value => {
  dialect = value;
};

const printBacktickString = str => `\`${jest.escapeBacktickString(str)}\``;

const vitestTestNameToKey = (testName, count) => `${testName} ${count}`;
const vitestKeyToTestName = key => {
  if (!/ \d+$/.test(key)) throw new Error('Snapshot keys must end with a number.');
  return key.replace(/ \d+$/, '');
};

function vitestGetSnapshotData(snapshotPath) {
  const data = Object.create(null);
  if (fs.existsSync(snapshotPath)) {
    try {
      // eslint-disable-next-line no-new-func
      new Function('exports', fs.readFileSync(snapshotPath, 'utf8'))(data);
    } catch {}
  }
  return {data, dirty: false};
}

function vitestSaveSnapshotFile(snapshotData, snapshotPath) {
  const snapshots = Object.keys(snapshotData)
    .sort(naturalCompare)
    .map(key => `exports[${printBacktickString(key)}] = ${printBacktickString(jest.normalizeNewlines(snapshotData[key]))};`);
  const content = `${VITEST_HEADER}\n\n${snapshots.join('\n\n')}\n`;
  if (fs.existsSync(snapshotPath) && fs.readFileSync(snapshotPath, 'utf8') === content) return;
  jest.ensureDirectoryExists(snapshotPath);
  fs.writeFileSync(snapshotPath, content);
}

module.exports = {
  ...jest,
  hintedTestName: (testName, hint) => (dialect === 'vitest' ? `${testName} > ${hint}` : `${testName}: ${hint}`),
  snapshotsMatch: (expected, received) =>
    dialect === 'vitest' ? expected?.trim() === received.trim() : expected === received,
  getSnapshotData: (...args) => (dialect === 'vitest' ? vitestGetSnapshotData(...args) : jest.getSnapshotData(...args)),
  keyToTestName: key => (dialect === 'vitest' ? vitestKeyToTestName(key) : jest.keyToTestName(key)),
  saveSnapshotFile: (...args) => (dialect === 'vitest' ? vitestSaveSnapshotFile(...args) : jest.saveSnapshotFile(...args)),
  setDialect,
  testNameToKey: (name, count) => (dialect === 'vitest' ? vitestTestNameToKey(name, count) : jest.testNameToKey(name, count)),
};
