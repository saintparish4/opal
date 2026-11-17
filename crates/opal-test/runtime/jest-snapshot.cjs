// Ported from jest/packages/jest-snapshot/src/index.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, and the PATCHES that port.mjs lists applied. Do not edit by
// hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "cleanup", {enumerable: true, get: () => cleanup});
Object.defineProperty(exports, "toMatchSnapshot", {enumerable: true, get: () => toMatchSnapshot});
Object.defineProperty(exports, "toMatchInlineSnapshot", {enumerable: true, get: () => toMatchInlineSnapshot});
Object.defineProperty(exports, "toThrowErrorMatchingSnapshot", {enumerable: true, get: () => toThrowErrorMatchingSnapshot});
Object.defineProperty(exports, "toThrowErrorMatchingInlineSnapshot", {enumerable: true, get: () => toThrowErrorMatchingInlineSnapshot});
const _m0 = require("fs");
const _m1 = require("./snapfile-dialect.cjs");
const _m2 = require("./jest-matcher-utils.cjs");
const _m3 = require("./support.cjs");
const _m4 = require("./jest-snapshot-SnapshotResolver.cjs");
const _m5 = require("./jest-snapshot-printSnapshot.cjs");
const _m6 = require("./jest-snapshot-utils.cjs");
const _m7 = require("./jest-snapshot-plugins.cjs");
const _m8 = require("./jest-snapshot-SnapshotResolver.cjs");
const _m9 = require("./jest-snapshot-State.cjs");
Object.defineProperty(exports, "addSerializer", {enumerable: true, get: () => _m7.addSerializer});
Object.defineProperty(exports, "getSerializers", {enumerable: true, get: () => _m7.getSerializers});
Object.defineProperty(exports, "EXTENSION", {enumerable: true, get: () => _m8.EXTENSION});
Object.defineProperty(exports, "buildSnapshotResolver", {enumerable: true, get: () => _m8.buildSnapshotResolver});
Object.defineProperty(exports, "isSnapshotPath", {enumerable: true, get: () => _m8.isSnapshotPath});
Object.defineProperty(exports, "SnapshotState", {enumerable: true, get: () => _m9.default});
/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */



                                        
                                                       




                                                                      




                                                         

                                                       

const DID_NOT_THROW = 'Received function did not throw'; // same as toThrow
const NOT_SNAPSHOT_MATCHERS = `Snapshot matchers cannot be used with ${_m2.BOLD_WEIGHT(
  'not',
)}`;

const INDENTATION_REGEX = /^([^\S\n]*)\S/m;

// Display name in report when matcher fails same as in snapshot file,
// but with optional hint argument in bold weight.
const printSnapshotName = (
  concatenatedBlockNames = '',
  hint = '',
  count        ,
)         => {
  const hasNames = concatenatedBlockNames.length > 0;
  const hasHint = hint.length > 0;

  return `Snapshot name: \`${
    hasNames ? _m1.escapeBacktickString(concatenatedBlockNames) : ''
  }${hasNames && hasHint ? ': ' : ''}${
    hasHint ? _m2.BOLD_WEIGHT(_m1.escapeBacktickString(hint)) : ''
  } ${count}\``;
};

function stripAddedIndentation(inlineSnapshot        ) {
  // Find indentation if exists.
  const match = inlineSnapshot.match(INDENTATION_REGEX);
  if (!match || !match[1]) {
    // No indentation.
    return inlineSnapshot;
  }

  const indentation = match[1];
  const lines = inlineSnapshot.split('\n');
  if (lines.length <= 2) {
    // Must be at least 3 lines.
    return inlineSnapshot;
  }

  if (lines[0].trim() !== '' || lines.at(-1) .trim() !== '') {
    // If not blank first and last lines, abort.
    return inlineSnapshot;
  }

  for (let i = 1; i < lines.length - 1; i++) {
    if (lines[i] !== '') {
      if (lines[i].indexOf(indentation) !== 0) {
        // All lines except first and last should either be blank or have the same
        // indent as the first line (or more). If this isn't the case we don't
        // want to touch the snapshot at all.
        return inlineSnapshot;
      }

      lines[i] = lines[i].slice(indentation.length);
    }
  }

  // Last line is a special case because it won't have the same indent as others
  // but may still have been given some indent to line up.
  lines[lines.length - 1] = '';

  // Return inline snapshot, now at indent 0.
  inlineSnapshot = lines.join('\n');
  return inlineSnapshot;
}

