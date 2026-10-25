// Ported from jest/packages/jest-snapshot/src/printSnapshot.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "getSnapshotColorForChalkInstance", {enumerable: true, get: () => getSnapshotColorForChalkInstance});
Object.defineProperty(exports, "getReceivedColorForChalkInstance", {enumerable: true, get: () => getReceivedColorForChalkInstance});
Object.defineProperty(exports, "aSnapshotColor", {enumerable: true, get: () => aSnapshotColor});
Object.defineProperty(exports, "bReceivedColor", {enumerable: true, get: () => bReceivedColor});
Object.defineProperty(exports, "noColor", {enumerable: true, get: () => noColor});
Object.defineProperty(exports, "HINT_ARG", {enumerable: true, get: () => HINT_ARG});
Object.defineProperty(exports, "SNAPSHOT_ARG", {enumerable: true, get: () => SNAPSHOT_ARG});
Object.defineProperty(exports, "PROPERTIES_ARG", {enumerable: true, get: () => PROPERTIES_ARG});
Object.defineProperty(exports, "matcherHintFromConfig", {enumerable: true, get: () => matcherHintFromConfig});
Object.defineProperty(exports, "printExpected", {enumerable: true, get: () => printExpected});
Object.defineProperty(exports, "printReceived", {enumerable: true, get: () => printReceived});
Object.defineProperty(exports, "printPropertiesAndReceived", {enumerable: true, get: () => printPropertiesAndReceived});
Object.defineProperty(exports, "printSnapshotAndReceived", {enumerable: true, get: () => printSnapshotAndReceived});
const _m0 = require("./support.cjs");
const _m1 = require("./jest-expect-utils.cjs");
const _m2 = require("./jest-get-type.cjs");
const _m3 = require("./jest-diff.cjs");
const _m4 = require("./jest-matcher-utils.cjs");
const _m5 = require("./format.cjs");
const _m6 = require("./jest-snapshot-colors.cjs");
const _m7 = require("./jest-snapshot-dedentLines.cjs");
const _m8 = require("./jest-snapshot-utils.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */









                                                                 


                         

const getSnapshotColorForChalkInstance = (
  chalkInstance       ,
)                   => {
  const level = chalkInstance.level;

  if (level === 3) {
    return chalkInstance
      .rgb(_m6.aForeground3[0], _m6.aForeground3[1], _m6.aForeground3[2])
      .bgRgb(_m6.aBackground3[0], _m6.aBackground3[1], _m6.aBackground3[2]);
  }

  if (level === 2) {
    return chalkInstance.ansi256(_m6.aForeground2).bgAnsi256(_m6.aBackground2);
  }

  return chalkInstance.magenta.bgYellowBright;
};

const getReceivedColorForChalkInstance = (
  chalkInstance       ,
)                   => {
  const level = chalkInstance.level;

  if (level === 3) {
    return chalkInstance
      .rgb(_m6.bForeground3[0], _m6.bForeground3[1], _m6.bForeground3[2])
      .bgRgb(_m6.bBackground3[0], _m6.bBackground3[1], _m6.bBackground3[2]);
  }

  if (level === 2) {
    return chalkInstance.ansi256(_m6.bForeground2).bgAnsi256(_m6.bBackground2);
  }

  return chalkInstance.cyan.bgWhiteBright; // also known as teal
};

const aSnapshotColor = getSnapshotColorForChalkInstance((_m0.default ?? _m0));
const bReceivedColor = getReceivedColorForChalkInstance((_m0.default ?? _m0));

const noColor = (string        )         => string;

const HINT_ARG = 'hint';
const SNAPSHOT_ARG = 'snapshot';
const PROPERTIES_ARG = 'properties';

const matcherHintFromConfig = (
  {
    context: {isNot, promise},
    hint,
    inlineSnapshot,
    matcherName,
    properties,
  }                     ,
  isUpdatable         ,
)         => {
  const options                     = {isNot, promise};
  if (isUpdatable) {
    options.receivedColor = bReceivedColor;
  }

  let expectedArgument = '';

  if (typeof properties === 'object') {
    expectedArgument = PROPERTIES_ARG;
    if (isUpdatable) {
      options.expectedColor = noColor;
    }

    if (typeof hint === 'string' && hint.length > 0) {
      options.secondArgument = HINT_ARG;
      options.secondArgumentColor = _m4.BOLD_WEIGHT;
    } else if (typeof inlineSnapshot === 'string') {
      options.secondArgument = SNAPSHOT_ARG;
      if (isUpdatable) {
        options.secondArgumentColor = aSnapshotColor;
      } else {
        options.secondArgumentColor = noColor;
      }
    }
  } else {
    if (typeof hint === 'string' && hint.length > 0) {
      expectedArgument = HINT_ARG;
      options.expectedColor = _m4.BOLD_WEIGHT;
    } else if (typeof inlineSnapshot === 'string') {
      expectedArgument = SNAPSHOT_ARG;
      if (isUpdatable) {
        options.expectedColor = aSnapshotColor;
      }
    }
  }

  return _m4.matcherHint(matcherName, undefined, expectedArgument, options);
};

// Given array of diffs, return string:
// * include common substrings
// * exclude change substrings which have opposite op
// * include change substrings which have argument op
//   with change color only if there is a common substring
const joinDiffs = (
  diffs             ,
  op        ,
  hasCommon         ,
)         =>
  diffs.reduce(
    (reduced        , diff      )         =>
      reduced +
      (diff[0] === _m3.DIFF_EQUAL
        ? diff[1]
        : diff[0] === op
          ? hasCommon
            ? _m4.INVERTED_COLOR(diff[1])
            : diff[1]
          : ''),
    '',
  );

const isLineDiffable = (received         )          => {
  const receivedType = _m2.getType(received);

  if (_m2.isPrimitive(received)) {
    return typeof received === 'string';
  }

  if (
    receivedType === 'date' ||
    receivedType === 'function' ||
    receivedType === 'regexp'
  ) {
    return false;
  }

  if (received instanceof Error) {
    return false;
  }

  if (
    receivedType === 'object' &&
    typeof (received       ).asymmetricMatch === 'function'
  ) {
    return false;
  }

  return true;
};

const printExpected = (val         )         =>
  _m4.EXPECTED_COLOR(_m8.minify(val));
const printReceived = (val         )         =>
  _m4.RECEIVED_COLOR(_m8.minify(val));

const printPropertiesAndReceived = (
  properties        ,
  received        ,
  expand         , // CLI options: true if `--expand` or false if `--no-expand`
)         => {
  const aAnnotation = 'Expected properties';
  const bAnnotation = 'Received value';

  if (isLineDiffable(properties) && isLineDiffable(received)) {
    const {replacedExpected, replacedReceived} =
      _m4.replaceMatchedToAsymmetricMatcher(properties, received, [], []);
    return _m3.diffLinesUnified(
      _m8.serialize(replacedExpected).split('\n'),
      _m8.serialize(_m1.getObjectSubset(replacedReceived, replacedExpected)).split(
        '\n',
      ),
      {
        aAnnotation,
        aColor: _m4.EXPECTED_COLOR,
        bAnnotation,
        bColor: _m4.RECEIVED_COLOR,
        changeLineTrailingSpaceColor: (_m0.default ?? _m0).bgYellow,
        commonLineTrailingSpaceColor: (_m0.default ?? _m0).bgYellow,
        emptyFirstOrLastLinePlaceholder: '↵', // U+21B5
        expand,
        includeChangeCounts: true,
      },
    );
  }

  const printLabel = _m4.getLabelPrinter(aAnnotation, bAnnotation);
  return `${printLabel(aAnnotation) + printExpected(properties)}\n${printLabel(
    bAnnotation,
  )}${printReceived(received)}`;
};

const MAX_DIFF_STRING_LENGTH = 20_000;

const printSnapshotAndReceived = (
  a        , // snapshot without extra line breaks
  b        , // received serialized but without extra line breaks
  received         ,
  expand         , // CLI options: true if `--expand` or false if `--no-expand`
  snapshotFormat                 ,
)         => {
  const aAnnotation = 'Snapshot';
  const bAnnotation = 'Received';
  const aColor = aSnapshotColor;
  const bColor = bReceivedColor;
  const options = {
    aAnnotation,
    aColor,
    bAnnotation,
    bColor,
    changeLineTrailingSpaceColor: noColor,
    commonLineTrailingSpaceColor: (_m0.default ?? _m0).bgYellow,
    emptyFirstOrLastLinePlaceholder: '↵', // U+21B5
    expand,
    includeChangeCounts: true,
  };

  if (typeof received === 'string') {
    if (
      a.length >= 2 &&
      a.startsWith('"') &&
      a.endsWith('"') &&
      b === _m5.format(received)
    ) {
      // If snapshot looks like default serialization of a string
      // and received is string which has default serialization.

      if (!a.includes('\n') && !b.includes('\n')) {
        // If neither string is multiline,
        // display as labels and quoted strings.
        let aQuoted = a;
        let bQuoted = b;

        if (
          a.length - 2 <= MAX_DIFF_STRING_LENGTH &&
          b.length - 2 <= MAX_DIFF_STRING_LENGTH
        ) {
          const diffs = _m3.diffStringsRaw(a.slice(1, -1), b.slice(1, -1), true);
          const hasCommon = diffs.some(diff => diff[0] === _m3.DIFF_EQUAL);
          aQuoted = `"${joinDiffs(diffs, _m3.DIFF_DELETE, hasCommon)}"`;
          bQuoted = `"${joinDiffs(diffs, _m3.DIFF_INSERT, hasCommon)}"`;
        }

        const printLabel = _m4.getLabelPrinter(aAnnotation, bAnnotation);
        return `${printLabel(aAnnotation) + aColor(aQuoted)}\n${printLabel(
          bAnnotation,
        )}${bColor(bQuoted)}`;
      }

      // Else either string is multiline, so display as unquoted strings.
      a = _m8.deserializeString(a); //  hypothetical expected string
      b = received; // not serialized
    }
    // Else expected had custom serialization or was not a string
    // or received has custom serialization.

    return a.length <= MAX_DIFF_STRING_LENGTH &&
      b.length <= MAX_DIFF_STRING_LENGTH
      ? _m3.diffStringsUnified(a, b, options)
      : _m3.diffLinesUnified(a.split('\n'), b.split('\n'), options);
  }

  if (isLineDiffable(received)) {
    const aLines2 = a.split('\n');
    const bLines2 = b.split('\n');

    // Fall through to fix a regression for custom serializers
    // like jest-snapshot-serializer-raw that ignore the indent option.
    const b0 = _m8.serialize(received, 0, snapshotFormat);
    if (b0 !== b) {
      const aLines0 = _m7.dedentLines(aLines2);

      if (aLines0 !== null) {
        // Compare lines without indentation.
        const bLines0 = b0.split('\n');

        return _m3.diffLinesUnified2(aLines2, bLines2, aLines0, bLines0, options);
      }
    }

    // Fall back because:
    // * props include a multiline string
    // * text has more than one adjacent line
    // * markup does not close
    return _m3.diffLinesUnified(aLines2, bLines2, options);
  }

  const printLabel = _m4.getLabelPrinter(aAnnotation, bAnnotation);
  return `${printLabel(aAnnotation) + aColor(a)}\n${printLabel(
    bAnnotation,
  )}${bColor(b)}`;
};
