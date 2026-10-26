//! Walks imports from an entry file and builds the module graph.
//!
//! Two properties matter more than feature coverage here:
//!
//! 1. **Nothing is fatal.** A specifier that cannot be resolved becomes an
//!    `Unresolved` edge plus a diagnostic. Real projects import optional
//!    dependencies, platform-specific natives, and packages that are not
//!    installed yet; a resolver that stops at the first miss cannot describe a
//!    real project.
//! 2. **Every filesystem question is recorded.** The resolver only ever asks
//!    "is there a file at this path?", and both answers go into the
//!    [`ResolveTrace`] — hits as content hashes, misses as probed paths. The
//!    misses are what make the memo layer correct: adding `./x.ts` next to an
//!    `./x` that resolved to `./x/index.js` changes the answer without changing
//!    any file the previous run read.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io;
use std::rc::Rc;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, ExportAllDeclaration, ExportFromDeclaration, Expression,
    ImportDeclaration, ImportExpression, ImportOrExportKind, TSImportEqualsDeclaration,
    TSModuleReference,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Dependency, DependencyKind, DependencyTarget, ModuleGraph, ModuleGraphBuilder, ModuleSystem,
    SourceKind,
};
use crate::hash::{ContentHash, HashBuilder};
use crate::path::NormalizedPath;

/// Node builtins Opal recognizes without a `node:` prefix. Anything prefixed
/// `node:` is treated as a builtin regardless, which is how new ones arrive.
const NODE_BUILTINS: &[&str] = &[
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "domain",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("entry {0} is not a file")]
    EntryNotFound(NormalizedPath),
    #[error("{path}: {source}")]
    Io {
        path: NormalizedPath,
        #[source]
        source: io::Error,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolverOptions {
    /// Tried in order when a specifier has no extension. Node's own order is
    /// `.js`, `.json`, `.node`; the TypeScript extensions follow so a TS
    /// project resolves without a separate mode.
    pub extensions: Vec<String>,
    /// Export conditions beyond `import`/`require` (chosen per edge) and
    /// `default` (always last).
    pub conditions: Vec<String>,
    /// `import type { T } from './t'` erases at compile time, so by default it
    /// is not an edge. Type-checking tools would want it on.
    pub follow_type_only_imports: bool,
    /// The import source of the JSX automatic runtime (`react`), or `None`
    /// for no JSX edges. When set, a `.jsx` or `.tsx` file that contains JSX
    /// also depends on `<source>/jsx-runtime`, the import a JSX compiler
    /// adds. A `@jsxImportSource` pragma overrides the source for its file,
    /// and `@jsxRuntime classic` removes the edge, as they do for the
    /// compiler.
    pub jsx_runtime: Option<String>,
    /// Resolve a JavaScript importer's specifiers exactly as Node 24 does:
    /// an `import` of a relative path names the file itself, with no
    /// extension or directory index added; a `require` adds only `.js`,
    /// `.json`, and `.node`; and a package `main` follows Node's legacy main
    /// resolution. Off, the resolver also accepts what TypeScript and
    /// bundlers accept. TypeScript and JSX importers resolve the permissive
    /// way either way, since what runs them is a compiler, not Node.
    pub node_exact: bool,
}

/// How a specifier may be completed into a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probing {
    /// Extensions from the options, directory indexes, and TypeScript's
    /// `.js` to `.ts` rewrite.
    Permissive,
    /// Node's ES module resolution: the file as named.
    NodeEsm,
    /// Node's CommonJS resolution: `.js`, `.json`, `.node`, and indexes.
    NodeCjs,
}

/// `Module._extensions` in Node 24, the only extensions `require()` adds.
const NODE_EXTENSIONS: [&str; 3] = [".js", ".json", ".node"];

impl Default for ResolverOptions {
    fn default() -> Self {
        Self {
            extensions: [
                ".js", ".json", ".node", ".mjs", ".cjs", ".jsx", ".ts", ".tsx", ".mts", ".cts",
            ]
            .iter()
            .map(|extension| (*extension).to_string())
            .collect(),
            conditions: vec!["node".to_string()],
            follow_type_only_imports: false,
            jsx_runtime: None,
            node_exact: false,
        }
    }
}

impl ResolverOptions {
    /// Part of the memo key: changing options must not reuse an old graph.
    pub fn digest(&self) -> ContentHash {
        let mut builder = HashBuilder::new("opal.resolver.options.v1");
        builder.push_u64(self.extensions.len() as u64);
        for extension in &self.extensions {
            builder.push_str(extension);
        }
        builder.push_u64(self.conditions.len() as u64);
        for condition in &self.conditions {
            builder.push_str(condition);
        }
        builder.push_u64(u64::from(self.follow_type_only_imports));
        // Only when set, so a graph resolved without JSX edges keeps the key
        // it had before the option existed.
        if let Some(source) = &self.jsx_runtime {
            builder.push_str("jsx-runtime");
            builder.push_str(source);
        }
        if self.node_exact {
            builder.push_str("node-exact");
        }
        builder.finish()
    }
}

/// Every filesystem fact the graph depends on, root-relative.
///
/// `files` are paths that were read, with the content that was read. `missing`
/// are paths probed and found absent — probed *before* the candidate that won,
/// so the set is exactly the negative facts the result relies on.
/// `directories` and `missing_directories` are directories whose existence a
/// resolution turned on: Node's ES module resolver stops at the first
/// `node_modules/<name>` directory, whatever is in it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveTrace {
    pub files: BTreeMap<NormalizedPath, ContentHash>,
    pub missing: BTreeSet<NormalizedPath>,
    #[serde(default)]
    pub directories: BTreeSet<NormalizedPath>,
    #[serde(default)]
    pub missing_directories: BTreeSet<NormalizedPath>,
}