const fileExists = (filePath        , fileSystem            )          =>
  fileSystem.exists(filePath) || _m0.existsSync(filePath);

const cleanup = (
  fileSystem            ,
  update                            ,
  snapshotResolver                  ,
  testPathIgnorePatterns                                                 ,
    
                       
                                  
) => {
  const pattern = `\\.${_m4.EXTENSION}$`;
  const files = fileSystem.matchFiles(pattern);
  let testIgnorePatternsRegex                = null;
  if (testPathIgnorePatterns && testPathIgnorePatterns.length > 0) {
    testIgnorePatternsRegex = new RegExp(testPathIgnorePatterns.join('|'));
  }

  const list = files.filter(snapshotFile => {
    const testPath = snapshotResolver.resolveTestPath(snapshotFile);

    // ignore snapshots of ignored tests
    if (testIgnorePatternsRegex && testIgnorePatternsRegex.test(testPath)) {
      return false;
    }

    if (!fileExists(testPath, fileSystem)) {
      if (update === 'all') {
        _m0.unlinkSync(snapshotFile);
      }
      return true;
    }

    return false;
  });

  return {
    filesRemoved: list.length,
    filesRemovedList: list,
  };
};

const toMatchSnapshot                             
          
                                                     
  = function (received, propertiesOrHint, hint) {
  const matcherName = 'toMatchSnapshot';
  let properties;

  const length = arguments.length;
  if (length === 2 && typeof propertiesOrHint === 'string') {
    hint = propertiesOrHint;
  } else if (length >= 2) {
    if (typeof propertiesOrHint !== 'object' || propertiesOrHint === null) {
      const options                     = {
        isNot: this.isNot,
        promise: this.promise,
      };
      let printedWithType = _m2.printWithType(
        'Expected properties',
        propertiesOrHint,
        _m5.printExpected,
      );

      if (length === 3) {
        options.secondArgument = 'hint';
        options.secondArgumentColor = _m2.BOLD_WEIGHT;

        if (propertiesOrHint == null) {
          printedWithType +=
            "\n\nTo provide a hint without properties: toMatchSnapshot('hint')";
        }
      }

      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, _m5.PROPERTIES_ARG, options),
          `Expected ${_m2.EXPECTED_COLOR('properties')} must be an object`,
          printedWithType,
        ),
      );
    }

    // Future breaking change: Snapshot hint must be a string
    // if (arguments.length === 3 && typeof hint !== 'string') {}

    properties = propertiesOrHint;
  }

  return _toMatchSnapshot({
    context: this,
    hint,
    isInline: false,
    matcherName,
    properties,
    received,
  });
};

const toMatchInlineSnapshot                             
          
                                                                   
  = function (received, propertiesOrSnapshot, inlineSnapshot) {
  const matcherName = 'toMatchInlineSnapshot';
  let properties;

  const length = arguments.length;
  if (length === 2 && typeof propertiesOrSnapshot === 'string') {
    inlineSnapshot = propertiesOrSnapshot;
  } else if (length >= 2) {
    const options                     = {
      isNot: this.isNot,
      promise: this.promise,
    };
    if (length === 3) {
      options.secondArgument = _m5.SNAPSHOT_ARG;
      options.secondArgumentColor = _m5.noColor;
    }

    if (
      typeof propertiesOrSnapshot !== 'object' ||
      propertiesOrSnapshot === null
    ) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, _m5.PROPERTIES_ARG, options),
          `Expected ${_m2.EXPECTED_COLOR('properties')} must be an object`,
          _m2.printWithType(
            'Expected properties',
            propertiesOrSnapshot,
            _m5.printExpected,
          ),
        ),
      );
    }

    if (length === 3 && typeof inlineSnapshot !== 'string') {
      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, _m5.PROPERTIES_ARG, options),
          'Inline snapshot must be a string',
          _m2.printWithType('Inline snapshot', inlineSnapshot, _m6.serialize),
        ),
      );
    }

    properties = propertiesOrSnapshot;
  }

  return _toMatchSnapshot({
    context: this,
    inlineSnapshot:
      inlineSnapshot === undefined
        ? undefined
        : stripAddedIndentation(inlineSnapshot),
    isInline: true,
    matcherName,
    properties,
    received,
  });
};

