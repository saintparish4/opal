// Ported from jest/packages/jest-matcher-utils/src/index.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "SERIALIZABLE_PROPERTIES", {enumerable: true, get: () => _m5.SERIALIZABLE_PROPERTIES});
Object.defineProperty(exports, "EXPECTED_COLOR", {enumerable: true, get: () => EXPECTED_COLOR});
Object.defineProperty(exports, "RECEIVED_COLOR", {enumerable: true, get: () => RECEIVED_COLOR});
Object.defineProperty(exports, "INVERTED_COLOR", {enumerable: true, get: () => INVERTED_COLOR});
Object.defineProperty(exports, "BOLD_WEIGHT", {enumerable: true, get: () => BOLD_WEIGHT});
Object.defineProperty(exports, "DIM_COLOR", {enumerable: true, get: () => DIM_COLOR});
Object.defineProperty(exports, "SUGGEST_TO_CONTAIN_EQUAL", {enumerable: true, get: () => SUGGEST_TO_CONTAIN_EQUAL});
Object.defineProperty(exports, "stringify", {enumerable: true, get: () => stringify});
Object.defineProperty(exports, "highlightTrailingWhitespace", {enumerable: true, get: () => highlightTrailingWhitespace});
Object.defineProperty(exports, "printReceived", {enumerable: true, get: () => printReceived});
Object.defineProperty(exports, "printExpected", {enumerable: true, get: () => printExpected});
Object.defineProperty(exports, "printWithType", {enumerable: true, get: () => printWithType});
Object.defineProperty(exports, "ensureNoExpected", {enumerable: true, get: () => ensureNoExpected});
Object.defineProperty(exports, "ensureActualIsNumber", {enumerable: true, get: () => ensureActualIsNumber});
Object.defineProperty(exports, "ensureExpectedIsNumber", {enumerable: true, get: () => ensureExpectedIsNumber});
Object.defineProperty(exports, "ensureNumbers", {enumerable: true, get: () => ensureNumbers});
Object.defineProperty(exports, "ensureExpectedIsNonNegativeInteger", {enumerable: true, get: () => ensureExpectedIsNonNegativeInteger});
Object.defineProperty(exports, "printDiffOrStringify", {enumerable: true, get: () => printDiffOrStringify});
Object.defineProperty(exports, "replaceMatchedToAsymmetricMatcher", {enumerable: true, get: () => replaceMatchedToAsymmetricMatcher});
Object.defineProperty(exports, "diff", {enumerable: true, get: () => diff});
Object.defineProperty(exports, "pluralize", {enumerable: true, get: () => pluralize});
Object.defineProperty(exports, "getLabelPrinter", {enumerable: true, get: () => getLabelPrinter});
Object.defineProperty(exports, "matcherErrorMessage", {enumerable: true, get: () => matcherErrorMessage});
Object.defineProperty(exports, "matcherHint", {enumerable: true, get: () => matcherHint});
const _m0 = require("./support.cjs");
const _m1 = require("./jest-get-type.cjs");
const _m2 = require("./jest-diff.cjs");
const _m3 = require("./format.cjs");
const _m4 = require("./jest-matcher-utils-Replaceable.cjs");
const _m5 = require("./jest-matcher-utils-deepCyclicCopyReplaceable.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */










const {
  AsymmetricMatcher,
  DOMCollection,
  DOMElement,
  Immutable,
  ReactElement,
  ReactTestComponent,
} = _m3.plugins;

const PLUGINS = [
  ReactTestComponent,
  ReactElement,
  DOMElement,
  DOMCollection,
  Immutable,
  AsymmetricMatcher,
];

                                                 // subset of Chalk type

                                  
                   
                                   
                               
                  
                   
                                   
                          
                                         
  

                                            

const EXPECTED_COLOR        = (_m0.default ?? _m0).green;
const RECEIVED_COLOR        = (_m0.default ?? _m0).red;
const INVERTED_COLOR        = (_m0.default ?? _m0).inverse;
const BOLD_WEIGHT        = (_m0.default ?? _m0).bold;
const DIM_COLOR        = (_m0.default ?? _m0).dim;

const MULTILINE_REGEXP = /\n/;
const SPACE_SYMBOL = '\u{00B7}'; // middle dot

const NUMBERS = [
  'zero',
  'one',
  'two',
  'three',
  'four',
  'five',
  'six',
  'seven',
  'eight',
  'nine',
  'ten',
  'eleven',
  'twelve',
  'thirteen',
];

const SUGGEST_TO_CONTAIN_EQUAL = (_m0.default ?? _m0).dim(
  'Looks like you wanted to test for object/array equality with the stricter `toContain` matcher. You probably need to use `toContainEqual` instead.',
);

const stringify = (
  object         ,
  maxDepth = 10,
  maxWidth = 10,
)         => {
  const MAX_LENGTH = 10_000;
  let result;

  try {
    result = _m3.format(object, {
      maxDepth,
      maxWidth,
      min: true,
      plugins: PLUGINS,
    });
  } catch {
    result = _m3.format(object, {
      callToJSON: false,
      maxDepth,
      maxWidth,
      min: true,
      plugins: PLUGINS,
    });
  }

  if (result.length >= MAX_LENGTH && maxDepth > 1) {
    return stringify(object, Math.floor(maxDepth / 2), maxWidth);
  } else if (result.length >= MAX_LENGTH && maxWidth > 1) {
    return stringify(object, maxDepth, Math.floor(maxWidth / 2));
  } else {
    return result;
  }
};

const highlightTrailingWhitespace = (text        )         =>
  text.replaceAll(/\s+$/gm, (_m0.default ?? _m0).inverse('$&'));

// Instead of inverse highlight which now implies a change,
// replace common spaces with middle dot at the end of any line.
const replaceTrailingSpaces = (text        )         =>
  text.replaceAll(/\s+$/gm, spaces => SPACE_SYMBOL.repeat(spaces.length));

const printReceived = (object         )         =>
  RECEIVED_COLOR(replaceTrailingSpaces(stringify(object)));
const printExpected = (value         )         =>
  EXPECTED_COLOR(replaceTrailingSpaces(stringify(value)));

function printWithType   (
  name        ,
  value   ,
  print                      ,
)         {
  const type = _m1.getType(value);
  const hasType =
    type !== 'null' && type !== 'undefined'
      ? `${name} has type:  ${type}\n`
      : '';
  const hasValue = `${name} has value: ${print(value)}`;
  return hasType + hasValue;
}

const ensureNoExpected = (
  expected         ,
  matcherName        ,
  options                     ,
)       => {
  if (expected !== undefined) {
    // Prepend maybe not only for backward compatibility.
    const matcherString = (options ? '' : '[.not]') + matcherName;
    throw new Error(
      matcherErrorMessage(
        matcherHint(matcherString, undefined, '', options),
        // Because expected is omitted in hint above,
        // expected is black instead of green in message below.
        'this matcher must not have an expected argument',
        printWithType('Expected', expected, printExpected),
      ),
    );
  }
};

/**
 * Ensures that `actual` is of type `number | bigint`
 */
const ensureActualIsNumber = (
  actual         ,
  matcherName        ,
  options                     ,
)       => {
  if (typeof actual !== 'number' && typeof actual !== 'bigint') {
    // Prepend maybe not only for backward compatibility.
    const matcherString = (options ? '' : '[.not]') + matcherName;
    throw new Error(
      matcherErrorMessage(
        matcherHint(matcherString, undefined, undefined, options),
        `${RECEIVED_COLOR('received')} value must be a number or bigint`,
        printWithType('Received', actual, printReceived),
      ),
    );
  }
};

/**
 * Ensures that `expected` is of type `number | bigint`
 */
const ensureExpectedIsNumber = (
  expected         ,
  matcherName        ,
  options                     ,
)       => {
  if (typeof expected !== 'number' && typeof expected !== 'bigint') {
    // Prepend maybe not only for backward compatibility.
    const matcherString = (options ? '' : '[.not]') + matcherName;
    throw new Error(
      matcherErrorMessage(
        matcherHint(matcherString, undefined, undefined, options),
        `${EXPECTED_COLOR('expected')} value must be a number or bigint`,
        printWithType('Expected', expected, printExpected),
      ),
    );
  }
};

/**
 * Ensures that `actual` & `expected` are of type `number | bigint`
 */
const ensureNumbers = (
  actual         ,
  expected         ,
  matcherName        ,
  options                     ,
)       => {
  ensureActualIsNumber(actual, matcherName, options);
  ensureExpectedIsNumber(expected, matcherName, options);
};

const ensureExpectedIsNonNegativeInteger = (
  expected         ,
  matcherName        ,
  options                     ,
)       => {
  if (
    typeof expected !== 'number' ||
    !Number.isSafeInteger(expected) ||
    expected < 0
  ) {
    // Prepend maybe not only for backward compatibility.
    const matcherString = (options ? '' : '[.not]') + matcherName;
    throw new Error(
      matcherErrorMessage(
        matcherHint(matcherString, undefined, undefined, options),
        `${EXPECTED_COLOR('expected')} value must be a non-negative integer`,
        printWithType('Expected', expected, printExpected),
      ),
    );
  }
};

// Given array of diffs, return concatenated string:
// * include common substrings
// * exclude change substrings which have opposite op
// * include change substrings which have argument op
//   with inverse highlight only if there is a common substring
const getCommonAndChangedSubstrings = (
  diffs             ,
  op        ,
  hasCommonDiff         ,
)         =>
  diffs.reduce(
    (reduced        , diff      )         =>
      reduced +
      (diff[0] === _m2.DIFF_EQUAL
        ? diff[1]
        : diff[0] === op
          ? hasCommonDiff
            ? INVERTED_COLOR(diff[1])
            : diff[1]
          : ''),
    '',
  );

const isLineDiffable = (expected         , received         )          => {
  const expectedType = _m1.getType(expected);
  const receivedType = _m1.getType(received);

  if (expectedType !== receivedType) {
    return false;
  }

  if (_m1.isPrimitive(expected)) {
    // Print generic line diff for strings only:
    // * if neither string is empty
    // * if either string has more than one line
    return (
      typeof expected === 'string' &&
      typeof received === 'string' &&
      expected.length > 0 &&
      received.length > 0 &&
      (MULTILINE_REGEXP.test(expected) || MULTILINE_REGEXP.test(received))
    );
  }

  if (
    expectedType === 'date' ||
    expectedType === 'function' ||
    expectedType === 'regexp'
  ) {
    return false;
  }

  if (expected instanceof Error && received instanceof Error) {
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

const MAX_DIFF_STRING_LENGTH = 20_000;

const printDiffOrStringify = (
  expected         ,
  received         ,
  expectedLabel        ,
  receivedLabel        ,
  expand         , // CLI options: true if `--expand` or false if `--no-expand`
)         => {
  if (
    typeof expected === 'string' &&
    typeof received === 'string' &&
    expected.length > 0 &&
    received.length > 0 &&
    expected.length <= MAX_DIFF_STRING_LENGTH &&
    received.length <= MAX_DIFF_STRING_LENGTH &&
    expected !== received
  ) {
    if (expected.includes('\n') || received.includes('\n')) {
      return _m2.diffStringsUnified(expected, received, {
        aAnnotation: expectedLabel,
        bAnnotation: receivedLabel,
        changeLineTrailingSpaceColor: (_m0.default ?? _m0).bgYellow,
        commonLineTrailingSpaceColor: (_m0.default ?? _m0).bgYellow,
        emptyFirstOrLastLinePlaceholder: '↵', // U+21B5
        expand,
        includeChangeCounts: true,
      });
    }

    const diffs = _m2.diffStringsRaw(expected, received, true);
    const hasCommonDiff = diffs.some(diff => diff[0] === _m2.DIFF_EQUAL);

    const printLabel = getLabelPrinter(expectedLabel, receivedLabel);
    const expectedLine =
      printLabel(expectedLabel) +
      printExpected(
        getCommonAndChangedSubstrings(diffs, _m2.DIFF_DELETE, hasCommonDiff),
      );
    const receivedLine =
      printLabel(receivedLabel) +
      printReceived(
        getCommonAndChangedSubstrings(diffs, _m2.DIFF_INSERT, hasCommonDiff),
      );

    return `${expectedLine}\n${receivedLine}`;
  }

  if (isLineDiffable(expected, received)) {
    const {replacedExpected, replacedReceived} =
      replaceMatchedToAsymmetricMatcher(expected, received, [], []);
    const difference = _m2.diff(replacedExpected, replacedReceived, {
      aAnnotation: expectedLabel,
      bAnnotation: receivedLabel,
      expand,
      includeChangeCounts: true,
    });

    if (
      typeof difference === 'string' &&
      difference.includes(`- ${expectedLabel}`) &&
      difference.includes(`+ ${receivedLabel}`)
    ) {
      return difference;
    }
  }

  const printLabel = getLabelPrinter(expectedLabel, receivedLabel);
  const expectedLine = printLabel(expectedLabel) + printExpected(expected);
  const receivedLine =
    printLabel(receivedLabel) +
    (stringify(expected) === stringify(received)
      ? 'serializes to the same string'
      : printReceived(received));

  return `${expectedLine}\n${receivedLine}`;
};

// Sometimes, e.g. when comparing two numbers, the output from jest-diff
// does not contain more information than the `Expected:` / `Received:` already gives.
// In those cases, we do not print a diff to make the output shorter and not redundant.
const shouldPrintDiff = (actual         , expected         ) => {
  if (typeof actual === 'number' && typeof expected === 'number') {
    return false;
  }
  if (typeof actual === 'bigint' && typeof expected === 'bigint') {
    return false;
  }
  if (typeof actual === 'boolean' && typeof expected === 'boolean') {
    return false;
  }
  return true;
};

function replaceMatchedToAsymmetricMatcher(
  replacedExpected         ,
  replacedReceived         ,
  expectedCycles                ,
  receivedCycles                ,
)                                                         {
  return _replaceMatchedToAsymmetricMatcher(
    (_m5.default ?? _m5)(replacedExpected),
    (_m5.default ?? _m5)(replacedReceived),
    expectedCycles,
    receivedCycles,
  );
}

function _replaceMatchedToAsymmetricMatcher(
  replacedExpected         ,
  replacedReceived         ,
  expectedCycles                ,
  receivedCycles                ,
) {
  if (!(_m4.default ?? _m4).isReplaceable(replacedExpected, replacedReceived)) {
    return {replacedExpected, replacedReceived};
  }

  if (
    expectedCycles.includes(replacedExpected) ||
    receivedCycles.includes(replacedReceived)
  ) {
    return {replacedExpected, replacedReceived};
  }

  expectedCycles.push(replacedExpected);
  receivedCycles.push(replacedReceived);

  const expectedReplaceable = new (_m4.default ?? _m4)(replacedExpected);
  const receivedReplaceable = new (_m4.default ?? _m4)(replacedReceived);

  // eslint-disable-next-line unicorn/no-array-for-each
  expectedReplaceable.forEach((expectedValue         , key         ) => {
    const receivedValue = receivedReplaceable.get(key);
    if (isAsymmetricMatcher(expectedValue)) {
      if (expectedValue.asymmetricMatch(receivedValue)) {
        receivedReplaceable.set(key, expectedValue);
      }
    } else if (isAsymmetricMatcher(receivedValue)) {
      if (receivedValue.asymmetricMatch(expectedValue)) {
        expectedReplaceable.set(key, receivedValue);
      }
    } else if ((_m4.default ?? _m4).isReplaceable(expectedValue, receivedValue)) {
      const replaced = _replaceMatchedToAsymmetricMatcher(
        expectedValue,
        receivedValue,
        expectedCycles,
        receivedCycles,
      );
      expectedReplaceable.set(key, replaced.replacedExpected);
      receivedReplaceable.set(key, replaced.replacedReceived);
    }
  });

  return {
    replacedExpected: expectedReplaceable.object,
    replacedReceived: receivedReplaceable.object,
  };
}

                          
                            
  

function isAsymmetricMatcher(data     )                            {
  const type = _m1.getType(data);
  return type === 'object' && typeof data.asymmetricMatch === 'function';
}

const diff = (
  a         ,
  b         ,
  options              ,
)                => (shouldPrintDiff(a, b) ? _m2.diff(a, b, options) : null);

const pluralize = (word        , count        )         =>
  `${NUMBERS[count] || count} ${word}${count === 1 ? '' : 's'}`;

// To display lines of labeled values as two columns with monospace alignment:
// given the strings which will describe the values,
// return function which given each string, returns the label:
// string, colon, space, and enough padding spaces to align the value.

                                             

const getLabelPrinter = (...strings               )             => {
  const maxLength = strings.reduce(
    (max, string) => Math.max(string.length, max),
    0,
  );
  return (string        )         =>
    `${string}: ${' '.repeat(maxLength - string.length)}`;
};

const matcherErrorMessage = (
  hint        , // assertion returned from call to matcherHint
  generic        , // condition which correct value must fulfill
  specific         , // incorrect value returned from call to printWithType
)         =>
  `${hint}\n\n${(_m0.default ?? _m0).bold('Matcher error')}: ${generic}${
    typeof specific === 'string' ? `\n\n${specific}` : ''
  }`;

// Display assertion for the report when a test fails.
// New format: rejects/resolves, not, and matcher name have black color
// Old format: matcher name has dim color
const matcherHint = (
  matcherName        ,
  received = 'received',
  expected = 'expected',
  options                     = {},
)         => {
  const {
    comment = '',
    expectedColor = EXPECTED_COLOR,
    isDirectExpectCall = false, // seems redundant with received === ''
    isNot = false,
    promise = '',
    receivedColor = RECEIVED_COLOR,
    secondArgument = '',
    secondArgumentColor = EXPECTED_COLOR,
  } = options;
  let hint = '';
  let dimString = 'expect'; // concatenate adjacent dim substrings

  if (!isDirectExpectCall && received !== '') {
    hint += DIM_COLOR(`${dimString}(`) + receivedColor(received);
    dimString = ')';
  }

  if (promise !== '') {
    hint += DIM_COLOR(`${dimString}.`) + promise;
    dimString = '';
  }

  if (isNot) {
    hint += `${DIM_COLOR(`${dimString}.`)}not`;
    dimString = '';
  }

  if (matcherName.includes('.')) {
    // Old format: for backward compatibility,
    // especially without promise or isNot options
    dimString += matcherName;
  } else {
    // New format: omit period from matcherName arg
    hint += DIM_COLOR(`${dimString}.`) + matcherName;
    dimString = '';
  }

  if (expected === '') {
    dimString += '()';
  } else {
    hint += DIM_COLOR(`${dimString}(`) + expectedColor(expected);
    if (secondArgument) {
      hint += DIM_COLOR(', ') + secondArgumentColor(secondArgument);
    }
    dimString = ')';
  }

  if (comment !== '') {
    dimString += ` // ${comment}`;
  }

  if (dimString !== '') {
    hint += DIM_COLOR(dimString);
  }

  return hint;
};