pub struct Resolution {
    pub graph: ModuleGraph,
    pub trace: ResolveTrace,
}

/// Resolves the graph reachable from `entry`.
pub fn resolve(
    root: &NormalizedPath,
    entry: &NormalizedPath,
    options: &ResolverOptions,
) -> Result<Resolution, ResolveError> {
    Resolver::new(root.clone(), options).run(entry)
}

enum Target {
    Path(NormalizedPath),
    Builtin(String),
    Unresolved(String),
}

struct Resolver<'a> {
    root: NormalizedPath,
    options: &'a ResolverOptions,
    builder: ModuleGraphBuilder,
    files: BTreeMap<NormalizedPath, ContentHash>,
    missing: BTreeSet<NormalizedPath>,
    directories: BTreeSet<NormalizedPath>,
    missing_directories: BTreeSet<NormalizedPath>,
    /// Directory -> parsed package.json. `Rc` because resolution reads a
    /// manifest while holding `&mut self`; single-threaded for now, and future
    /// parallelism would move this to `Arc` plus a shared cache.
    manifests: HashMap<NormalizedPath, Option<Rc<Value>>>,
    sources: HashMap<NormalizedPath, Rc<String>>,
    /// What each loaded file turned out to be. Kept because an extensionless
    /// file's kind is decided by its first line, which only `load` has seen.
    kinds: HashMap<NormalizedPath, SourceKind>,
    /// How the specifier being resolved may be completed.
    probing: Probing,
}

impl<'a> Resolver<'a> {
    fn new(root: NormalizedPath, options: &'a ResolverOptions) -> Self {
        Self {
            builder: ModuleGraphBuilder::new(root.clone()),
            root,
            options,
            files: BTreeMap::new(),
            missing: BTreeSet::new(),
            directories: BTreeSet::new(),
            missing_directories: BTreeSet::new(),
            manifests: HashMap::new(),
            sources: HashMap::new(),
            kinds: HashMap::new(),
            probing: Probing::Permissive,
        }
    }

    fn run(mut self, entry: &NormalizedPath) -> Result<Resolution, ResolveError> {
        let entry = self.root.join(entry.as_str());
        if !self.probe(&entry) {
            return Err(ResolveError::EntryNotFound(entry));
        }

        let entry_id = self.intern(&entry)?;
        let mut queue = VecDeque::from([(entry_id, entry)]);

        // Iterative rather than recursive: dependency chains in real projects
        // get deep enough to be a stack-overflow risk, and a package manager
        // that panics on a deep tree is worse than a slow one.
        while let Some((id, path)) = queue.pop_front() {
            for (specifier, kind) in self.dependencies_of(&path)? {
                let directory = path.parent().unwrap_or_else(|| self.root.clone());
                self.probing = self.probing_for(&path, kind);
                let target = self.resolve_specifier(&directory, &specifier, kind);
                let target = match target {
                    Target::Builtin(name) => DependencyTarget::Builtin { name },
                    Target::Unresolved(reason) => {
                        self.builder.add_diagnostic(
                            &path,
                            format!("cannot resolve {specifier:?}: {reason}"),
                        );
                        DependencyTarget::Unresolved { reason }
                    }
                    Target::Path(resolved) => {
                        let known = self.builder.id_of(&resolved);
                        let target_id = match known {
                            Some(existing) => existing,
                            None => {
                                let new_id = self.intern(&resolved)?;
                                queue.push_back((new_id, resolved));
                                new_id
                            }
                        };
                        DependencyTarget::Module {
                            id: super::ModuleId(target_id as u32),
                        }
                    }
                };
                self.builder.add_dependency(
                    id,
                    Dependency {
                        specifier,
                        kind,
                        target,
                    },
                );
            }
        }

        let root = self.root.clone();
        let relative = |path: NormalizedPath| path.relative_to(&root).unwrap_or(path);
        Ok(Resolution {
            trace: ResolveTrace {
                files: self
                    .files
                    .into_iter()
                    .map(|(path, hash)| (relative(path), hash))
                    .collect(),
                missing: self.missing.into_iter().map(relative).collect(),
                directories: self.directories.into_iter().map(relative).collect(),
                missing_directories: self.missing_directories.into_iter().map(relative).collect(),
            },
            graph: self.builder.finish(),
        })
    }

    /// Registers a module after reading and hashing it.
    fn intern(&mut self, path: &NormalizedPath) -> Result<usize, ResolveError> {
        if let Some(id) = self.builder.id_of(path) {
            return Ok(id);
        }
        let (hash, source_kind) = self.load(path)?;
        let system = self.module_system(path, source_kind);
        Ok(self.builder.insert(path.clone(), hash, source_kind, system))
    }