const _toMatchSnapshot = (config                     ) => {
  const {context, hint, inlineSnapshot, isInline, matcherName, properties} =
    config;
  let {received} = config;

  /** If a test was ran with `test.failing`. Passed by Jest Circus. */
  const {testFailing = false} = context;

  if (!testFailing && context.dontThrow) {
    // Suppress errors while running tests
    context.dontThrow();
  }

  const {currentConcurrentTestName, currentTestIdentity, isNot, snapshotState} =
    context;
  const currentTestName =
    currentConcurrentTestName?.() ?? context.currentTestName;
  const testIdentity = currentTestIdentity?.();

  if (isNot) {
    throw new Error(
      _m2.matcherErrorMessage(
        _m5.matcherHintFromConfig(config, false),
        NOT_SNAPSHOT_MATCHERS,
      ),
    );
  }

  if (snapshotState == null) {
    // Because the state is the problem, this is not a matcher error.
    // Call generic stringify from jest-matcher-utils package
    // because uninitialized snapshot state does not need snapshot serializers.
    throw new Error(
      `${_m5.matcherHintFromConfig(config, false)}\n\n` +
        'Snapshot state must be initialized' +
        `\n\n${_m2.printWithType('Snapshot state', snapshotState, _m2.stringify)}`,
    );
  }

  const fullTestName =
    currentTestName && hint
      ? _m1.hintedTestName(currentTestName, hint)
      : currentTestName || ''; // future BREAKING change: || hint

  if (typeof properties === 'object') {
    if (typeof received !== 'object' || received === null) {
      throw new Error(
        _m2.matcherErrorMessage(
          _m5.matcherHintFromConfig(config, false),
          `${_m2.RECEIVED_COLOR(
            'received',
          )} value must be an object when the matcher has ${_m2.EXPECTED_COLOR(
            'properties',
          )}`,
          _m2.printWithType('Received', received, _m5.printReceived),
        ),
      );
    }

    const propertyPass = context.equals(received, properties, [
      context.utils.iterableEquality,
      context.utils.subsetEquality,
    ]);

    if (propertyPass) {
      received = _m6.deepMerge(received, properties);
    } else {
      const key = snapshotState.fail(
        fullTestName,
        received,
        undefined,
        testIdentity,
      );
      const matched = /(\d+)$/.exec(key);
      const count = matched === null ? 1 : Number(matched[1]);

      const message = () =>
        `${_m5.matcherHintFromConfig(config, false)}\n\n${printSnapshotName(
          currentTestName,
          hint,
          count,
        )}\n\n${_m5.printPropertiesAndReceived(
          properties,
          received,
          snapshotState.expand,
        )}`;

      const matcherResult = {
        message,
        name: matcherName,
        pass: false,
      };

      return isInline
        ? matcherResult
        : {...matcherResult, snapshotPath: snapshotState.snapshotPath};
    }
  }

  const result = snapshotState.match({
    error: context.error,
    inlineSnapshot,
    isInline,
    received,
    testFailing,
    testIdentity,
    testName: fullTestName,
  });
  const {actual, count, expected, pass} = result;

  if (pass) {
    return {message: () => '', pass: true};
  }

  const message =
    expected === undefined
      ? () =>
          `${_m5.matcherHintFromConfig(config, true)}\n\n${printSnapshotName(
            currentTestName,
            hint,
            count,
          )}\n\n` +
          `New snapshot was ${_m2.BOLD_WEIGHT('not written')}. The update flag ` +
          'must be explicitly passed to write a new snapshot.\n\n' +
          'This is likely because this test is run in a continuous integration ' +
          '(CI) environment in which snapshots are not written by default.\n\n' +
          `Received:${actual.includes('\n') ? '\n' : ' '}${_m5.bReceivedColor(
            actual,
          )}`
      : () =>
          `${_m5.matcherHintFromConfig(config, true)}\n\n${printSnapshotName(
            currentTestName,
            hint,
            count,
          )}\n\n${_m5.printSnapshotAndReceived(
            expected,
            actual,
            received,
            snapshotState.expand,
            snapshotState.snapshotFormat,
          )}`;

  // Passing the actual and expected objects so that a custom reporter
  // could access them, for example in order to display a custom visual diff,
  // or create a different error message
  const matcherResult = {
    actual,
    expected,
    message,
    name: matcherName,
    pass: false,
  };

  return isInline
    ? matcherResult
    : {...matcherResult, snapshotPath: snapshotState.snapshotPath};
};

