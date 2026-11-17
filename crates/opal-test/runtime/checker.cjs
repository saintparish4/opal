// The type-check ts-jest does as it emits each file, done once for the run
// in a process of its own (transformers.cjs runs ts-jest to emit only).
//
// ts-jest checks a file with a LanguageService when it transforms it, and
// throws its TSError from the transform when the file's semantic and
// syntactic diagnostics, filtered by its `diagnostics` options, aren't
// empty. Each Jest worker builds that LanguageService for itself, and makes
// a new Program for every file it adds. Here one Program holds the
// tsconfig's files and the run's test files, under ts-jest's own ConfigSet:
// the compiler options it uses, the files it reports on, the codes it
// ignores and the TSError it makes are ts-jest's.
//
// What a file's check came to is kept across runs with what it depended on:
// the files it imports, all the way down, and every file that declares
// things globally, which TypeScript's own builder treats the same way. A
// file none of those changed for keeps its answer, and a run whose files
// all keep theirs doesn't load TypeScript. An error is always looked at
// again, as what fixes it (an install, say) can be outside what was read.
//
// A worker asks about each file ts-jest emits for it, and waits: a file with
// errors throws where ts-jest would have thrown. Between questions the
// checker works through the run's test files in the order they were handed
// out, so most answers are ready when asked for.
//
// One JSON message a line each way, over stdin and stdout:
//   in:  {type: 'start', config, tests}, then {type: 'check', id, path,
//        entry} for each question, and {type: 'finish'};
//   out: {id, verdict} for each question, verdict null or {codes,
//        diagnosticText, message, throws}; {type: 'done'} once the state is
//        saved.
'use strict';

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const readline = require('readline');
// Node's compile cache, for what loads in this process's own realm: a
// project's transformer, and TypeScript, load in half the time the second
// time.
const Module = require('module');
if (process.env.OPAL_TEST_CACHE && typeof Module.enableCompileCache === 'function') {
  Module.enableCompileCache(require('path').join(process.env.OPAL_TEST_CACHE, 'node-compile-cache'));
}
const {projectConfigOf, stableStringify, tsconfigChain} = require('./transformers.cjs');

// ts-jest's own logger would print again, from here, what the workers'
// ts-jest has printed. With no targets it prints nothing.
process.env.TS_JEST_LOG = '';

const INDEX_VERSION = 2;
const CACHE_ROOT = process.env.OPAL_TEST_CACHE || null;
// OPAL_TEST_PROFILE=1 prints where the check's time went.
const profile = process.env.OPAL_TEST_PROFILE ? {checked: 0, checkMs: 0, kept: 0, loadMs: 0, stale: null} : null;
const send = message => process.stdout.write(`${JSON.stringify(message)}\n`);
const sha256 = data => crypto.createHash('sha256').update(data).digest('hex');

let start = null;
let finishing = false;
const requests = [];
let wake = null;
const ready = () => {
  const resolve = wake;
  wake = null;
  if (resolve) resolve();
};

readline.createInterface({input: process.stdin}).on('line', line => {
  if (!line) return;
  const message = JSON.parse(line);
  if (message.type === 'start') {
    start = message;
    start.projectConfig = projectConfigOf(message.config);
    start.projectConfigString = stableStringify(start.projectConfig);
  } else if (message.type === 'check') {
    requests.push(message);
  } else if (message.type === 'finish') {
    finishing = true;
  }
  ready();
});
process.stdin.on('end', () => {
  finishing = true;
  ready();
});

const TS_EXTENSIONS = ['.ts', '.tsx', '.mts', '.cts', '.json'];
const JS_EXTENSIONS = ['.js', '.jsx', '.mjs', '.cjs'];

// The files below a folder TypeScript globs, the way it globs them: one
// added or removed there can change what's in the Program.
function listing(dir, extensions) {
  const names = [];
  const walk = (current, prefix) => {
    let found;
    try {
      found = fs.readdirSync(current, {withFileTypes: true});
    } catch {
      return;
    }
    for (const item of found) {
      if (item.name === 'node_modules' || item.name.startsWith('.')) continue;
      if (item.isDirectory()) walk(path.join(current, item.name), `${prefix}${item.name}/`);
      else if (extensions.some(extension => item.name.endsWith(extension))) names.push(`${prefix}${item.name}`);
    }
  };
  walk(dir, '');
  return sha256(names.sort().join('\n'));
}