    /// The (specifier, kind) pairs a module imports, deduplicated in source
    /// order.
    fn dependencies_of(
        &mut self,
        path: &NormalizedPath,
    ) -> Result<Vec<(String, DependencyKind)>, ResolveError> {
        let kind = self.kind_of(path);
        if !kind.is_parsed() {
            return Ok(Vec::new());
        }
        let source = match self.sources.get(path) {
            Some(source) => Rc::clone(source),
            None => return Ok(Vec::new()),
        };

        if nests_deeper_than(&source, MAX_BRACKET_NESTING) {
            self.builder.add_diagnostic(
                path,
                format!(
                    "brackets nest more than {MAX_BRACKET_NESTING} deep; not parsed, so its imports are not followed"
                ),
            );
            return Ok(Vec::new());
        }
        let source_type = SourceType::from_path(path.as_path()).unwrap_or_default();
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, &source, source_type).parse();

        if parsed.fatal_error {
            self.builder
                .add_diagnostic(path, "parser could not recover; imports may be missing");
        } else if !parsed.diagnostics.is_empty() {
            self.builder.add_diagnostic(
                path,
                format!(
                    "{} syntax error(s); parsed on a best-effort basis",
                    parsed.diagnostics.len()
                ),
            );
        }

        let mut collector = ImportCollector {
            found: Vec::new(),
            follow_type_only: self.options.follow_type_only_imports,
            has_jsx: false,
        };
        collector.visit_program(&parsed.program);
        if collector.has_jsx
            && let Some(source) = jsx_runtime_source(
                self.options.jsx_runtime.as_deref(),
                &source,
                &parsed.program.comments,
            )
        {
            collector.push(
                &format!("{source}/jsx-runtime"),
                DependencyKind::StaticImport,
            );
        }