const toThrowErrorMatchingSnapshot                             
          
                                        
  = function (received, hint, fromPromise) {
  const matcherName = 'toThrowErrorMatchingSnapshot';

  // Future breaking change: Snapshot hint must be a string
  // if (hint !== undefined && typeof hint !== string) {}

  return _toThrowErrorMatchingSnapshot(
    {
      context: this,
      hint,
      isInline: false,
      matcherName,
      received,
    },
    fromPromise,
  );
};

const toThrowErrorMatchingInlineSnapshot                             
          
                                                  
  = function (received, inlineSnapshot, fromPromise) {
  const matcherName = 'toThrowErrorMatchingInlineSnapshot';

  if (inlineSnapshot !== undefined && typeof inlineSnapshot !== 'string') {
    const options                     = {
      expectedColor: _m5.noColor,
      isNot: this.isNot,
      promise: this.promise,
    };

    throw new Error(
      _m2.matcherErrorMessage(
        _m2.matcherHint(matcherName, undefined, _m5.SNAPSHOT_ARG, options),
        'Inline snapshot must be a string',
        _m2.printWithType('Inline snapshot', inlineSnapshot, _m6.serialize),
      ),
    );
  }

  return _toThrowErrorMatchingSnapshot(
    {
      context: this,
      inlineSnapshot:
        inlineSnapshot === undefined
          ? undefined
          : stripAddedIndentation(inlineSnapshot),
      isInline: true,
      matcherName,
      received,
    },
    fromPromise,
  );
};

const _toThrowErrorMatchingSnapshot = (
  config                     ,
  fromPromise          ,
) => {
  const {context, hint, inlineSnapshot, isInline, matcherName, received} =
    config;

  context.dontThrow?.();

  const {isNot, promise} = context;

  if (!fromPromise) {
    if (typeof received !== 'function') {
      const options                     = {isNot, promise};

      throw new Error(
        _m2.matcherErrorMessage(
          _m2.matcherHint(matcherName, undefined, '', options),
          `${_m2.RECEIVED_COLOR('received')} value must be a function`,
          _m2.printWithType('Received', received, _m5.printReceived),
        ),
      );
    }
  }

  if (isNot) {
    throw new Error(
      _m2.matcherErrorMessage(
        _m5.matcherHintFromConfig(config, false),
        NOT_SNAPSHOT_MATCHERS,
      ),
    );
  }

  let error;

  if (fromPromise) {
    error = received;
  } else {
    try {
      received();
    } catch (receivedError) {
      error = receivedError;
    }
  }

  if (error === undefined) {
    // Because the received value is a function, this is not a matcher error.
    throw new Error(
      `${_m5.matcherHintFromConfig(config, false)}\n\n${DID_NOT_THROW}`,
    );
  }

  let message = error.message;
  while ('cause' in error) {
    error = error.cause;
    if (_m3.isError(error) || error instanceof Error) {
      message += `\nCause: ${error.message}`;
    } else {
      if (typeof error === 'string') {
        message += `\nCause: ${error}`;
      }
      break;
    }
  }

  return _toMatchSnapshot({
    context,
    hint,
    inlineSnapshot,
    isInline,
    matcherName,
    received: message,
  });
};