const hashOf = file => {
  try {
    return sha256(fs.readFileSync(file));
  } catch {
    return null;
  }
};

// Whether a file, as its text now stands, could declare something every
// file sees: a script (no import or export) or a module with `declare
// global` or `declare module`. Erring towards yes costs a recheck.
function mayDeclareGlobally(file) {
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return true;
  }
  return !/\b(?:import|export)\b/.test(text) || /\bdeclare\s+(?:global|module)\b/.test(text) || /\/\/\/\s*<reference/.test(text);
}

// ts-jest's ConfigSet.raiseDiagnostics, as data: what it throws or, under
// `warnOnly`, logs.
function verdictOf(configSet, diagnostics, file) {
  try {
    configSet.raiseDiagnostics(diagnostics, file, {warn() {}});
  } catch (error) {
    return {codes: error.diagnosticCodes, diagnosticText: error.diagnosticText, message: error.message, throws: true};
  }
  const {ignoreCodes} = configSet._diagnostics;
  const reported = diagnostics.filter(
    diagnostic =>
      !ignoreCodes.includes(diagnostic.code) && !(diagnostic.file && !configSet.shouldReportDiagnostics(diagnostic.file.fileName)),
  );
  if (reported.length === 0) return null;
  const error = configSet.createTsError(reported);
  return {codes: error.diagnosticCodes, diagnosticText: error.diagnosticText, message: error.message, throws: false};
}

// --- one ts-jest transform entry ---------------------------------------------

class Entry {
  constructor(index) {
    const [, transformPath, options] = start.config.transform[index];
    this.index = index;
    this.transformPath = transformPath;
    this.transformerConfig = options === null ? undefined : options;
    this.stored = undefined;
    this.state = null;
    // file -> {verdict, closure}: this run's answers.
    this.answers = new Map();
    const versionOf = request => {
      try {
        const file = require.resolve(`${request}/package.json`, {paths: [path.dirname(transformPath)]});
        return `${file}@${JSON.parse(fs.readFileSync(file, 'utf8')).version}`;
      } catch {
        return request;
      }
    };
    // Where this entry's answers are kept: one place per ts-jest, TypeScript,
    // options and config.
    const key = sha256(
      [INDEX_VERSION, versionOf('ts-jest'), versionOf('typescript'), stableStringify(this.transformerConfig) || '', start.projectConfigString].join('\0'),
    ).slice(0, 32);
    this.indexFile = CACHE_ROOT ? path.join(CACHE_ROOT, 'test-check', `${key}.json`) : null;
    // The tsconfig files ts-jest reads, each with its text.
    this.tsconfigs = tsconfigChain(start.projectConfig, {
      ...(start.projectConfig.globals || {})['ts-jest'],
      ...this.transformerConfig,
    });
  }

  // The last run's answers that still hold: {changed, index}, or null when
  // none does. One changed tsconfig, folder listing or global file, or a
  // changed file that may now declare globally, and none does; otherwise an
  // answer holds unless a file in its closure changed.
  storedIndex() {
    if (this.stored !== undefined) return this.stored;
    this.stored = null;
    if (!this.indexFile || !this.tsconfigs) return null;
    let index;
    try {
      index = JSON.parse(fs.readFileSync(this.indexFile, 'utf8'));
    } catch {
      return null;
    }
    const stale = reason => {
      if (profile) profile.stale = reason;
      return null;
    };
    if (index.version !== INDEX_VERSION) return stale('another version');
    for (let i = 0; i < this.tsconfigs.length; i += 2) {
      if (index.tsconfigs[this.tsconfigs[i]] !== sha256(this.tsconfigs[i + 1])) return stale(this.tsconfigs[i]);
    }
    for (const [dir, {extensions, hash}] of Object.entries(index.dirs)) {
      if (listing(dir, extensions) !== hash) return stale(dir);
    }
    const changed = new Set(Object.keys(index.files).filter(file => hashOf(file) !== index.files[file]));
    const globals = new Set(index.globals);
    for (const file of changed) {
      if (globals.has(file) || mayDeclareGlobally(file)) return stale(file);
    }
    this.stored = {changed, index};
    return this.stored;
  }