        let mut seen = HashSet::new();
        Ok(collector
            .found
            .into_iter()
            .filter(|entry| seen.insert(entry.clone()))
            .collect())
    }

    fn resolve_specifier(
        &mut self,
        from: &NormalizedPath,
        specifier: &str,
        kind: DependencyKind,
    ) -> Target {
        if let Some(name) = specifier.strip_prefix("node:") {
            return Target::Builtin(name.to_string());
        }
        if is_builtin(specifier) {
            return Target::Builtin(specifier.to_string());
        }
        if specifier.starts_with("#") {
            // package.json `imports` (subpath imports) is not implemented.
            return Target::Unresolved("subpath imports (#) are not supported yet".to_string());
        }
        if specifier.is_empty() {
            return Target::Unresolved("empty specifier".to_string());
        }

        let is_relative = specifier.starts_with("./")
            || specifier.starts_with("../")
            || specifier == "."
            || specifier == "..";
        if is_relative || specifier.starts_with('/') {
            let base = from.join(specifier);
            return match self.resolve_written(&base, specifier) {
                Some(path) => Target::Path(path),
                None => {
                    Target::Unresolved("no file, extension, or directory index matched".to_string())
                }
            };
        }

        self.resolve_bare(from, specifier, kind)
    }

    /// Walks up `node_modules` directories from `from` to the project root.
    ///
    /// The walk stops at the root rather than continuing to the filesystem
    /// root: a dependency resolved from outside the project would make the
    /// graph unshareable, since the cache is keyed on root-relative paths.
    fn resolve_bare(
        &mut self,
        from: &NormalizedPath,
        specifier: &str,
        kind: DependencyKind,
    ) -> Target {
        let (package, subpath) = split_bare_specifier(specifier);
        let mut directory = Some(from.clone());

        while let Some(current) = directory {
            if current.file_name() != Some("node_modules") {
                let candidate = current.join("node_modules").join(&package);
                if let Some(target) = self.resolve_package(&candidate, &subpath, kind) {
                    return target;
                }
            }
            if current == self.root {
                break;
            }
            directory = current
                .parent()
                .filter(|parent| parent.starts_with(&self.root));
        }

        Target::Unresolved(format!(
            "package {package:?} is not installed under the project root"
        ))
    }

    /// Resolves inside one candidate package directory. `None` means "this
    /// directory is not a package" so the caller keeps walking up.
    fn resolve_package(
        &mut self,
        package_dir: &NormalizedPath,
        subpath: &str,
        kind: DependencyKind,
    ) -> Option<Target> {
        let manifest = self.manifest(package_dir);

        let Some(manifest) = manifest else {
            return self.resolve_package_without_manifest(package_dir, subpath);
        };

        if let Some(exports) = manifest.get("exports") {
            return Some(self.resolve_exports(package_dir, exports, subpath, kind));
        }

        if subpath == "." && self.probing != Probing::Permissive {
            // `tryPackage` for `require()` and `legacyMainResolve` for
            // `import`: `main` as a file, with an extension, or as a
            // directory's index, then the package's own index. Neither reads
            // a package.json inside `main`.
            return match self.resolve_directory_as_node(package_dir) {
                Some(resolved) => Some(Target::Path(resolved)),
                // `require()` moves on to the next `node_modules` from a
                // package with no `main` and no index, and throws for a
                // `main` that names nothing.
                None if self.probing == Probing::NodeCjs
                    && manifest.get("main").and_then(Value::as_str).is_none() =>
                {
                    None
                }
                None => Some(Target::Unresolved(
                    "package has no main and no index file".to_string(),
                )),
            };
        }

        if subpath == "." {
            if let Some(main) = manifest.get("main").and_then(Value::as_str) {
                let candidate = package_dir.join(main);
                if let Some(resolved) = self.resolve_path(&candidate) {
                    return Some(Target::Path(resolved));
                }
            }
            return Some(match self.resolve_directory(package_dir) {
                Some(resolved) => Target::Path(resolved),
                None => Target::Unresolved("package has no main and no index file".to_string()),
            });
        }

        let candidate = package_dir.join(subpath.trim_start_matches("./"));
        match self.resolve_written(&candidate, subpath) {
            Some(resolved) => Some(Target::Path(resolved)),
            // `require()` tries the subpath under each `node_modules` in turn.
            None if self.probing == Probing::NodeCjs => None,
            None => Some(Target::Unresolved(format!(
                "{subpath} is not a file in the package"
            ))),
        }
    }

    /// A `node_modules/<name>` directory with no package.json.
    fn resolve_package_without_manifest(
        &mut self,
        package_dir: &NormalizedPath,
        subpath: &str,
    ) -> Option<Target> {
        if self.probing == Probing::NodeEsm {
            // Node's ES module resolver stops at the first such directory,
            // whatever is in it, and resolves there or fails.
            if !self.directory_exists(package_dir) {
                return None;
            }
            let resolved = if subpath == "." {
                self.resolve_directory_as_node(package_dir)
            } else {
                let candidate = package_dir.join(subpath.trim_start_matches("./"));
                self.resolve_written(&candidate, subpath)
            };
            return Some(match resolved {
                Some(resolved) => Target::Path(resolved),
                None => Target::Unresolved(format!(
                    "{subpath} is not a file in a package directory with no package.json"
                )),
            });
        }
        // `require()`, and the permissive resolver, move past a directory
        // that has nothing for the specifier, which also keeps a stray empty
        // directory from shadowing a real package further up.
        let resolved = if subpath == "." {
            self.resolve_directory(package_dir)
        } else {
            let candidate = package_dir.join(subpath.trim_start_matches("./"));
            self.resolve_written(&candidate, subpath)
        }?;
        Some(Target::Path(resolved))
    }

    /// `candidate` as the specifier wrote it: a path ending in `/`, or `.` or
    /// `..`, names a directory only (`Module._findPath`), and the ES module
    /// resolver imports no directory (ERR_UNSUPPORTED_DIR_IMPORT).
    fn resolve_written(
        &mut self,
        candidate: &NormalizedPath,
        written: &str,
    ) -> Option<NormalizedPath> {
        if !names_a_directory(written) {
            return self.resolve_path(candidate);
        }
        match self.probing {
            Probing::NodeEsm => None,
            _ => self.resolve_directory(candidate),
        }
    }

    /// Node's `exports` resolution, minus the parts we do not need.
    ///
    /// Supported: string sugar, condition objects (in declaration order),
    /// subpath maps, one `*` pattern per key, arrays as fallback lists, and
    /// `null` as an explicit block. Not supported: `imports`/`#` specifiers.
    /// Unlike the extension-probing paths above, an `exports` target must exist
    /// exactly as written — that is Node's rule, and being lenient here would
    /// resolve things Node refuses to.
    fn resolve_exports(
        &mut self,
        package_dir: &NormalizedPath,
        exports: &Value,
        subpath: &str,
        kind: DependencyKind,
    ) -> Target {
        let conditions = self.conditions_for(kind);
        let Some((selected, wildcard)) = match_subpath(exports, subpath) else {
            return Target::Unresolved(format!("exports map has no entry for {subpath:?}"));
        };
        let Some(target) = select_condition(&selected, &conditions) else {
            return Target::Unresolved(format!(
                "exports entry for {subpath:?} is blocked or matches no condition in {conditions:?}"
            ));
        };

        let target = match wildcard {
            Some(ref captured) if has_invalid_segment(captured) => {
                return Target::Unresolved(format!(
                    "{subpath:?} is not a valid subpath of this package's exports"
                ));
            }
            Some(ref captured) => target.replace('*', captured),
            None => target,
        };
        if !target
            .strip_prefix("./")
            .is_some_and(|rest| !has_invalid_segment(rest))
        {
            return Target::Unresolved(format!(
                "exports target {target:?} is not a relative path inside the package"
            ));
        }

        let candidate = package_dir.join(&target);
        if self.probe(&candidate) {
            Target::Path(candidate)
        } else {
            Target::Unresolved(format!("exports target {target:?} does not exist"))
        }
    }

    fn conditions_for(&self, kind: DependencyKind) -> Vec<String> {
        let mut conditions = self.options.conditions.clone();
        conditions.push(
            if kind.is_require_like() {
                "require"
            } else {
                "import"
            }
            .to_string(),
        );
        conditions
    }

    fn probing_for(&self, importer: &NormalizedPath, kind: DependencyKind) -> Probing {
        if !self.options.node_exact || self.kind_of(importer) != SourceKind::JavaScript {
            return Probing::Permissive;
        }
        if kind.is_require_like() {
            Probing::NodeCjs
        } else {
            Probing::NodeEsm
        }
    }

    /// File, then file+extension, then directory index.
    fn resolve_path(&mut self, candidate: &NormalizedPath) -> Option<NormalizedPath> {
        if self.probe(candidate) {
            return Some(candidate.clone());
        }
        match self.probing {
            // Node's ESM resolver does not complete a path; a directory is
            // ERR_UNSUPPORTED_DIR_IMPORT and a missing extension is
            // ERR_MODULE_NOT_FOUND.
            Probing::NodeEsm => return None,
            Probing::NodeCjs => {
                for extension in NODE_EXTENSIONS {
                    let with_extension = candidate.with_suffix(extension);
                    if self.probe(&with_extension) {
                        return Some(with_extension);
                    }
                }
                return self.resolve_directory(candidate);
            }
            Probing::Permissive => {}
        }
        for extension in &self.options.extensions.clone() {
            let with_extension = candidate.with_suffix(extension);
            if self.probe(&with_extension) {
                return Some(with_extension);
            }
        }
        // TypeScript projects import the *emitted* name: `./x.js` on disk is
        // `./x.ts`. tsc and every bundler rewrite this, so a resolver that
        // doesn't cannot walk a real TS codebase.
        for (from, to) in [
            (".js", [".ts", ".tsx"].as_slice()),
            (".mjs", [".mts"].as_slice()),
            (".cjs", [".cts"].as_slice()),
        ] {
            if let Some(stem) = candidate.as_str().strip_suffix(from) {
                for extension in to {
                    let rewritten = NormalizedPath::new(format!("{stem}{extension}"));
                    if self.probe(&rewritten) {
                        return Some(rewritten);
                    }
                }
            }
        }
        self.resolve_directory(candidate)
    }

    fn resolve_directory(&mut self, directory: &NormalizedPath) -> Option<NormalizedPath> {
        if self.probing != Probing::Permissive {
            return self.resolve_directory_as_node(directory);
        }
        if let Some(manifest) = self.manifest(directory)
            && let Some(main) = manifest.get("main").and_then(Value::as_str)
        {
            let candidate = directory.join(main);
            if self.probe(&candidate) {
                return Some(candidate);
            }
            for extension in &self.options.extensions.clone() {
                let with_extension = candidate.with_suffix(extension);
                if self.probe(&with_extension) {
                    return Some(with_extension);
                }
            }
        }
        for extension in &self.options.extensions.clone() {
            let candidate = directory.join("index").with_suffix(extension);
            if self.probe(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// A directory as Node finds its file: `main` as named, with each
    /// extension, or as a directory's index, then the directory's own index.
    /// This is `tryPackage` for `require()` and `legacyMainResolve` for
    /// `import`, which try the same list.
    fn resolve_directory_as_node(&mut self, directory: &NormalizedPath) -> Option<NormalizedPath> {
        if let Some(manifest) = self.manifest(directory)
            && let Some(main) = manifest.get("main").and_then(Value::as_str)
        {
            let main = directory.join(main);
            if self.probe(&main) {
                return Some(main);
            }
            for extension in NODE_EXTENSIONS {
                let candidate = main.with_suffix(extension);
                if self.probe(&candidate) {
                    return Some(candidate);
                }
            }
            for extension in NODE_EXTENSIONS {
                let candidate = main.join("index").with_suffix(extension);
                if self.probe(&candidate) {
                    return Some(candidate);
                }
            }
        }
        for extension in NODE_EXTENSIONS {
            let candidate = directory.join("index").with_suffix(extension);
            if self.probe(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// ESM or CJS, by extension and then by the nearest `package.json` `type`.
    fn module_system(&mut self, path: &NormalizedPath, kind: SourceKind) -> ModuleSystem {
        if matches!(kind, SourceKind::Json | SourceKind::Opaque) {
            return ModuleSystem::NotApplicable;
        }
        match path.extension() {
            Some("mjs" | "mts") => return ModuleSystem::Esm,
            Some("cjs" | "cts") => return ModuleSystem::Cjs,
            _ => {}
        }

        let mut directory = path.parent();
        while let Some(current) = directory {
            if let Some(manifest) = self.manifest(&current) {
                let is_module = manifest.get("type").and_then(Value::as_str) == Some("module");
                return if is_module {
                    ModuleSystem::Esm
                } else {
                    ModuleSystem::Cjs
                };
            }
            if current == self.root {
                break;
            }
            directory = current
                .parent()
                .filter(|parent| parent.starts_with(&self.root));
        }
        ModuleSystem::Cjs
    }

    /// Reads and caches `<directory>/package.json`.
    ///
    /// A malformed manifest is treated as absent rather than as an error: npm
    /// tarballs in the wild contain files that no JSON parser accepts, and one
    /// of them must not take down resolution of an unrelated subtree.
    fn manifest(&mut self, directory: &NormalizedPath) -> Option<Rc<Value>> {
        if let Some(cached) = self.manifests.get(directory) {
            return cached.clone();
        }
        let path = directory.join("package.json");
        let parsed = match self.load(&path) {
            Ok(_) => {
                let source = self.sources.get(&path).cloned();
                match source
                    .as_deref()
                    .map(|text| serde_json::from_str::<Value>(text))
                {
                    Some(Ok(value)) => Some(Rc::new(value)),
                    Some(Err(error)) => {
                        self.builder
                            .add_diagnostic(&path, format!("invalid JSON: {error}"));
                        None
                    }
                    None => None,
                }
            }
            Err(_) => None,
        };
        self.manifests.insert(directory.clone(), parsed.clone());
        parsed
    }

    /// What a loaded file is, or what its name alone suggests.
    fn kind_of(&self, path: &NormalizedPath) -> SourceKind {
        self.kinds
            .get(path)
            .copied()
            .unwrap_or_else(|| source_kind_of(path))
    }

    /// Reads a file, recording its content hash in the trace.
    fn load(&mut self, path: &NormalizedPath) -> Result<(ContentHash, SourceKind), ResolveError> {
        if let Some(hash) = self.files.get(path) {
            return Ok((*hash, self.kind_of(path)));
        }
        if !self.probe(path) {
            return Err(ResolveError::Io {
                path: path.clone(),
                source: io::Error::from(io::ErrorKind::NotFound),
            });
        }

        let bytes = std::fs::read(path.as_path()).map_err(|source| ResolveError::Io {
            path: path.clone(),
            source,
        })?;
        let hash = ContentHash::of(&bytes);
        self.files.insert(path.clone(), hash);

        // An npm bin script — `node_modules/next/dist/bin/next` — is an
        // extensionless file with a shebang, so a name-only decision calls it
        // opaque and nothing is ever walked from it. That reads as a clean
        // pass ("1 modules, 0 edges") rather than as the silent no-op it is.
        let kind = match source_kind_of(path) {
            SourceKind::Opaque if path.extension().is_none() => {
                shebang_kind(&bytes).unwrap_or(SourceKind::Opaque)
            }
            named => named,
        };
        self.kinds.insert(path.clone(), kind);

        // Source text is kept only for files that get parsed; a `.node` addon
        // or a large asset is hashed and dropped.
        if kind.is_parsed() || matches!(kind, SourceKind::Json) {
            match String::from_utf8(bytes) {
                Ok(text) => {
                    self.sources.insert(path.clone(), Rc::new(text));
                }
                Err(_) => {
                    self.builder
                        .add_diagnostic(path, "not valid UTF-8; treated as an opaque file");
                }
            }
        }
        Ok((hash, kind))
    }

    /// The only filesystem question the resolver asks. Both answers are
    /// recorded, which is what makes the memo layer's validation complete.
    fn probe(&mut self, path: &NormalizedPath) -> bool {
        if self.files.contains_key(path) {
            return true;
        }
        if path.as_path().is_file() {
            self.missing.remove(path);
            true
        } else {
            self.missing.insert(path.clone());
            false
        }
    }

    /// Whether `path` is a directory, recorded in the trace either way.
    fn directory_exists(&mut self, path: &NormalizedPath) -> bool {
        if path.as_path().is_dir() {
            self.directories.insert(path.clone());
            true
        } else {
            self.missing_directories.insert(path.clone());
            false
        }
    }
}

/// Whether a specifier names a directory only: it ends in `/`, or its last
/// segment is `.` or `..`.
fn names_a_directory(specifier: &str) -> bool {
    specifier.ends_with('/')
        || specifier
            .rsplit('/')
            .next()
            .is_some_and(|last| last == "." || last == "..")
}

fn is_builtin(specifier: &str) -> bool {
    let head = specifier.split('/').next().unwrap_or(specifier);
    NODE_BUILTINS.contains(&head)
}

/// `@scope/pkg/sub` -> (`@scope/pkg`, `./sub`); `pkg` -> (`pkg`, `.`).
pub fn split_bare_specifier(specifier: &str) -> (String, String) {
    let mut parts = specifier.splitn(if specifier.starts_with('@') { 3 } else { 2 }, '/');
    let mut package = parts.next().unwrap_or_default().to_string();
    if specifier.starts_with('@')
        && let Some(second) = parts.next()
    {
        package.push('/');
        package.push_str(second);
    }
    match parts.next().filter(|rest| !rest.is_empty()) {
        Some(rest) => (package, format!("./{rest}")),
        None => (package, ".".to_string()),
    }
}

/// Finds the `exports` entry for a subpath, returning it plus any `*` capture.
fn match_subpath(exports: &Value, subpath: &str) -> Option<(Value, Option<String>)> {
    let Some(map) = exports.as_object() else {
        // String or array sugar applies to the package root only.
        return (subpath == ".").then(|| (exports.clone(), None));
    };
    let is_subpath_map = map.keys().any(|key| key.starts_with('.'));
    if !is_subpath_map {
        return (subpath == ".").then(|| (exports.clone(), None));
    }
    if let Some(exact) = map.get(subpath) {
        return Some((exact.clone(), None));
    }

    // Longest matching prefix wins, which is Node's rule for `*` patterns.
    let mut best: Option<(usize, Value, String)> = None;
    for (key, value) in map {
        let Some((prefix, suffix)) = key.split_once('*') else {
            continue;
        };
        let Some(rest) = subpath.strip_prefix(prefix) else {
            continue;
        };
        let Some(captured) = rest.strip_suffix(suffix) else {
            continue;
        };
        if best.as_ref().is_none_or(|(len, _, _)| prefix.len() > *len) {
            best = Some((prefix.len(), value.clone(), captured.to_string()));
        }
    }
    best.map(|(_, value, captured)| (value, Some(captured)))
}

/// Node's containment rule for `exports`. Past its leading `./`, a target may
/// not contain an empty, `.`, `..`, or `node_modules` segment, and neither may
/// the part of a specifier a `*` captures. Without it, `pkg/../../x` through
/// `"./*": "./dist/*"` resolves outside the package, into whatever sits next
/// to it, while Node refuses it.
fn has_invalid_segment(path: &str) -> bool {
    path.split(['/', '\\']).any(|segment| {
        segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.eq_ignore_ascii_case("node_modules")
    })
}

/// Walks a condition object in declaration order, which is why `serde_json` is
/// built with `preserve_order`.
fn select_condition(value: &Value, conditions: &[String]) -> Option<String> {
    match value {
        Value::String(target) => Some(target.clone()),
        Value::Array(candidates) => candidates
            .iter()
            .find_map(|candidate| select_condition(candidate, conditions)),
        Value::Object(map) => map.iter().find_map(|(key, nested)| {
            let matches = key == "default" || conditions.iter().any(|condition| condition == key);
            matches
                .then(|| select_condition(nested, conditions))
                .flatten()
        }),
        // `null` blocks a subpath on purpose.
        _ => None,
    }
}

/// A `#!` line naming node, which is how a file with no extension says it is
/// JavaScript. Anything else — `#!/bin/sh`, a binary, a plain text file —
/// stays opaque, because guessing wrong means parsing a shell script as JS.
fn shebang_kind(bytes: &[u8]) -> Option<SourceKind> {
    let first = bytes.split(|byte| *byte == b'\n').next()?;
    let shebang = std::str::from_utf8(first)
        .ok()?
        .trim_end()
        .strip_prefix("#!")?;
    // Splitting on `/` as well as whitespace covers `#!/usr/bin/node` and
    // `#!/usr/bin/env node` alike, and `env -S node --flag` falls out of it.
    shebang
        .split(|character: char| character.is_whitespace() || character == '/')
        .any(|word| matches!(word, "node" | "nodejs"))
        .then_some(SourceKind::JavaScript)
}

fn source_kind_of(path: &NormalizedPath) -> SourceKind {
    match path.extension() {
        Some("js" | "mjs" | "cjs") => SourceKind::JavaScript,
        Some("jsx") => SourceKind::Jsx,
        Some("ts" | "mts" | "cts") => SourceKind::TypeScript,
        Some("tsx") => SourceKind::Tsx,
        Some("json") => SourceKind::Json,
        _ => SourceKind::Opaque,
    }
}

struct ImportCollector {
    found: Vec<(String, DependencyKind)>,
    follow_type_only: bool,
    has_jsx: bool,
}

/// The JSX runtime a file's JSX compiles against: its `@jsxImportSource`
/// pragma, else the configured default, and none under `@jsxRuntime classic`.
fn jsx_runtime_source<'s>(
    default: Option<&'s str>,
    source: &'s str,
    comments: &[oxc_ast::Comment],
) -> Option<&'s str> {
    let default = default?;
    let mut chosen = default;
    for comment in comments {
        let text = &source[comment.span.start as usize..comment.span.end as usize];
        if let Some(rest) = text.split("@jsxRuntime").nth(1)
            && rest.split_whitespace().next() == Some("classic")
        {
            return None;
        }
        if let Some(rest) = text.split("@jsxImportSource").nth(1)
            && let Some(name) = rest.split_whitespace().next()
        {
            chosen = name.trim_end_matches("*/");
        }
    }
    Some(chosen)
}

impl ImportCollector {
    fn push(&mut self, specifier: &str, kind: DependencyKind) {
        self.found.push((specifier.to_string(), kind));
    }

    fn keeps(&self, kind: ImportOrExportKind) -> bool {
        self.follow_type_only || kind.is_value()
    }
}

impl<'a> Visit<'a> for ImportCollector {
    fn visit_import_declaration(&mut self, declaration: &ImportDeclaration<'a>) {
        if self.keeps(declaration.import_kind) {
            self.push(&declaration.source.value, DependencyKind::StaticImport);
        }
        walk::walk_import_declaration(self, declaration);
    }

    fn visit_export_from_declaration(&mut self, declaration: &ExportFromDeclaration<'a>) {
        if self.keeps(declaration.export_kind) {
            self.push(&declaration.source.value, DependencyKind::ExportFrom);
        }
        walk::walk_export_from_declaration(self, declaration);
    }

    fn visit_export_all_declaration(&mut self, declaration: &ExportAllDeclaration<'a>) {
        if self.keeps(declaration.export_kind) {
            self.push(&declaration.source.value, DependencyKind::ExportFrom);
        }
        walk::walk_export_all_declaration(self, declaration);
    }

    fn visit_import_expression(&mut self, expression: &ImportExpression<'a>) {
        // Only literal specifiers are edges. `import(someVariable)` is a real
        // pattern, and guessing at it would put fictional modules in the graph.
        if let Expression::StringLiteral(source) = &expression.source {
            self.push(&source.value, DependencyKind::DynamicImport);
        }
        walk::walk_import_expression(self, expression);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &call.callee
            && callee.name == "require"
            && let Some(Argument::StringLiteral(source)) = call.arguments.first()
        {
            self.push(&source.value, DependencyKind::Require);
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_jsx_element(&mut self, element: &oxc_ast::ast::JSXElement<'a>) {
        self.has_jsx = true;
        walk::walk_jsx_element(self, element);
    }

    fn visit_jsx_fragment(&mut self, fragment: &oxc_ast::ast::JSXFragment<'a>) {
        self.has_jsx = true;
        walk::walk_jsx_fragment(self, fragment);
    }

    fn visit_ts_import_equals_declaration(&mut self, declaration: &TSImportEqualsDeclaration<'a>) {
        if let TSModuleReference::ExternalModuleReference(reference) = &declaration.module_reference
            && self.keeps(declaration.import_kind)
        {
            self.push(&reference.expression.value, DependencyKind::TsImportEquals);
        }
        walk::walk_ts_import_equals_declaration(self, declaration);
    }
}

/// How deep `(`, `[`, and `{` may nest in a file the resolver parses. The
/// parser recurses once per bracket, and a file nested deeper could
/// overflow the caller's stack and abort the process; V8 itself rejects
/// about 5,000 nested parentheses.
pub const MAX_BRACKET_NESTING: usize = 4_000;

/// Whether `(`, `[`, and `{` nest deeper than `limit` in `source`. Brackets
/// inside strings, comments, and regular expressions count too, opening and
/// closing, so the count can be off either way for a file that has them.
pub fn nests_deeper_than(source: &str, limit: usize) -> bool {
    let mut depth = 0usize;
    for byte in source.bytes() {
        match byte {
            b'(' | b'[' | b'{' => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_bare_specifier() {
        assert_eq!(split_bare_specifier("react"), ("react".into(), ".".into()));
        assert_eq!(
            split_bare_specifier("react-dom/client"),
            ("react-dom".into(), "./client".into())
        );
        assert_eq!(
            split_bare_specifier("@scope/pkg"),
            ("@scope/pkg".into(), ".".into())
        );
        assert_eq!(
            split_bare_specifier("@scope/pkg/deep/path"),
            ("@scope/pkg".into(), "./deep/path".into())
        );
    }

    #[test]
    fn test_builtin_detection_covers_subpaths() {
        assert!(is_builtin("fs"));
        assert!(is_builtin("fs/promises"));
        assert!(!is_builtin("fs-extra"));
        assert!(!is_builtin("react"));
    }

    #[test]
    fn test_conditions_are_matched_in_declaration_order() {
        let exports = serde_json::json!({
            "import": "./esm.mjs",
            "require": "./cjs.cjs",
            "default": "./fallback.js"
        });
        let (selected, _) = match_subpath(&exports, ".").unwrap();
        assert_eq!(
            select_condition(&selected, &["node".into(), "import".into()]).unwrap(),
            "./esm.mjs"
        );
        assert_eq!(
            select_condition(&selected, &["node".into(), "require".into()]).unwrap(),
            "./cjs.cjs"
        );
        assert_eq!(
            select_condition(&selected, &["browser".into()]).unwrap(),
            "./fallback.js"
        );
    }

    #[test]
    fn test_subpath_patterns_prefer_the_longest_prefix() {
        let exports = serde_json::json!({
            "./features/*": "./src/features/*.js",
            "./*": "./src/*.js"
        });
        let (value, captured) = match_subpath(&exports, "./features/a").unwrap();
        assert_eq!(value.as_str(), Some("./src/features/*.js"));
        assert_eq!(captured.as_deref(), Some("a"));
    }

    #[test]
    fn test_exports_segments_that_would_leave_the_package_are_invalid() {
        for invalid in [
            "..",
            "../x",
            "a/../../x",
            "a//b",
            "./a",
            "a/.",
            "node_modules/x",
            "a\\..\\x",
            "",
        ] {
            assert!(
                has_invalid_segment(invalid),
                "{invalid:?} should be refused"
            );
        }
        for valid in ["a", "a/b.js", "features/x.mjs", "..a", "a.."] {
            assert!(!has_invalid_segment(valid), "{valid:?} should be allowed");
        }
    }

    #[test]
    fn test_null_export_blocks_a_subpath() {
        let exports = serde_json::json!({ "./internal/*": null });
        let (value, _) = match_subpath(&exports, "./internal/x").unwrap();
        assert_eq!(select_condition(&value, &["node".into()]), None);
    }

    #[test]
    fn test_array_export_falls_back_to_first_usable() {
        let exports = serde_json::json!({ ".": [{ "browser": "./b.js" }, "./fallback.js"] });
        let (value, _) = match_subpath(&exports, ".").unwrap();
        assert_eq!(
            select_condition(&value, &["node".into()]).unwrap(),
            "./fallback.js"
        );
    }

    #[test]
    fn test_options_digest_changes_with_options() {
        let baseline = ResolverOptions::default();
        let mut changed = baseline.clone();
        changed.follow_type_only_imports = true;
        assert_ne!(baseline.digest(), changed.digest());
    }

    #[test]
    fn test_options_digest_changes_with_the_bundlers_options() {
        let baseline = ResolverOptions::default();
        let mut react = baseline.clone();
        react.jsx_runtime = Some("react".into());
        let mut preact = baseline.clone();
        preact.jsx_runtime = Some("preact".into());
        let mut exact = baseline.clone();
        exact.node_exact = true;
        let digests = [
            baseline.digest(),
            react.digest(),
            preact.digest(),
            exact.digest(),
        ];
        for (i, digest) in digests.iter().enumerate() {
            for other in &digests[i + 1..] {
                assert_ne!(digest, other);
            }
        }
    }

    #[test]
    fn test_options_digest_is_unchanged_while_the_bundlers_options_are_unset() {
        // The digest as it was before `jsx_runtime` and `node_exact`, so a
        // graph cached then keeps its key.
        let options = ResolverOptions::default();
        let mut builder = HashBuilder::new("opal.resolver.options.v1");
        builder.push_u64(options.extensions.len() as u64);
        for extension in &options.extensions {
            builder.push_str(extension);
        }
        builder.push_u64(options.conditions.len() as u64);
        for condition in &options.conditions {
            builder.push_str(condition);
        }
        builder.push_u64(u64::from(options.follow_type_only_imports));
        assert_eq!(options.digest(), builder.finish());
    }

    #[test]
    fn test_bracket_nesting_is_measured_across_kinds() {
        assert!(!nests_deeper_than("f([{}])", 3));
        assert!(nests_deeper_than("f([{(}])", 3));
        // Closing more than was opened does not go below zero.
        assert!(!nests_deeper_than(")))(((", 3));
        assert!(nests_deeper_than(")))((((", 3));
    }
}
