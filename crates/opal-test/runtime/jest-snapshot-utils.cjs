// Ported from jest/packages/jest-snapshot/src/utils.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "addExtraLineBreaks", {enumerable: true, get: () => addExtraLineBreaks});
Object.defineProperty(exports, "removeExtraLineBreaks", {enumerable: true, get: () => removeExtraLineBreaks});
Object.defineProperty(exports, "removeLinesBeforeExternalMatcherTrap", {enumerable: true, get: () => removeLinesBeforeExternalMatcherTrap});
Object.defineProperty(exports, "serialize", {enumerable: true, get: () => serialize});
Object.defineProperty(exports, "minify", {enumerable: true, get: () => minify});
Object.defineProperty(exports, "deserializeString", {enumerable: true, get: () => deserializeString});
Object.defineProperty(exports, "deepMerge", {enumerable: true, get: () => deepMerge});
Object.defineProperty(exports, "processInlineSnapshotsWithBabel", {enumerable: true, get: () => processInlineSnapshotsWithBabel});
Object.defineProperty(exports, "processPrettierAst", {enumerable: true, get: () => processPrettierAst});
Object.defineProperty(exports, "groupSnapshotsByFile", {enumerable: true, get: () => groupSnapshotsByFile});
const _m0 = require("fs");
const _m1 = require("./jest-snapfile.cjs");
const _m2 = require("./format.cjs");
const _m3 = require("./jest-snapshot-plugins.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

                                                         
             
       
       
          
                  
                     
                      




                                            

function isObject(item         )          {
  return item != null && typeof item === 'object' && !Array.isArray(item);
}

// Add extra line breaks at beginning and end of multiline snapshot
// to make the content easier to read.
const addExtraLineBreaks = (string        )         =>
  string.includes('\n') ? `\n${string}\n` : string;

// Remove extra line breaks at beginning and end of multiline snapshot.
// Instead of trim, which can remove additional newlines or spaces
// at beginning or end of the content from a custom serializer.
const removeExtraLineBreaks = (string        )         =>
  string.length > 2 && string.startsWith('\n') && string.endsWith('\n')
    ? string.slice(1, -1)
    : string;

const removeLinesBeforeExternalMatcherTrap = (stack        )         => {
  const lines = stack.split('\n');

  for (let i = 0; i < lines.length; i += 1) {
    // It's a function name specified in `packages/expect/src/index.ts`
    // for external custom matchers.
    if (lines[i].includes('__EXTERNAL_MATCHER_TRAP__')) {
      return lines.slice(i + 1).join('\n');
    }
  }

  return stack;
};

const escapeRegex = true;
const printFunctionName = false;

const serialize = (
  val         ,
  indent = 2,
  formatOverrides                      = {},
)         =>
  _m1.normalizeNewlines(
    _m2.format(val, {
      escapeRegex,
      indent,
      plugins: _m3.getSerializers(),
      printFunctionName,
      ...formatOverrides,
    }),
  );

const minify = (val         )         =>
  _m2.format(val, {
    escapeRegex,
    min: true,
    plugins: _m3.getSerializers(),
    printFunctionName,
  });

// Remove double quote marks and unescape double quotes and backslashes.
const deserializeString = (stringified        )         =>
  stringified.slice(1, -1).replaceAll(/\\("|\\)/g, '$1');

const isAnyOrAnything = (input        ) =>
  '$$typeof' in input &&
  input.$$typeof === Symbol.for('jest.asymmetricMatcher') &&
  ['Any', 'Anything'].includes(input.constructor.name);

const deepMergeArray = (target            , source            ) => {
  const mergedOutput = [...target];

  for (const [index, sourceElement] of source.entries()) {
    const targetElement = mergedOutput[index];

    if (Array.isArray(target[index]) && Array.isArray(sourceElement)) {
      mergedOutput[index] = deepMergeArray(target[index], sourceElement);
    } else if (isObject(targetElement) && !isAnyOrAnything(sourceElement)) {
      mergedOutput[index] = deepMerge(target[index], sourceElement);
    } else {
      // Source does not exist in target or target is primitive and cannot be deep merged
      mergedOutput[index] = sourceElement;
    }
  }

  return mergedOutput;
};

// eslint-disable-next-line @typescript-eslint/explicit-module-boundary-types
const deepMerge = (target     , source     )      => {
  if (isObject(target) && isObject(source)) {
    const mergedOutput = {...target};

    for (const key of Object.keys(source)) {
      if (isObject(source[key]) && !source[key].$$typeof) {
        if (key in target) {
          mergedOutput[key] = deepMerge(target[key], source[key]);
        } else {
          Object.assign(mergedOutput, {[key]: source[key]});
        }
      } else if (Array.isArray(source[key])) {
        mergedOutput[key] = deepMergeArray(target[key], source[key]);
      } else {
        Object.assign(mergedOutput, {[key]: source[key]});
      }
    }

    return mergedOutput;
  } else if (Array.isArray(target) && Array.isArray(source)) {
    return deepMergeArray(target, source);
  }

  return target;
};

const indent = (
  snapshot        ,
  numIndents        ,
  indentation        ,
)         => {
  const lines = snapshot.split('\n');
  // Prevent re-indentation of inline snapshots.
  if (
    lines.length >= 2 &&
    lines[1].startsWith(indentation.repeat(numIndents + 1))
  ) {
    return snapshot;
  }

  return lines
    .map((line, index) => {
      if (index === 0) {
        // First line is either a 1-line snapshot or a blank line.
        return line;
      } else if (index === lines.length - 1) {
        // The last line should be placed on the same level as the expect call.
        return indentation.repeat(numIndents) + line;
      } else {
        // Do not indent empty lines.
        if (line === '') {
          return line;
        }

        // Not last line, indent one level deeper than expect call.
        return indentation.repeat(numIndents + 1) + line;
      }
    })
    .join('\n');
};

// Loading babel costs well over a hundred modules, and only writing inline
// snapshots needs it, so defer it until the first snapshot is processed.
function loadBabel() {
  const generate = (
    require(
      require.resolve('@babel/generator', {
        [Symbol.for('jest-resolve-outside-vm-option')]: true,
      }),
    )                                     
  ).default;

  const {parseSync, types} = require(
    require.resolve('@babel/core', {
      [Symbol.for('jest-resolve-outside-vm-option')]: true,
    }),
  )                                ;

  const {
    isAwaitExpression,
    templateElement,
    templateLiteral,
    traverseFast,
    traverse,
  } = types;

  return {
    generate,
    isAwaitExpression,
    parseSync,
    templateElement,
    templateLiteral,
    traverse,
    traverseFast,
  };
}

let babel                                          ;

function getBabel() {
  babel ??= loadBabel();
  return babel;
}

const processInlineSnapshotsWithBabel = (
  snapshots                       ,
  sourceFilePath        ,
  rootDir        ,
    
                                      
                     
                                  
) => {
  const {generate, parseSync} = getBabel();
  const sourceFile = _m0.readFileSync(sourceFilePath, 'utf8');

  // TypeScript projects may not have a babel config; make sure they can be parsed anyway.
  const presets = [require.resolve('babel-preset-current-node-syntax')];
  const plugins                    = [];
  if (/\.([cm]?ts|tsx)$/.test(sourceFilePath)) {
    plugins.push([
      require.resolve('@babel/plugin-syntax-typescript'),
      {isTSX: sourceFilePath.endsWith('x')},
      // unique name to make sure Babel does not complain about a possible duplicate plugin.
      'TypeScript syntax plugin added by Jest snapshot',
    ]);
  }

  // Record the matcher names seen during traversal and pass them down one
  // by one to formatting parser.
  const snapshotMatcherNames                = [];

  let ast                    ;

  try {
    ast = parseSync(sourceFile, {
      filename: sourceFilePath,
      plugins,
      presets,
      root: rootDir,
    });
  } catch (error     ) {
    // attempt to recover from missing jsx plugin
    if (error.message.includes('@babel/plugin-syntax-jsx')) {
      try {
        const jsxSyntaxPlugin             = [
          require.resolve('@babel/plugin-syntax-jsx'),
          {},
          // unique name to make sure Babel does not complain about a possible duplicate plugin.
          'JSX syntax plugin added by Jest snapshot',
        ];
        ast = parseSync(sourceFile, {
          filename: sourceFilePath,
          plugins: [...plugins, jsxSyntaxPlugin],
          presets,
          root: rootDir,
        });
      } catch {
        throw error;
      }
    } else {
      throw error;
    }
  }

  if (!ast) {
    throw new Error(`jest-snapshot: Failed to parse ${sourceFilePath}`);
  }
  traverseAst(snapshots, ast, snapshotMatcherNames);

  return {
    snapshotMatcherNames,
    sourceFile,
    // substitute in the snapshots in reverse order, so slice calculations aren't thrown off.
    sourceFileWithSnapshots: snapshots.reduceRight(
      (sourceSoFar, nextSnapshot) => {
        const {node} = nextSnapshot;
        if (
          !node ||
          typeof node.start !== 'number' ||
          typeof node.end !== 'number'
        ) {
          throw new Error('Jest: no snapshot insert location found');
        }

        // A hack to prevent unexpected line breaks in the generated code
        node.loc .end.line = node.loc .start.line;

        return (
          sourceSoFar.slice(0, node.start) +
          generate(node, {retainLines: true}).code.trim() +
          sourceSoFar.slice(node.end)
        );
      },
      sourceFile,
    ),
  };
};

const processPrettierAst = (
  ast      ,
  options                            ,
  snapshotMatcherNames               ,
  keepNode          ,
)       => {
  const {isAwaitExpression, templateElement, templateLiteral, traverse} =
    getBabel();
  traverse(ast, (node      , ancestors                    ) => {
    if (node.type !== 'CallExpression') return;

    const {arguments: args, callee} = node;
    if (
      callee.type !== 'MemberExpression' ||
      callee.property.type !== 'Identifier' ||
      !snapshotMatcherNames.includes(callee.property.name) ||
      !callee.loc ||
      callee.computed
    ) {
      return;
    }

    let snapshotIndex                    ;
    let snapshot                    ;
    for (const [i, node] of args.entries()) {
      if (node.type === 'TemplateLiteral') {
        snapshotIndex = i;
        snapshot = node.quasis[0].value.raw;
      }
    }
    if (snapshot === undefined) {
      return;
    }

    const parent = ancestors.at(-1) .node;
    const startColumn =
      isAwaitExpression(parent) && parent.loc
        ? parent.loc.start.column
        : callee.loc.start.column;

    const useSpaces = !options?.useTabs;
    snapshot = indent(
      snapshot,
      Math.ceil(
        useSpaces
          ? startColumn / (options?.tabWidth ?? 1)
          : // Each tab is 2 characters.
            startColumn / 2,
      ),
      useSpaces ? ' '.repeat(options?.tabWidth ?? 1) : '\t',
    );

    if (keepNode) {
      (args[snapshotIndex ]                   ).quasis[0].value.raw = snapshot;
    } else {
      const replacementNode = templateLiteral(
        [
          templateElement({
            raw: snapshot,
          }),
        ],
        [],
      );
      args[snapshotIndex ] = replacementNode;
    }
  });
};

const groupSnapshotsBy =
  (createKey                                            ) =>
  (snapshots                       ) =>
    snapshots.reduce                                       (
      (object, inlineSnapshot) => {
        const key = createKey(inlineSnapshot);
        return {
          ...object,
          [key]: [...(object[key] || []), inlineSnapshot],
        };
      },
      {},
    );

const groupSnapshotsByFrame = groupSnapshotsBy(({frame: {line, column}}) =>
  typeof line === 'number' && typeof column === 'number'
    ? `${line}:${column - 1}`
    : '',
);
const groupSnapshotsByFile = groupSnapshotsBy(({frame: {file}}) => file);

const traverseAst = (
  snapshots                       ,
  ast                ,
  snapshotMatcherNames               ,
) => {
  const {templateElement, templateLiteral, traverseFast} = getBabel();
  const groupedSnapshots = groupSnapshotsByFrame(snapshots);
  const remainingSnapshots = new Set(snapshots.map(({snapshot}) => snapshot));

  traverseFast(ast, (node      ) => {
    if (node.type !== 'CallExpression') return;

    const {arguments: args, callee} = node;
    if (
      callee.type !== 'MemberExpression' ||
      callee.property.type !== 'Identifier' ||
      callee.property.loc == null
    ) {
      return;
    }
    const {line, column} = callee.property.loc.start;
    const snapshotsForFrame = groupedSnapshots[`${line}:${column}`];
    if (!snapshotsForFrame) {
      return;
    }
    if (snapshotsForFrame.length > 1) {
      throw new Error(
        'Jest: Multiple inline snapshots for the same call are not supported.',
      );
    }
    const inlineSnapshot = snapshotsForFrame[0];
    inlineSnapshot.node = node;

    snapshotMatcherNames.push(callee.property.name);

    const snapshotIndex = args.findIndex(
      ({type}) => type === 'TemplateLiteral' || type === 'StringLiteral',
    );

    const {snapshot} = inlineSnapshot;
    remainingSnapshots.delete(snapshot);
    const replacementNode = templateLiteral(
      [templateElement({raw: _m1.escapeBacktickString(snapshot)})],
      [],
    );

    if (snapshotIndex === -1) {
      args.push(replacementNode);
    } else {
      args[snapshotIndex] = replacementNode;
    }
  });

  if (remainingSnapshots.size > 0) {
    throw new Error("Jest: Couldn't locate all inline snapshots.");
  }
};