  // A kept answer for `file`, or undefined.
  keptAnswer(file) {
    const stored = this.storedIndex();
    const kept = stored && stored.index.answers[file];
    if (!kept || kept.verdict !== null || kept.closure.some(dependency => stored.changed.has(dependency))) return undefined;
    if (profile) profile.kept += 1;
    return kept;
  }

  // ts-jest, its ConfigSet and the Program, for the first file no kept
  // answer is good for.
  load() {
    if (this.state) return this.state;
    const started = Date.now();
    try {
      return this.loadProgram();
    } finally {
      if (profile) profile.loadMs += Date.now() - started;
    }
  }

  loadProgram() {
    let transformer = require(this.transformPath);
    if (transformer && transformer.__esModule && transformer.default) transformer = transformer.default;
    if (typeof transformer.createTransformer === 'function') transformer = transformer.createTransformer(this.transformerConfig);
    if (typeof transformer._createConfigSet !== 'function' || typeof transformer._createCompiler !== 'function') {
      // A ts-jest opal can't run to emit only checks types itself, in the
      // workers (transformers.cjs's emitOnly).
      this.state = {off: true};
      return this.state;
    }
    // The config _configsFor gives the ConfigSet: the transformer's options
    // over globals['ts-jest'].
    const config = start.projectConfig;
    const globals = config.globals || {};
    const configSet = transformer._createConfigSet(
      this.transformerConfig
        ? {...config, globals: {...globals, 'ts-jest': {...globals['ts-jest'], ...this.transformerConfig}}}
        : config,
    );
    if (configSet.isolatedModules) {
      // ts-jest emits with transpileModule and checks nothing more: what that
      // reports, the workers' ts-jest reports.
      this.state = {configSet, off: true};
      return this.state;
    }
    const ts = configSet.compilerModule;
    // ts-jest's TsCompiler, made without its LanguageService, for the
    // options it compiles with and the ones it resolves imports with.
    configSet.isolatedModules = true;
    transformer._createCompiler(configSet, new Map());
    configSet.isolatedModules = false;
    const compiler = transformer._compiler && (transformer._compiler._compilerInstance || transformer._compiler);
    const initial = (compiler && compiler._initialCompilerOptions) || {...configSet.parsedTsConfig.options};
    let options;
    if (compiler && typeof compiler.fixupCompilerOptionsForModuleKind === 'function') {
      options = compiler.fixupCompilerOptionsForModuleKind(initial, false);
    } else {
      options = (compiler && compiler._compilerOptions) || {...initial, module: ts.ModuleKind.CommonJS};
    }
    // ts-jest 29.4 resolves with the tsconfig's options, not the ones it
    // compiles with (its _resolveModuleName).
    const resolveWith = compiler && typeof compiler._resolveModuleName === 'function' ? initial : options;
    const read = new Map();
    const imports = new Map();
    const host = ts.createCompilerHost(options);
    const readFile = host.readFile.bind(host);
    host.readFile = file => {
      const text = readFile(file);
      if (text !== undefined && !read.has(file)) read.set(file, hashOf(file));
      return text;
    };
    const cache = ts.createModuleResolutionCache(
      configSet.cwd,
      name => (ts.sys.useCaseSensitiveFileNames ? name : name.toLowerCase()),
      resolveWith,
    );
    const kinds = ts.ModuleResolutionKind;
    const kind = typeof ts.getEmitModuleResolutionKind === 'function' ? ts.getEmitModuleResolutionKind(resolveWith) : undefined;
    const modeAware = kind !== undefined && kind !== kinds.Classic && kind !== (kinds.Node10 ?? kinds.NodeJs);
    host.resolveModuleNames = (names, containingFile) => {
      const mode =
        modeAware && typeof ts.getImpliedNodeFormatForFile === 'function'
          ? ts.getImpliedNodeFormatForFile(containingFile, cache.getPackageJsonInfoCache?.(), host, resolveWith)
          : undefined;
      const resolved = names.map(name => ts.resolveModuleName(name, containingFile, resolveWith, host, cache, undefined, mode).resolvedModule);
      imports.set(containingFile, resolved.filter(Boolean).map(module => module.resolvedFileName));
      return resolved;
    };
    // The tsconfig's TypeScript files but its tests, which ts-jest starts
    // its LanguageService with, and the run's test files, which it adds as
    // it transforms them. When the last run's answers mostly hold, only the
    // files that declare globally: those and what a file imports are all its
    // check can see (storedIndex), and the files to check come in as roots
    // one by one.
    let roots;
    if (this.stored) {
      const libs = path.dirname(ts.getDefaultLibFilePath(options));
      roots = new Set(this.stored.index.globals.filter(file => path.dirname(file) !== libs && fs.existsSync(file)));
    } else {
      roots = new Set(configSet.parsedTsConfig.fileNames.filter(file => /\.[cm]?tsx?$/.test(file) && (file.endsWith('.d.ts') || !configSet.isTestFile(file))));
      for (const test of start.tests) if (entryIndexFor(test) === this.index) roots.add(test);
    }
    this.state = {configSet, host, imports, options, read, roots, ts};
    this.build();
    return this.state;
  }

