# The bundle contract

This file says what `opal build` promises, and what it refuses. It was
written before the bundler, and the bundler is held to it: every rule here
has a test that runs both programs under Node and compares them, or a
refusal with a code listed at the end.

- [What correct means](#what-correct-means)
- [The reference](#the-reference)
- [What is observed](#what-is-observed)
- [Rules](#rules)
- [Refusals](#refusals)
- [Divergences no build can detect](#divergences-no-build-can-detect)

## What correct means

A bundle is correct when, run under Node, it is observationally
indistinguishable from the unbundled program it was built from.

The unbundled program is the entry file and every module it reaches, run
in place by `node <entry>`. The bundle is one ES module, run by
`node <outdir>/<name>.mjs` from the same checkout. "Observationally" is
defined by the list in [What is observed](#what-is-observed): anything on
that list must match, and anything off it is named there and may differ.

When the bundler cannot keep a rule, it stops and prints a diagnostic that
names the module, the line, and the rule. It never writes a bundle that
might behave differently. The [refusals](#refusals) are part of the
contract, not gaps in it.

## The reference

- **Language**: ECMA-262, 2025 edition. Section numbers below are that
  edition's; the anchor names (`#sec-…`) are stable across editions.
- **Host**: Node.js 24, checked against 24.19.0. Paths to Node's source
  below are at the tag `v24.19.0`.
- **Resolution**: opal-core's module graph. The bundler never resolves an
  import itself. For `--target node` it asks for the conditions Node 24
  sets by default, `node`, `import` or `require`, `module-sync`, and
  `node-addons` (`lib/internal/modules/esm/utils.js:74-87`). For
  `--target browser` it asks for `browser`, `import` or `require`, and
  `default`.
- **TypeScript**: what `tsc` emits for the `compilerOptions` listed in
  [rule 13](#13-typescript-and-jsx), with `isolatedModules` semantics.

`--target browser` keeps every rule below except the ones that name a Node
API. It has no reference runtime to compare against, so its oracle is the
same program run under Node with the `browser` condition, plus a page load
in Chromium for the compatibility matrix.

## What is observed

These must be identical between the program and its bundle:

1. Bytes written to stdout and to stderr by the program, in order.
2. The exit code.
3. For an uncaught exception or unhandled rejection: the thrown value's
   constructor name and its `message`.
4. The order of every synchronous effect, and the order of promise jobs
   relative to each other, except where [rule 10](#10-dynamic-import) says
   otherwise.
5. Every value a program can compute about its own modules: binding
   values, `typeof`, identity (`===`) of module objects, namespace objects,
   `module.exports`, JSON modules, function and class `.name`, the keys and
   descriptors of namespace objects, `import.meta.url`, `import.meta.main`,
   `__filename`, and `__dirname`.

These are not observed, and may differ:

- `Function.prototype.toString` output. TypeScript and JSX are compiled,
  so source text cannot survive.
- `Error.prototype.stack`, and the file paths and line numbers in it. The
  source map gives them back under `node --enable-source-maps`.
- `process.argv[1]`, which names the file Node was started with.
- How `util.inspect` and `console.log` print a module namespace object,
  and `util.types.isModuleNamespaceObject`. See
  [Divergences no build can detect](#divergences-no-build-can-detect).
- Timing, memory, and the count of microtask turns before a dynamic
  import settles ([rule 10](#10-dynamic-import)).
- Text of an error message that names the importing file's path, such as
  `ERR_MODULE_NOT_FOUND`'s "imported from …". The error's class and code
  are still observed.

## Rules

### 1. Module format

A file is ESM or CommonJS by Node's rules: `.mjs`/`.mts` are ESM,
`.cjs`/`.cts` are CommonJS, and `.js`/`.ts`/extensionless files take the
`type` of the nearest `package.json`. When that `package.json` has no
`type`, or there is none, Node 24 detects the format from the source: a
file that fails to compile as CommonJS because of ESM-only syntax (an
`import` or `export` declaration, `import.meta`, or top-level `await`) is
loaded as ESM (`lib/internal/modules/cjs/loader.js:1792-1802`,
`containsModuleSyntax`). The bundler applies the same detection to such
files, and only to them. An explicit `"type": "commonjs"` turns detection
off, as it does in Node.

A byte-order mark at the start of a file is ignored
(`lib/internal/modules/helpers.js:214`, `stripBOM`).

### 2. ESM evaluation order

The bundle evaluates modules in the order ECMA-262 does
(§16.2.1.6.1.3.1 InnerModuleEvaluation, `#sec-innermoduleevaluation`): a
depth-first walk from the entry over each module's `[[RequestedModules]]`
in source order, each module's body running after all its requested
modules have run or are already on the stack. A module requested twice,
by the same or another specifier, runs once.

`[[RequestedModules]]` are the `import` and `export … from` declarations,
in the order they appear in the source (§16.2.1.7 Source Text Module
Records, `ModuleRequests`), and nothing else: a `require()` or `import()`
runs its target when the call runs, not before.

A cycle runs in the same order: the module whose import closes the cycle
sees its partner's bindings before that partner's body has run. Function
declarations in every module are initialized before any module body runs
(§16.2.1.7.3.1 InitializeEnvironment), so a cycle may call them early;
`let`, `const`, and `class` bindings are in their temporal dead zone until
their declaration runs (§9.1.1.1.6 GetBindingValue), and reading one early
throws `ReferenceError: Cannot access 'x' before initialization`, naming
the binding as it was written.

If a module's evaluation throws, nothing after it in the order runs, and
the error is the uncaught error of the program.

### 3. Bindings

- **Live.** An imported binding reads the exporting module's binding at the
  moment of the read, not a copy (§9.1.1.5.5 CreateImportBinding).
- **Immutable from the importer.** Assigning to an imported binding throws
  `TypeError: Assignment to constant variable.` after the right-hand side
  has been evaluated (§6.2.5.6 PutValue, on an immutable import binding).
- **Names.** A binding keeps the name it was written with wherever the name
  is observable: `.name` of a function or class, including names given by
  `NamedEvaluation` to anonymous functions and classes (§10.2.9
  SetFunctionName), and the binding named in a TDZ `ReferenceError`. When
  two modules declare the same top-level name, the bundler renames one of
  them in the output and preserves every observable name of the renamed
  binding.
- **Globals.** A module that reads a global (`process`, `console`, a name
  nothing declares) reads the same global in the bundle. No binding of one
  module, and no helper of the bundler's, may shadow a global another
  module reads.
- **Calls.** Calling an imported function passes `this` as `undefined`, as
  a call through an import binding does in the original.

### 4. Module namespace objects

`import * as ns`, `export * as ns from`, and the value a dynamic import
resolves to are namespace objects (§10.4.6 Module Namespace Exotic
Objects). In the bundle each is one object per module, created once, and
it has the internal methods §10.4.6 gives it:

- its prototype is `null`, it is not extensible, and setting its prototype
  to anything but `null` fails;
- `[[OwnPropertyKeys]]` is the export names sorted by code unit, then
  `@@toStringTag` (§10.4.6.11);
- each export reads through to the live binding, including the TDZ throw
  (§10.4.6.8 `[[Get]]`), and reports a descriptor of `{ value, writable:
  true, enumerable: true, configurable: false }`;
- `[[Set]]` and `[[Delete]]` of an export fail, so they throw in strict
  code;
- `@@toStringTag` is `"Module"`.

Export names come from §16.2.1.7.2.1 GetExportedNames and §16.2.1.7.2.2
ResolveExport: `export *` skips `default`, and a name two `export *`
declarations provide differently is ambiguous, so it is left out of the
namespace. Importing an ambiguous or missing name is a link error in Node,
a `SyntaxError` before any code runs, so the bundler refuses it
([`E-LINK`](#refusals)).

### 5. `this`, `arguments`, and strictness

- At the top level of an ES module, `this` is `undefined` (§9.1.1.5.4
  GetThisBinding), and `arguments` is not defined.
- ES modules are strict. CommonJS modules are strict only if their source
  says `"use strict"`; a sloppy CommonJS module stays sloppy in the bundle
  ([rule 6](#6-commonjs-modules)).

### 6. CommonJS modules

A CommonJS module runs inside the function Node wraps it in, with the
parameters `exports, require, module, __filename, __dirname`, called with
`this` set to `module.exports` (`lib/internal/modules/cjs/loader.js:1862-
1872`). In the bundle:

- **Its body is not rewritten.** The bundler does not rename, hoist, or
  reorder anything inside a CommonJS module. TypeScript and JSX are
  compiled and `--define` is applied; nothing else changes.
- **Strictness is kept.** A module that begins with a `"use strict"`
  directive is emitted as a function in the bundle. One that does not is
  emitted as source text and compiled at run time by `vm.compileFunction`
  (`--target node`) or `Function` (`--target browser`), which is how Node
  compiles it, so sloppy-mode semantics (`this` in plain calls, silent
  failed assignments, `with`, octal literals, mapped `arguments`) are
  unchanged. Under `--target node` it is compiled with its original file
  name, so its stack frames name the original file.
- **It runs once,** on first `require()` or at its place in the ESM order,
  whichever comes first. A `require()` in a cycle returns the partly-filled
  `module.exports`, as in Node. A module whose evaluation throws is
  removed from the cache, so the next `require()` runs it again
  (`lib/internal/modules/cjs/loader.js:1394-1400`).
- **`require(specifier)`** returns the bundled module when the graph
  resolved that literal specifier from this module. Any other argument,
  including a specifier the graph could not resolve, is resolved when the
  call runs, by Node's own resolver from the original file's location
  (`module.createRequire`). If the result is a file in the bundle, the
  bundled instance is returned; otherwise Node loads it, and a missing
  module throws `MODULE_NOT_FOUND` as it would have. Under `--target
  browser`, a specifier the graph did not resolve throws `MODULE_NOT_FOUND`.
- **`require.resolve`** is Node's, from the original file's location.
- **`require.main`** is the entry's `module` object when the entry is a
  CommonJS module, and `undefined` when it is an ES module
  (`lib/internal/modules/helpers.js:197`).
- **`module`** has `id` (`"."` for the entry, else the file name),
  `filename`, `path`, `loaded`, `exports`, `children`, `paths`, and
  `require`. `module.parent` is `undefined` when an ES module loaded it.
  It is a plain object, not an instance of `node:module`'s `Module`.
- **`__filename` and `__dirname`** are the original file's path and
  directory, computed at run time from the bundle's own location, so they
  match the unbundled program wherever the checkout is.

A CommonJS module cannot `require()` an ES module in the bundle
([`E-REQUIRE-ESM`](#refusals)).

### 7. An ES module importing CommonJS

Node turns a CommonJS module into a module record whose exports are
fixed after it runs (`lib/internal/modules/esm/translators.js:203-275`,
`createCJSModuleWrap`). The bundle does the same:

- **`default`** and **`"module.exports"`** are `module.exports`, whatever
  `__esModule` says.
- **Named exports** are the names Node's lexer finds in the source without
  running it, plus, transitively, those of CommonJS modules it re-exports
  with `module.exports = require(…)` and the patterns the lexer recognizes
  (`translators.js:420-470`, `cjsPreparseModuleExports`). The lexer is
  merve 1.2.2 (`deps/merve`); opal carries a port of it and checks the port
  against Node's own binding.
- **Values are a snapshot.** After the module runs, each named export is
  read once: a name the object does not own reads `undefined`, and a
  getter that throws reads `undefined` (`translators.js:258-271`). Later
  changes to `module.exports` are not seen through named imports or the
  namespace, only through `default`.
- Importing a name the lexer did not find is Node's link-time
  `SyntaxError: Named export 'x' not found`, so the bundler refuses it
  ([`E-LINK`](#refusals)).

### 8. JSON

- `require('./x.json')` parses the file once and caches the object
  (`lib/internal/modules/cjs/loader.js:2011-2015`).
- An ES module imports JSON only with `with { type: 'json' }`; without it
  Node refuses to link, and so does the bundler ([`E-LINK`](#refusals)). The
  namespace has only `default`.
- `require()` and `import` of the same file share one object, in either
  order (`translators.js:484-520`).
- The text is parsed by `JSON.parse`, so a `"__proto__"` key is an own
  property, as in Node.

### 9. `import.meta`

- `import.meta.url`, `.filename`, and `.dirname` are the original module's,
  computed at run time from the bundle's location.
- `import.meta.main` is the bundle's own `import.meta.main` in the entry
  module, and `false` in every other module.
- `import.meta.resolve(specifier)` with a relative string literal is the
  URL resolved against the original module. Any other use of
  `import.meta.resolve`, and any use of `import.meta` other than reading one
  of these properties, is refused ([`E-IMPORT-META`](#refusals)).

Under `--target browser`, only `import.meta.url` is kept, and the others
are refused.

### 10. Dynamic import

`import(specifier)` with a string literal that the graph resolved to a
bundled module returns a promise for its namespace object. The module, and
any of its static dependencies not yet run, run once, after the current
synchronous code and before the promise settles. Their order is rule 2's
order, starting from the imported module. A module that is only reached
through dynamic imports is run lazily, by a function in the bundle, and
keeps rules 2 to 4 exactly, TDZ included.

If such a module's evaluation throws, the promise rejects with that error,
and later imports of it, or of a module in the same cycle that was still
on the stack, reject with the same error (§16.2.1.6.1.3.1).

Node settles a dynamic import's promise some number of microtask turns
after the call, and that number depends on its loader, not on the
language. The bundle settles it after the current synchronous code, in a
later microtask turn; how many turns later is not observed.

`import()` of an external or builtin module stays an `import()` in the
bundle. Any other `import()` argument is refused ([`E-DYNAMIC`](#refusals)).

### 11. Top-level await

The entry module may use top-level `await`, unless it is in a cycle: no
module runs after the entry, so suspending it changes no order. Top-level
`await` anywhere else is refused for now ([`E-TLA`](#refusals)): ECMA-262
lets sibling modules run while an async module is suspended
(§16.2.1.6.1.3.2 ExecuteAsyncModule, §16.2.1.6.1.3.3
GatherAvailableAncestors), and a concatenated bundle cannot interleave
them without a scheduler this version does not have.

### 12. External and builtin modules

- A Node builtin (`node:fs`, `fs`) is always external. Under `--target
  browser` importing one is refused ([`E-BUILTIN`](#refusals)).
- `--external <name>` keeps a package, or a path, out of the bundle. ESM
  `import`s of external and builtin modules are hoisted to the top of the
  bundle, so they run before every bundled module. For a builtin that is
  exact, since builtins are loaded before any user code. For anything
  else, naming it external is the user's statement that its evaluation
  order does not matter.
- An external that an ES module imports is resolved by Node from the
  bundle's location; a `require()` of one is resolved from the original
  module's location ([rule 6](#6-commonjs-modules)).

### 13. TypeScript and JSX

TypeScript is compiled as `tsc` compiles it with `isolatedModules`, for
these `compilerOptions`, read from the `tsconfig.json` nearest each file
(following `extends`):

| Option | Effect |
|---|---|
| `verbatimModuleSyntax` | `false` (the default): an import used only as a type is removed, so its module does not run; `true`: only `import type` is removed. |
| `experimentalDecorators`, `emitDecoratorMetadata` | Legacy decorators and their `design:*` metadata, as `tsc` emits them. Without `experimentalDecorators`, a decorator is refused ([`E-TS`](#refusals)). |
| `useDefineForClassFields` | Class fields are defined (`true`) or assigned in the constructor (`false`); the default follows `target` as `tsc`'s does. |
| `jsx`, `jsxImportSource`, `jsxFactory`, `jsxFragmentFactory` | `react-jsx` (automatic runtime, the default when unset) or `react` (classic). `preserve` is refused. |

`const enum` is compiled like `enum`, as `isolatedModules` requires. An
`export =` or `import x = require()` in an ES module is refused.

### 14. `--define`

`--define <expr>=<value>` replaces each read of the global property path
`<expr>` (for example `process.env.NODE_ENV`) with the JavaScript
expression `<value>`, in every module. A path whose root is a local binding
is not replaced. Defining a value is the user's statement that the
expression has that value when the program runs: the bundle is equivalent
to the original program run where it does. Assigning to a defined path is
refused ([`E-DEFINE`](#refusals)).

### 15. Tree shaking

A top-level declaration in an ES module is left out of the bundle only
when nothing reads it, it is not exported to anything that reads it, and
evaluating it cannot have an effect. That is: function declarations;
`let`, `const`, and `var` whose initializers are literals, functions,
arrow functions, or side-effect-free classes; and classes with no
`extends`, decorators, computed keys, static blocks, or static
initializers that could have an effect. An identifier read is not
side-effect free, since it can throw in the TDZ.

A module whose package declares `"sideEffects": false`, or whose path does
not match the globs in its `sideEffects` array, is left out entirely when
nothing reads its exports. That is the package author's statement about
the package, not something the bundler checked, and it is the one place a
bundle may diverge from the program: when the statement is false. Which
packages in the compatibility matrix make that statement, and whether it
held, is recorded in `compat/COMPATIBILITY.md`.

### 16. Determinism

The bundle and its source map are a function of the graph snapshot, the
options, and the opal version, and nothing else: not the absolute path of
the checkout, the time, the machine, or the order the filesystem lists
directories. Paths in the output are relative to the out-dir.

## Refusals

Each refusal stops the build, prints the code, and names the module and
line.

| Code | What | Why |
|---|---|---|
| `E-PARSE` | A module does not parse. | Node would throw a `SyntaxError` before running anything. |
| `E-LINK` | An import names a missing or ambiguous export, a CommonJS name the lexer does not find, or JSON without `type: 'json'`; or a static import cannot be resolved. | Node fails to link the graph, before any code runs. |
| `E-TLA` | Top-level `await` outside the entry module, or in an entry that is in a cycle. | Rule 11. |
| `E-REQUIRE-ESM` | A CommonJS module `require()`s an ES module. | Node 24 runs it with its own cycle and async-module errors (`ERR_REQUIRE_CYCLE_MODULE`, `ERR_REQUIRE_ASYNC_MODULE`) that this version does not reproduce. |
| `E-DYNAMIC` | `import()` of anything but a string literal resolved by the graph or naming an external or builtin; or `import()` inside a sloppy CommonJS module. | Rule 10. The second needs the module's own loader, which a runtime-compiled function does not have. |
| `E-IMPORT-META` | A use of `import.meta` rule 9 does not list. | Rule 9. |
| `E-EVAL` | Direct `eval` in an ES module. | `eval` can name any binding by its original name, and the bundle renames bindings. |
| `E-CJS-API` | `require.cache`, `require.extensions`, or `module.constructor` in a CommonJS module. | They expose Node's module system itself, which the bundle replaces. |
| `E-BUILTIN` | A Node builtin under `--target browser`. | There is none in a browser. |
| `E-BROWSER-FIELD` | Under `--target browser`, a package with a `browser` field in its `package.json`. | opal-core does not apply that field, so the bundle would contain different files than a browser bundler picks. |
| `E-TS` | TypeScript that `tsc` would not emit under `isolatedModules`, or a decorator without `experimentalDecorators`. | Rule 13. |
| `E-DEFINE` | An assignment to a `--define`d path. | Rule 14. |
| `E-UNSUPPORTED` | A file type the bundler does not load (CSS, images, `.node` addons, WebAssembly). | Out of scope for this version. |

Out of scope altogether, and refused by the command line rather than per
module: code splitting, plugins, CSS and HTML entry points, minification,
macros, bytecode, and CommonJS or IIFE output.

## Divergences no build can detect

These differ, and no static check can tell whether a program depends on
them, so the bundler cannot refuse them. Each is off the list in
[What is observed](#what-is-observed).

- **Printing a namespace object.** Node's `util.inspect` recognizes a real
  namespace object through a V8 internal no JavaScript object can satisfy,
  and prints `[Module: null prototype] { … }`. The bundle's namespace
  objects are proxies that keep every internal method of §10.4.6, and
  `util.inspect` prints them as `[Object: null prototype] [Module] { … }`.
- **Stack traces.** Frames name the bundle file, unless the module is a
  sloppy CommonJS module under `--target node` ([rule 6](#6-commonjs-modules)).
- **A static import graph that throws, followed by a dynamic import of a
  module in it**, from an `uncaughtException` handler. Node rejects the
  import with the first error; the bundle's eager modules do not record
  it.
