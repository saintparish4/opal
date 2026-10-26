# The bundle contract

This file says what `opal build` promises, and what it refuses. It was
written before the bundler, and the bundler is held to it: every rule here
has a test that runs both programs under Node and compares them, or a
refusal with a code listed at the end. Rule 17's programs have no
unbundled run to compare with, and are checked by what the build writes
and by a page load in Chromium.

- [What correct means](#what-correct-means)
- [The reference](#the-reference)
- [What is observed](#what-is-observed)
- [Rules](#rules)
- [Refusals](#refusals)
- [Divergences the bundler does not refuse](#divergences-the-bundler-does-not-refuse)

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
  `default`. Either way it asks opal-core to resolve a JavaScript module's
  specifiers exactly as Node does (`node_exact`): an `import` of a
  relative path names the file, with no extension or directory index
  added; a `require()` adds only `.js`, `.json`, and `.node`, the keys of
  Node 24's `Module._extensions`; a package `main` is completed as
  `legacyMainResolve` completes it. A TypeScript or JSX module's
  specifiers resolve as `tsc` resolves them, extensions and indexes and
  the `.js` to `.ts` rewrite included, since a compiler, not Node, is what
  reads them.
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
   constructor name, its `message`, and whether it reached the process as
   an exception or as a rejection. Node 24 evaluates an ES module as a
   promise, so a throw at an ES module's top level is a rejection; a handler
   for `uncaughtException` receives that origin as its second argument.
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
  [Divergences the bundler does not refuse](#divergences-the-bundler-does-not-refuse).
- Timing, memory, and the count of microtask turns before a dynamic
  import settles ([rule 10](#10-dynamic-import)).
- Text of an error message that names the importing file's path, such as
  `ERR_MODULE_NOT_FOUND`'s "imported from …". The error's class and code
  are still observed.
- Where Node's own warnings point: the file, line, and quoted source in
  "Warning: Detected unsettled top-level await at …". That the warning is
  printed is observed, and so is the exit code 13 that follows.
- Node's warnings about how it loads files: `ExperimentalWarning` for type
  stripping, and `MODULE_TYPELESS_PACKAGE_JSON`, which says a `.js` file
  under a `package.json` with no `type` was parsed twice to detect ESM. They
  describe Node's loader, which the bundle replaces, and they name absolute
  paths.

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
- **TDZ messages** are the one place a name is not kept: V8 names the
  identifier the reading code wrote, and in the bundle a read through an
  import is a read of the exporting module's binding. See
  [Divergences](#divergences-the-bundler-does-not-refuse).

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

Three cases follow V8 rather than the spec. First, the spec resolves a namespace
re-export, `export * as ns from "m"` or `import * as ns from "m"; export
{ ns }`, to `m`'s namespace itself, so two `export *` that reach it through
different declarations name one binding; test262's
`ambiguous-export-bindings/namespace-unambiguous-*` tests say so. V8 gives
each declaration a binding of its own, so Node calls the name ambiguous and
fails those tests. The bundler does what Node does: one declaration
reached twice is one binding, two declarations are ambiguous.

Second, a circular request. The spec's ResolveExport returns null when it
meets a `(module, name)` it is already resolving, and moves on. V8 throws
`SyntaxError: Detected cycle while resolving name` when an import or an
indirect export (`export { x } from`) meets one, and returns null only when
an `export *` does. V8 also caches every export it resolves, so whether it
meets a cycle depends on what it resolved first: it resolves each module's
imports, then its indirect exports, as each module finishes linking
(FinishInstantiate in V8's `source-text-module.cc`). The bundler runs that
algorithm over the graph after the spec's, and refuses a program it
rejects ([`E-LINK`](#refusals)). The bindings still come from the spec's
algorithm. Modules reached only through `import()` are modelled as linking
one `import()` target at a time, in file order. test262's
`instn-star-iee-multi-cycle-same-name` is such a program; Node fails it.

Third, a namespace's names. The spec takes every name ResolveExport
resolves, so a name one `export *` module finds ambiguous is ambiguous for
every module above it. V8 builds a namespace from export tables instead
(FetchStarExports in `source-text-module.cc`): a module's table is its own
exports, plus each name its `export *` modules' tables provide with one
binding, and an ambiguous name is left out of the table. So a module above
that also gets the name from another branch has it in its namespace, where
the spec leaves it out. Importing the name by itself is still a
`SyntaxError` in V8, as in the spec. The bundle builds namespaces V8's way.
Where the `export *` declarations form a cycle, V8 reads tables that are
still partial and keeps what it read, so the names depend on which
namespace it built first, an order set by imports and by when each
`import()` runs. A namespace over such a cycle is built only when the two
algorithms agree and no name is ambiguous, and is refused otherwise
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
- **`module`** has the own properties Node's has, in Node's order: `id`
  (`"."` for the entry, else the file name), `path`, `exports`,
  `filename`, `loaded`, `children`, and `paths` (Node's
  `Module._nodeModulePaths`). `require` and `parent` are on its prototype,
  as in Node; `parent` is the module that first required it, and
  `undefined` when an ES module loaded it. Its class is the bundle's own,
  named `Module`, not `node:module`'s.
- **`__filename` and `__dirname`** are the original file's path and
  directory, computed at run time from the bundle's own location, so they
  match the unbundled program wherever the checkout is.
- **A CommonJS entry that throws at its top level** is an uncaught
  exception at once in Node, before any pending microtask runs: monitors
  see it, then a handler if there is one, and otherwise the process exits
  with code 1 (`lib/internal/process/execution.js`,
  `createOnGlobalUncaughtException`). The bundle is an ES module, where the
  same throw would be a rejection one microtask later, so the bundle's
  loader reports it the way Node does.
- **`createRequire` imported from `node:module`** returns a `require` that
  resolves with Node's resolver from the given location and returns the
  bundled instance when the file it finds is in the bundle. Node's own
  `createRequire` would load the original file a second time.

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

Under `--target browser`, `import.meta.filename` and `.dirname` are
refused, since a browser has no file paths, and `.main` is `undefined`, as
a browser gives it.

### 10. Dynamic import

`import(specifier)` with a string literal that the graph resolved to a
bundled module returns a promise for its namespace object. The module, and
any of its static dependencies not yet run, run once, after the current
synchronous code and before the promise settles. Their order is rule 2's
order, starting from the imported module. A module that is only reached
through dynamic imports is run lazily, by a generator in the bundle, and
keeps rules 2 to 4 exactly, TDZ included. Linking and evaluating stay two
steps, as in ECMA-262 (§16.2.1.6.1.2 Link before §16.2.1.6.1.3
Evaluate): before any lazy module an `import()` reaches runs, each one's
generator is created and run to its first `yield`, which makes its
functions and puts its `let`s in their TDZ, so a module that runs first
can already call another's function through a namespace or a re-export.
A lazy module that reads `arguments` at its top level is refused
([`E-UNSUPPORTED`](#refusals)): in an ES module that is a global, and in
the generator it would be the generator's own.

A link error in a lazily imported module (a missing export, an
unresolved specifier) makes Node reject that `import()` when it runs. The
bundler refuses the build instead ([`E-LINK`](#refusals)): a refusal is
always allowed, and this one is rare.

If such a module's evaluation throws, the promise rejects with that error,
and later imports of it, or of a module in the same cycle that was still
on the stack, reject with the same error (§16.2.1.6.1.3.1).

Node settles a dynamic import's promise some number of microtask turns
after the call, and that number depends on its loader, not on the
language. The bundle settles it after the current synchronous code, in a
later microtask turn; how many turns later is not observed.

`import()` of an external or builtin module stays an `import()` in the
bundle.

Any other `import()` (an expression, or a literal the graph did not
resolve) is resolved when it runs, as Node resolves it. The specifier is
converted to a string first, synchronously, as EvaluateImportCall does
(§13.3.10.2). A relative one resolves against the original module's URL;
an absolute URL is used as it is; a bare one is resolved by Node from the
bundle's location, with `import.meta.resolve`. From there it finds the same
package as from the original module only if both find it in the same
`node_modules` directory: the bundle checks that, and throws an `Error`
naming both directories when they differ, instead of loading a copy the
original program would not have. A file in the bundle is the bundled
module, with the namespace it has there; anything else Node loads. Since
such an import can reach any module, a bundle that has one builds a
namespace object for every module and exempts their exports from tree
shaking. `import()` with options is refused ([`E-DYNAMIC`](#refusals)).

### 11. Top-level await

The entry module may use top-level `await`, unless it is in a cycle: no
module runs after the entry, so suspending it changes no order. While it is
suspended it is still evaluating, so an `import()` of the entry settles only
when the entry completes, as in Node. A module that is reached only through
`import()` and statically depends on such an entry is refused: Node makes it
wait for the entry while the rest of its graph runs, and the bundle would run
all of it at once. Top-level
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
- `export * from` an external or builtin is refused when the bundle needs
  the re-exporting module's names, since they are not known until the
  external runs ([`E-LINK`](#refusals)).

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

The automatic runtime's import (`<source>/jsx-runtime`) comes from the
module graph like any other, which adds it for the import source the
`tsconfig.json` nearest the entry names, after a file's own
`@jsxImportSource` and `@jsxRuntime` pragmas, read as oxc reads them: in
the comments before the first statement, the last of each kind winning.
A JSX file under another `tsconfig.json` that names another import
source, or that a pragma switches from the classic runtime to the
automatic one, has no edge for its import and is refused
([`E-LINK`](#refusals)).

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

A module whose nearest `package.json` declares `"sideEffects": false`, or
an array of globs its path (relative to the package) matches none of, is
taken at its word: any of its top-level statements nothing reads may go,
pure or not. A glob with no `/` matches at any depth, as webpack reads
the field. That is the package author's statement, not something the
bundler checked, and it is the one place a bundle may diverge from the
program: when the statement is false. The entry is never shaken this way.

Which packages in the compatibility matrix make that statement, and
whether it held, is recorded in `compat/COMPATIBILITY.md`: each package
that says `false` was imported for its effects alone, bundled with the
claim honored and not, and the runs compared. Four claims were false
(`@vitest/expect`, `h3`, `zod`, `zod-validation-error`), and the bundler
does not honor those, in any version (`KNOWN_FALSE_CLAIMS` in `src/load.rs`).
A package is the directory under the last `node_modules` in a file's path,
since a package can carry a nameless `package.json` stub in a subpath.

### 16. Determinism

The bundle and its source map are a function of the graph snapshot, the
options, and the opal version, and nothing else: not the absolute path of
the checkout, the time, the machine, or the order the filesystem lists
directories. Paths in the output are relative to the out-dir, so with the
out-dir inside the project root (the default, `<root>/dist`) the bundle
does not depend on where the project is.

### 17. Assets, with `--assets`

No runtime gives `import logo from './logo.svg'` or `import './app.css'` a
meaning: Node refuses both, and a browser refuses to run a stylesheet as a
module. Vite gives them one, and scaffolds such as vite-react are written
against it. With `--assets`, the bundle takes Vite's, narrowly:

- An ES module's import of any file that is not JavaScript or JSON copies
  it to `<out-dir>/assets/<name>-<hash>.<ext>`, where the hash is of its
  bytes.
- The default import of such a file is the copy's URL, resolved against
  the bundle's own URL. It is its one export.
- A `.css` file has no export. Importing it links the stylesheet into
  `document.head` at its place in rule 2's order. Vite's production build
  links it in the page before any script runs, so the bundle's stylesheet
  applies a moment later than Vite's; nothing a script can read differs
  but layout measured before it loads. A stylesheet with `url()` or
  `@import` is refused, since following them is CSS bundling. Outside
  `--target browser`, a stylesheet is refused ([`E-UNSUPPORTED`](#refusals)).

Without `--assets`, all of these are refused, as Node refuses them.

## Refusals

Each refusal stops the build, prints the code, and names the module and
line.

| Code | What | Why |
|---|---|---|
| `E-PARSE` | A module does not parse. | Node would throw a `SyntaxError` before running anything. |
| `E-LINK` | An import names a missing or ambiguous export, a CommonJS name the lexer does not find, or JSON without `type: 'json'`; a cycle of re-exports V8 rejects, or a namespace with an ambiguous name over a cycle of `export *` (rule 4); or a static import cannot be resolved, including one Node does not complete (`./x` for `./x.js`). | Node fails to link the graph, before any code runs. |
| `E-TLA` | Top-level `await` outside the entry module, or in an entry that is in a cycle; or a lazily imported module that depends on an entry that awaits. | Rule 11. |
| `E-REQUIRE-ESM` | A CommonJS module `require()`s an ES module. | Node 24 runs it with its own cycle and async-module errors (`ERR_REQUIRE_CYCLE_MODULE`, `ERR_REQUIRE_ASYNC_MODULE`) that this version does not reproduce. |
| `E-DYNAMIC` | `import()` with options; or `import()` inside a sloppy CommonJS module. | Rule 10. The bundler does not read import attributes at run time, and a sloppy module is compiled at run time without access to the bundle's loader. |
| `E-IMPORT-META` | A use of `import.meta` rule 9 does not list. | Rule 9. |
| `E-EVAL` | Direct `eval` in an ES module. | `eval` can name any binding by its original name, and the bundle renames bindings. |
| `E-CJS-API` | `require.cache`, `require.extensions`, or `module.constructor` in a CommonJS module. | They expose Node's module system itself, which the bundle replaces. |
| `E-BUILTIN` | A Node builtin under `--target browser`. | There is none in a browser. |
| `E-BROWSER-FIELD` | Under `--target browser`, a package with a `browser` field in its `package.json`. | opal-core does not apply that field, so the bundle would contain different files than a browser bundler picks. |
| `E-TS` | TypeScript that `tsc` would not emit for an ES module (`export =`, `import x = require()`), a decorator without `experimentalDecorators`, or `"jsx": "preserve"`. | Rule 13. |
| `E-DEFINE` | An assignment to a `--define`d path. | Rule 14. |
| `E-UNSUPPORTED` | An ES module's import of a file that is not JavaScript or JSON, without `--assets`, or a stylesheet with `url()` or `@import`, or outside a browser (rule 17; a `require()` of one is Node's own); an entry that is not JavaScript; source or defer phase imports; `arguments` at the top level of a lazily imported module (rule 10); a destructuring or `for-in`/`for-of` assignment to an import binding; a module whose brackets nest more than 4,000 deep, or whose syntax tree nests more than 10,000 nodes deep. | Out of scope for this version, or (the last) a form the rewriter does not turn into its `TypeError`. |

Nesting that the bracket count does not see, such as 500,000 chained
`?:` (which Node runs) or 300,000 chained arrow functions (which Node
fails from 1,000), is refused as nesting more than 10,000 nodes deep,
however deep it goes. oxc's parser has no stack check of its own and
recurses once per level, so each module is parsed on a stack sized to
the file, by the most stack per byte and per bracket that
`examples/parse-stack.rs` measured, doubled (`opal_core::stack`); a
byte pays for at least half the worst bracket, so brackets hidden from
the count in a comment or a string are paid for too. If the
OS will not reserve that much, which takes a file of several megabytes,
the module is parsed on 256 MiB, where nesting deeper than about 300,000
levels aborts the build instead of being refused.

Out of scope altogether, so the command line has no flag for them: code
splitting, plugins, CSS and HTML entry points, minification, macros,
bytecode, and CommonJS or IIFE output.

## Divergences the bundler does not refuse

These differ, and the bundler builds the program anyway: either no static
check can tell whether a program depends on them, or, for TDZ messages,
the check that could would refuse common programs to keep an error
message. Each is off the list in [What is observed](#what-is-observed),
or named here as the exception to it.

- **Printing a namespace object.** Node's `util.inspect` recognizes a real
  namespace object through a V8 internal no JavaScript object can satisfy,
  and prints `[Module: null prototype] { … }`. The bundle's namespace
  objects are proxies that keep every internal method of §10.4.6, and
  `util.inspect` prints them as `[Object: null prototype] [Module] { … }`.
- **TDZ messages.** A `ReferenceError` for a binding read before its
  declaration ran has the right class, and a message naming the identifier
  as the bundle wrote it. V8 names the identifier the reading code wrote:
  for a read through an import, that is the importing module's local name
  (`import { shared as fromMain }` reads as `fromMain`), and in the bundle
  the read is of the exporting module's binding, whose name is the
  exporting module's (or the name the bundle gave it, when two modules
  declare the same one). The message matches whenever the names do.
  Making it match always would mean wrapping every read across modules in
  a cycle in a function, which is the cost the bundle does not pay.
  `tests/fixtures/esm-tdz-renamed` pins this.
- **Errors writing to a namespace object.** `ns.x = 1`, `delete ns.x`, and
  `Object.defineProperty(ns, …)` fail as §10.4.6 says and throw
  `TypeError` in strict code, with a message from the proxy (`'set' on
  proxy: trap returned falsish for property 'x'`) instead of V8's
  (`Cannot assign to read only property 'x' of object '[object Module]'`).
  A trap cannot tell whether its caller is strict, so throwing V8's message
  itself would make a sloppy caller throw where it should fail silently.
- **A namespace object as the receiver of an ordinary `[[Set]]`, while
  the binding is uninitialized.** `Reflect.set(obj, "x", v, ns)`, or
  `super.x = v` where `this` is a namespace, reads the receiver's own
  descriptor (§10.1.9.2 OrdinarySetWithOwnDescriptor), which for a binding
  in its TDZ throws `ReferenceError`, as the bundle's proxy does. V8 does
  not read it for a namespace and returns `false`, a `TypeError` in strict
  code; test262's `namespace/internals/super-access-to-tdz-binding` fails
  in Node for this. A proxy's descriptor trap cannot tell this read from
  `Object.getOwnPropertyDescriptor(ns, "x")`, which throws
  `ReferenceError` in V8 too.
- **`node:module` through its namespace or default import.**
  `import { createRequire } from 'node:module'` gets the bundle's
  `createRequire` (rule 6); `module.createRequire` through `import module
  from 'node:module'` is Node's own, which loads the original files.
- **The class of a CommonJS `module` object.** It is the bundle's own
  `Module` (rule 6), so `module instanceof require('node:module').Module`
  is `false`; reading `module.constructor` is refused (`E-CJS-API`).
- **Stack traces.** Frames name the bundle file, unless the module is a
  sloppy CommonJS module under `--target node` ([rule 6](#6-commonjs-modules)).
- **A static import graph that throws, followed by a dynamic import of a
  module in it**, from an `uncaughtException` handler. Node rejects the
  import with the first error; the bundle's eager modules do not record
  it.