  build() {
    const {host, options, roots, ts} = this.state;
    this.state.program = ts.createProgram({host, oldProgram: this.state.program, options, rootNames: [...roots]});
  }

  // A file's verdict: null, or the TSError ts-jest would throw for it.
  check(file) {
    const answered = this.answers.get(file);
    if (answered) return answered.verdict;
    const kept = this.keptAnswer(file);
    if (kept) {
      this.answers.set(file, kept);
      return kept.verdict;
    }
    const state = this.load();
    let verdict = null;
    let closure = [];
    if (!state.off && state.configSet.shouldReportDiagnostics(file)) {
      if (!state.program.getSourceFile(file)) {
        state.roots.add(file);
        this.build();
      }
      const sourceFile = state.program.getSourceFile(file);
      if (sourceFile) {
        const started = Date.now();
        const diagnostics = [...state.program.getSemanticDiagnostics(sourceFile), ...state.program.getSyntacticDiagnostics(sourceFile)];
        verdict = verdictOf(state.configSet, diagnostics, file);
        closure = this.closureOf(sourceFile.fileName);
        if (profile) {
          profile.checked += 1;
          profile.checkMs += Date.now() - started;
        }
      }
    }
    this.answers.set(file, {closure, verdict});
    return verdict;
  }

  // The file and every file it imports, all the way down.
  closureOf(file) {
    const seen = new Set([file]);
    const pending = [file];
    while (pending.length > 0) {
      for (const dependency of this.state.imports.get(pending.pop()) || []) {
        if (!seen.has(dependency)) {
          seen.add(dependency);
          pending.push(dependency);
        }
      }
    }
    return [...seen];
  }

