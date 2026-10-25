// `.each`, ported from jest/packages/jest-each/src (bind.ts, validation.ts,
// table/array.ts, table/template.ts, table/interpolation.ts): array tables
// with printf placeholders, arrays of objects with `$name`, and tagged
// template tables. Each row becomes one call to the function `.each` was
// bound to, with the formatted title. A malformed table is reported the way
// Jest reports it: as one failing test under the unformatted title.
'use strict';

const util = require('util');
const {format: pretty} = require('./format.cjs');
const {ErrorWithStack, convertDescriptorToString} = require('./errors.cjs');

const SUPPORTED_PLACEHOLDERS = /%[#Odfijops]/g;
const PRETTY_PLACEHOLDER = '%p';
const JSON_PLACEHOLDER = '%j';
const INDEX_PLACEHOLDER = '%#';
const NUMBER_PLACEHOLDER = '%$';
const PLACEHOLDER_PREFIX = '%';
const ESCAPED_PLACEHOLDER_PREFIX = '%%';
const JEST_EACH_PLACEHOLDER_ESCAPE = '@@__JEST_EACH_PLACEHOLDER_ESCAPE__@@';

function bind(cb, supportsDone = true, needsEachError = false) {
  const bindWrap = (table, ...taggedTemplateData) => {
    const errorWithStack = new ErrorWithStack(undefined, bindWrap);
    return function eachBind(title, test, timeout) {
      title = convertDescriptorToString(title);
      try {
        const tests = taggedTemplateData.length === 0
          ? buildArrayTests(title, table)
          : buildTemplateTests(title, table, taggedTemplateData);
        for (const row of tests) {
          if (needsEachError) {
            cb(row.title, applyArguments(supportsDone, row.arguments, test), timeout, errorWithStack);
          } else {
            cb(row.title, applyArguments(supportsDone, row.arguments, test), timeout);
          }
        }
        return;
      } catch (error) {
        const err = new Error(error.message);
        err.stack = errorWithStack.stack?.replace(/^Error: /s, `Error: ${error.message}`);
        return cb(title, () => {
          throw err;
        });
      }
    };
  };
  return bindWrap;
}

function buildArrayTests(title, table) {
  validateArrayTable(table);
  return convertArrayTable(title, table);
}

function buildTemplateTests(title, table, taggedTemplateData) {
  const headings = extractValidTemplateHeadings(table[0]).replaceAll(/\s/g, '').split('|');
  validateTemplateTableArguments(headings, taggedTemplateData);
  return convertTemplateTable(title, headings, taggedTemplateData);
}

const applyArguments = (supportsDone, params, test) =>
  supportsDone && params.length < test.length
    ? done => test(...params, done)
    : () => test(...params);

// --- validation ------------------------------------------------------------

function validateArrayTable(table) {
  if (!Array.isArray(table)) {
    throw new TypeError(
      '`.each` must be called with an Array or Tagged Template Literal.\n\n' +
        `Instead was called with: ${pretty(table, {maxDepth: 1, min: true})}\n`,
    );
  }
  if (table.raw !== undefined) {
    if (typeof table[0] === 'string' && table[0].trim() === '') {
      throw new Error('Error: `.each` called with an empty Tagged Template Literal of table data.\n');
    }
    throw new Error(
      'Error: `.each` called with a Tagged Template Literal with no data, remember to interpolate with ${expression} syntax.\n',
    );
  }
  if (table.length === 0) {
    throw new Error('Error: `.each` called with an empty Array of table data.\n');
  }
}

function validateTemplateTableArguments(headings, data) {
  const incompleteData = data.length % headings.length;
  const missingData = headings.length - incompleteData;
  if (incompleteData > 0) {
    throw new Error(
      `Not enough arguments supplied for given headings:\n${headings.join(' | ')}\n\n` +
        `Received:\n${pretty(data)}\n\n` +
        `Missing ${missingData.toString()} ${missingData === 1 ? 'argument' : 'arguments'}`,
    );
  }
}

const HEADINGS_FORMAT = /^\n\s*[^\s]+\s*(\|\s*[^\s]+\s*)*\n/g;

function extractValidTemplateHeadings(headings) {
  const matches = headings.match(HEADINGS_FORMAT);
  if (matches === null) {
    throw new Error(
      `Table headings do not conform to expected format:\n\nheading1 | headingN\n\nReceived:\n\n${pretty(headings)}`,
    );
  }
  return matches[0];
}

// --- array tables ----------------------------------------------------------

function convertArrayTable(title, arrayTable) {
  if (isTemplates(title, arrayTable)) {
    return arrayTable.map((template, index) => ({
      arguments: [template],
      title: interpolateVariables(title, template, index).replaceAll(
        ESCAPED_PLACEHOLDER_PREFIX,
        PLACEHOLDER_PREFIX,
      ),
    }));
  }
  return normaliseTable(arrayTable).map((row, index) => ({
    arguments: row,
    title: formatTitle(title, row, index),
  }));
}

const isTable = table => table.every(Array.isArray);

// `test()` on a global regexp moves its lastIndex, which Jest's own code
// relies on not mattering; a fresh regexp here keeps the call pure.
const hasPlaceholder = title => new RegExp(SUPPORTED_PLACEHOLDERS.source).test(title);

const isTemplates = (title, arrayTable) =>
  !hasPlaceholder(interpolateEscapedPlaceholders(title)) &&
  !isTable(arrayTable) &&
  arrayTable.every(col => col != null && typeof col === 'object');

const normaliseTable = table => (isTable(table) ? table : table.map(col => [col]));

function formatTitle(title, row, rowIndex) {
  return row
    .reduce(
      (formattedTitle, value) => {
        const [placeholder] = formattedTitle.match(SUPPORTED_PLACEHOLDERS) || [];
        const normalisedValue = typeof value === 'string'
          ? value.replaceAll(PLACEHOLDER_PREFIX, JEST_EACH_PLACEHOLDER_ESCAPE)
          : value;
        if (!placeholder) return formattedTitle;
        if (placeholder === PRETTY_PLACEHOLDER) {
          const prettyValue = pretty(normalisedValue, {maxDepth: 1, min: true});
          return formattedTitle.replace(PRETTY_PLACEHOLDER, () => prettyValue);
        }
        if (placeholder === JSON_PLACEHOLDER) {
          const json = stringifyJson(normalisedValue);
          return formattedTitle.replace(JSON_PLACEHOLDER, () => json);
        }
        return util.format(formattedTitle, normalisedValue);
      },
      interpolateTitleIndexAndNumber(interpolateEscapedPlaceholders(title), rowIndex),
    )
    .replaceAll(JEST_EACH_PLACEHOLDER_ESCAPE, PLACEHOLDER_PREFIX);
}

const interpolateEscapedPlaceholders = title =>
  title.replaceAll(ESCAPED_PLACEHOLDER_PREFIX, JEST_EACH_PLACEHOLDER_ESCAPE);

const interpolateTitleIndexAndNumber = (title, index) =>
  title.replace(INDEX_PLACEHOLDER, index.toString()).replace(NUMBER_PLACEHOLDER, (index + 1).toString());

function stringifyJson(value) {
  if (isCyclic(value)) return '[Circular]';
  return `${JSON.stringify(value, (_, entry) => (typeof entry === 'bigint' ? `${entry}n` : entry))}`;
}

function isCyclic(value, ancestors = []) {
  if (value === null || typeof value !== 'object') return false;
  if (ancestors.includes(value)) return true;
  const nested = [...ancestors, value];
  return Object.values(value).some(entry => isCyclic(entry, nested));
}

// --- template tables -------------------------------------------------------

function convertTemplateTable(title, headings, row) {
  const table = Array.from({length: row.length / headings.length}, (_, index) =>
    row.slice(index * headings.length, index * headings.length + headings.length),
  );
  const templates = table.map(values => Object.fromEntries(values.map((value, index) => [headings[index], value])));
  return templates.map((template, index) => ({
    arguments: [template],
    title: interpolateVariables(title, template, index),
  }));
}

const escapeStrForRegex = string => string.replaceAll(/[$()*+.?[\\\]^{|}]/g, '\\$&');

function interpolateVariables(title, template, index) {
  // Longest keys first, so an overlapping key like `a|b` wins over `a`.
  const keys = Object.keys(template)
    .filter(key => key.length > 0)
    .sort((a, b) => b.length - a.length);
  if (keys.length === 0) return title.replace('$#', `${index}`);
  const alternation = keys.map(escapeStrForRegex).join('|');
  return title
    .replaceAll(new RegExp(`\\$(${alternation})[.\\w]*`, 'g'), match => {
      const keyPath = match.slice(1).split('.');
      const value = getPath(template, keyPath);
      return isPrimitive(value) ? String(value) : pretty(value, {maxDepth: 1, min: true});
    })
    .replace('$#', `${index}`);
}

function getPath(template, [head, ...tail]) {
  if (template === null) return 'null';
  if (template === undefined) return 'undefined';
  if (!head || !Object.prototype.hasOwnProperty.call(template, head)) return template;
  return getPath(template[head], tail);
}

const isPrimitive = value => Object(value) !== value;

module.exports = {bind};