  // The answers, kept and new, with what they depend on, written whole and
  // renamed into place.
  save() {
    const state = this.state;
    if (!this.indexFile || !this.tsconfigs || (state && state.off)) return;
    const stored = this.stored;
    if (!state && !stored) return;
    const answers = {};
    if (stored) {
      for (const [file, kept] of Object.entries(stored.index.answers)) {
        if (kept.verdict === null && !kept.closure.some(dependency => stored.changed.has(dependency))) answers[file] = kept;
      }
    }
    for (const [file, answer] of this.answers) answers[file] = answer;
    let files;
    let globals;
    let dirs;
    if (state) {
      const {configSet, program, ts} = state;
      files = Object.fromEntries(state.read);
      globals = program
        .getSourceFiles()
        .filter(sourceFile => !ts.isExternalModule(sourceFile) || (sourceFile.moduleAugmentations || []).length > 0)
        .map(sourceFile => sourceFile.fileName);
      const extensions = configSet.parsedTsConfig.options.allowJs ? [...TS_EXTENSIONS, ...JS_EXTENSIONS] : TS_EXTENSIONS;
      dirs = {};
      for (const dir of Object.keys(configSet.parsedTsConfig.wildcardDirectories || {})) {
        dirs[dir] = {extensions, hash: listing(dir, extensions)};
      }
      // The @types folders TypeScript includes from: one added is in every
      // Program after.
      for (let dir = configSet.rootDir; ; dir = path.dirname(dir)) {
        const types = path.join(dir, 'node_modules', '@types');
        if (fs.existsSync(types)) dirs[types] = {extensions: ['.d.ts', 'package.json'], hash: listing(types, ['.d.ts', 'package.json'])};
        if (path.dirname(dir) === dir) break;
      }
      // A kept answer's closure was read last run.
      for (const answer of Object.values(answers)) {
        for (const file of answer.closure) if (!(file in files)) files[file] = hashOf(file);
      }
    } else {
      ({dirs, globals} = stored.index);
      files = {};
      for (const file of Object.keys(stored.index.files)) files[file] = hashOf(file);
    }
    const tsconfigs = {};
    for (let i = 0; i < this.tsconfigs.length; i += 2) tsconfigs[this.tsconfigs[i]] = sha256(this.tsconfigs[i + 1]);
    fs.mkdirSync(path.dirname(this.indexFile), {recursive: true});
    const staging = `${this.indexFile}.${process.pid}`;
    fs.writeFileSync(staging, JSON.stringify({answers, dirs, files, globals, tsconfigs, version: INDEX_VERSION}));
    fs.renameSync(staging, this.indexFile);
  }
}

// --- the run ---------------------------------------------------------------------

const entries = new Map();
let routes = null;

// The transform entry ts-jest handles a file under, the one a worker picks,
// or -1.
function entryIndexFor(file) {
  if (!routes) {
    routes = {
      ignore: (start.config.transformIgnorePatterns || ['/node_modules/', '\\.pnp\\.[^\\/]+$']).map(pattern => new RegExp(pattern)),
      transform: start.config.transform.map(([pattern], index) => ({
        index,
        regex: new RegExp(pattern),
        runner: start.config.transformRunners[index].runner,
      })),
    };
  }
  if (routes.ignore.some(regex => regex.test(file))) return -1;
  const match = routes.transform.find(({regex}) => regex.test(file));
  return match && match.runner === 'ts-jest' ? match.index : -1;
}

function entry(index) {
  let found = entries.get(index);
  if (!found) {
    found = new Entry(index);
    entries.set(index, found);
  }
  return found;
}

function answer(request) {
  let verdict = null;
  try {
    verdict = entry(request.entry).check(request.path);
  } catch (error) {
    process.stderr.write(`opal test: couldn't check the types in ${request.path}: ${error && error.stack ? error.stack : error}\n`);
  }
  send({id: request.id, verdict});
}

async function main() {
  while (!start && !finishing) await new Promise(resolve => (wake = resolve));
  if (start) {
    const ahead = start.tests.slice();
    while (!finishing) {
      if (requests.length > 0) {
        answer(requests.shift());
        continue;
      }
      if (ahead.length > 0) {
        const test = ahead.shift();
        const index = entryIndexFor(test);
        try {
          if (index >= 0) entry(index).check(test);
        } catch {}
        // Lets the questions that came in meanwhile be read.
        await new Promise(resolve => setImmediate(resolve));
        continue;
      }
      await new Promise(resolve => (wake = resolve));
    }
    while (requests.length > 0) answer(requests.shift());
    for (const found of entries.values()) {
      try {
        found.save();
      } catch (error) {
        process.stderr.write(`opal test: couldn't save the type check's answers: ${error && error.message}\n`);
      }
    }
  }
  if (profile) {
    process.stderr.write(
      `opal test profile: type check loaded in ${profile.loadMs} ms, ${profile.checked} files checked in ${profile.checkMs} ms, ${profile.kept} kept${profile.stale ? `, all stale: ${profile.stale}` : ''}\n`,
    );
  }
  send({type: 'done'});
}

main();
