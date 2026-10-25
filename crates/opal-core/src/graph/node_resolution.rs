//! Node's module resolution, as `require()` and `import` do it at run time:
//! a port of lib/internal/modules/cjs/loader.js (`Module._resolveFilename`
//! and what it calls), lib/internal/modules/esm/resolve.js (`moduleResolve`,
//! `packageExportsResolve`, `packageImportsResolve`) and the package.json
//! lookups of src/node_modules.cc, from Node 24.19.0.
//!
//! This is not [`super::resolver`], which builds a bundler's graph and differs
//! from Node on purpose (it probes TypeScript extensions, maps `./x.js` to
//! `./x.ts`, and stops at the project root). Here the rules are Node's
//! exactly, including the ones that look like accidents: which directories a
//! walk for package.json stops at, that an `exports` target is a URL (so
//! `%20` in one is a space and `#` starts a fragment), and which failures
//! are errors with codes and which are "Cannot find module".
//!
//! Errors come back as [`Failure`], which carries what the JavaScript side
//! needs to throw Node's own error object: a Node error code and the
//! arguments its message is made from (internal/errors.js makes the
//! message), or a not-found the loader words itself, since only it knows the
//! require stack. tests/node-resolution-cross-check.rs holds this to
//! `require.resolve` and `import.meta.resolve` on generated package trees.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use serde_json::Value;

/// A package.json, with the fields resolution reads.
#[derive(Debug, Default)]
pub struct Package {
    /// The package.json's own path.
    pub path: PathBuf,
    pub name: Option<String>,
    pub main: Option<String>,
    /// `"commonjs"`, `"module"`, or None for anything else (Node's "none").
    pub module_type: Option<String>,
    pub exports: Option<Value>,
    pub imports: Option<Value>,
}

/// What a specifier resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    File(PathBuf),
    /// `node:fs` and the like, for an import.
    Builtin(String),
    /// A URL with another scheme (`data:`, `https:`), which the caller
    /// accepts or refuses.
    Url(String),
}

/// Why resolution failed, in the terms Node's error for it is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// `Cannot find module '<request>'` from require, with the require
    /// stack the loader appends.
    NotFound,
    /// A package.json whose `main` names nothing: `Cannot find module
    /// '<filename>'. Please verify that the package.json has a valid "main"
    /// entry`.
    BadMain {
        filename: PathBuf,
        package_json: PathBuf,
    },
    /// What an `exports` or `imports` target named doesn't exist, from
    /// require: `Cannot find module '<request>'`, with the error's `path`
    /// property, which Node sets differently at each place it makes this
    /// error (the package's directory, its package.json, or nothing).
    EsmNotFound {
        request: String,
        path: Option<PathBuf>,
    },
    /// A Node error, made by internal/errors.js from these arguments, with
    /// a line Node adds to its message for an import that require() would
    /// have found (`Did you mean to import "pkg/x.js"?`).
    Node {
        code: &'static str,
        args: Vec<Option<String>>,
        hint: Option<String>,
    },
}

/// A warning Node emits while resolving: `process.emitWarning(message,
/// kind, code)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
    pub kind: &'static str,
    pub code: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stat {
    File,
    Dir,
    Missing,
}

fn node(code: &'static str, args: Vec<Option<String>>) -> Failure {
    Failure::Node {
        code,
        args,
        hint: None,
    }
}

fn some(text: impl Into<String>) -> Option<String> {
    Some(text.into())
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Conditions for `require`: Node's getCjsConditions().
pub const REQUIRE_CONDITIONS: &[&str] = &["require", "node", "node-addons", "module-sync"];
/// Conditions for `import`: Node's default conditions.
pub const IMPORT_CONDITIONS: &[&str] = &["node", "import", "module-sync", "node-addons"];

/// Resolves specifiers the way Node does, remembering what it learned about
/// the filesystem (stats, real paths, package.json files) for as long as it
/// lives, as Node's caches do for a process.
#[derive(Default)]
pub struct NodeResolver {
    stats: HashMap<PathBuf, Stat>,
    real: HashMap<PathBuf, PathBuf>,
    packages: HashMap<PathBuf, Option<Rc<Package>>>,
    /// Global folders searched after every node_modules: $NODE_PATH, then
    /// ~/.node_modules, ~/.node_libraries and <prefix>/lib/node.
    global_paths: Vec<PathBuf>,
    /// Warnings resolution emitted, for the caller to pass on.
    pub warnings: Vec<Warning>,
}

impl NodeResolver {
    /// A resolver with Node's global folders for this environment: from
    /// $NODE_PATH, $HOME, and the prefix of `exec_path` (the runtime's
    /// executable, as process.execPath).
    pub fn new(exec_path: &Path) -> Self {
        let mut global_paths = Vec::new();
        if let Some(node_path) = std::env::var_os("NODE_PATH") {
            global_paths
                .extend(std::env::split_paths(&node_path).filter(|p| !p.as_os_str().is_empty()));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            global_paths.push(home.join(".node_modules"));
            global_paths.push(home.join(".node_libraries"));
        }
        if let Some(prefix) = exec_path.parent().and_then(Path::parent) {
            global_paths.push(prefix.join("lib").join("node"));
        }
        Self {
            global_paths,
            ..Self::default()
        }
    }

    fn stat(&mut self, path: &Path) -> Stat {
        if let Some(stat) = self.stats.get(path) {
            return *stat;
        }
        let stat = match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => Stat::Dir,
            Ok(_) => Stat::File,
            Err(_) => Stat::Missing,
        };
        self.stats.insert(path.to_path_buf(), stat);
        stat
    }

    fn real_path(&mut self, path: &Path) -> PathBuf {
        if let Some(real) = self.real.get(path) {
            return real.clone();
        }
        let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.real.insert(path.to_path_buf(), real.clone());
        real
    }

    /// `<dir>/package.json`, parsed, or None if there isn't one. Invalid JSON
    /// is ERR_INVALID_PACKAGE_CONFIG, as Node's reader throws.
    fn package(&mut self, dir: &Path) -> Result<Option<Rc<Package>>, Failure> {
        if let Some(cached) = self.packages.get(dir) {
            return Ok(cached.clone());
        }
        let path = dir.join("package.json");
        let package = match std::fs::read(&path) {
            Err(_) => None,
            Ok(bytes) => {
                let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
                    node(
                        "ERR_INVALID_PACKAGE_CONFIG",
                        vec![Some(display(&path)), None, None],
                    )
                })?;
                let string = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
                Some(Rc::new(Package {
                    name: string("name"),
                    main: string("main"),
                    module_type: string("type").filter(|t| t == "commonjs" || t == "module"),
                    exports: value.get("exports").filter(|v| !v.is_null()).cloned(),
                    imports: value.get("imports").filter(|v| !v.is_null()).cloned(),
                    path,
                }))
            }
        };
        self.packages.insert(dir.to_path_buf(), package.clone());
        Ok(package)
    }

    /// The nearest package.json above `file` for CommonJS (TraverseParent in
    /// node_modules.cc): its parents, not the filesystem root, stopping at a
    /// node_modules directory.
    pub fn nearest_package(&mut self, file: &Path) -> Result<Option<Rc<Package>>, Failure> {
        let mut current = file.to_path_buf();
        while current.pop() {
            if current.parent().is_none() {
                break;
            }
            if current
                .file_name()
                .is_some_and(|name| name == "node_modules")
            {
                return Ok(None);
            }
            if let Some(package) = self.package(&current)? {
                return Ok(Some(package));
            }
        }
        Ok(None)
    }

    /// The package scope of `file` for ES modules (GetPackageScopeConfig):
    /// the nearest package.json, the root's included, stopping at a
    /// node_modules directory. Decides whether a .js file is an ES module.
    pub fn package_scope(&mut self, file: &Path) -> Result<Option<Rc<Package>>, Failure> {
        let mut current = file.to_path_buf();
        while current.pop() {
            if current
                .file_name()
                .is_some_and(|name| name == "node_modules")
            {
                return Ok(None);
            }
            if let Some(package) = self.package(&current)? {
                return Ok(Some(package));
            }
            if current.parent().is_none() {
                break;
            }
        }
        Ok(None)
    }

    // ---- require -----------------------------------------------------------

    /// What `require(request)` loads when called from the module at `parent`
    /// (None for the main module and other parentless calls). Builtins are
    /// the caller's: this is only called for requests that aren't one.
    pub fn require(
        &mut self,
        request: &str,
        parent: Option<&Path>,
        is_main: bool,
    ) -> Result<PathBuf, Failure> {
        let conditions = REQUIRE_CONDITIONS;
        let paths = self.lookup_paths(request, parent);

        if request.starts_with('#')
            && let Some(parent) = parent
            && let Some(package) = self.nearest_package(parent)?
            && package.imports.is_some()
        {
            return match self.package_imports_resolve(request, parent, conditions) {
                Ok(target) => self.finalize_esm(&target, Some(parent), Some(package.path.clone())),
                Err(Failure::Node {
                    code: "ERR_MODULE_NOT_FOUND",
                    ..
                }) => Err(Failure::EsmNotFound {
                    request: request.to_string(),
                    path: None,
                }),
                Err(other) => Err(other),
            };
        }

        if let Some(resolved) = self.try_self(parent, request, conditions)? {
            return Ok(resolved);
        }

        match self.find_path(request, &paths, is_main, conditions)? {
            Some(found) => Ok(found),
            None => Err(Failure::NotFound),
        }
    }

    /// Module._resolveLookupPaths: where a request is looked for.
    fn lookup_paths(&mut self, request: &str, parent: Option<&Path>) -> Vec<PathBuf> {
        let bytes = request.as_bytes();
        let relative_like = bytes.first() == Some(&b'.')
            && (bytes.len() == 1 || bytes[1] == b'.' || bytes[1] == b'/');
        if !relative_like {
            let mut paths = match parent {
                Some(parent) => node_module_paths(parent.parent().unwrap_or(Path::new("/"))),
                None => Vec::new(),
            };
            paths.extend(self.global_paths.iter().cloned());
            return paths;
        }
        match parent {
            Some(parent) => vec![parent.parent().unwrap_or(Path::new("/")).to_path_buf()],
            None => vec![PathBuf::from(".")],
        }
    }

    /// trySelf: a package requiring itself by name, through its `exports`.
    fn try_self(
        &mut self,
        parent: Option<&Path>,
        request: &str,
        conditions: &[&str],
    ) -> Result<Option<PathBuf>, Failure> {
        let Some(parent) = parent else {
            return Ok(None);
        };
        let Some(package) = self.nearest_package(parent)? else {
            return Ok(None);
        };
        let (Some(exports), Some(name)) = (&package.exports, &package.name) else {
            return Ok(None);
        };
        let expansion = if request == name {
            ""
        } else if let Some(rest) = request.strip_prefix(name.as_str())
            && rest.starts_with('/')
        {
            rest
        } else {
            return Ok(None);
        };
        let package_dir = package.path.parent().unwrap().to_path_buf();
        let subpath = format!(".{expansion}");
        match self.package_exports_resolve(
            &package_dir,
            &subpath,
            exports,
            Some(parent),
            conditions,
        ) {
            Ok(target) => self
                .finalize_esm(&target, Some(parent), Some(package.path.clone()))
                .map(Some),
            Err(Failure::Node {
                code: "ERR_MODULE_NOT_FOUND",
                ..
            }) => Err(Failure::EsmNotFound {
                request: request.to_string(),
                path: Some(package.path.clone()),
            }),
            Err(other) => Err(other),
        }
    }

    /// Module._findPath.
    fn find_path(
        &mut self,
        request: &str,
        paths: &[PathBuf],
        is_main: bool,
        conditions: &[&str],
    ) -> Result<Option<PathBuf>, Failure> {
        let absolute = request.starts_with('/');
        let paths: Vec<PathBuf> = if absolute {
            vec![PathBuf::new()]
        } else {
            paths.to_vec()
        };
        if paths.is_empty() {
            return Ok(None);
        }
        let trailing_slash = has_trailing_slash(request);
        let inside_path =
            !(is_relative(request) && normalize(Path::new(request)).starts_with(".."));
        for current in &paths {
            if inside_path && !current.as_os_str().is_empty() && self.stat(current) != Stat::Dir {
                continue;
            }
            if !absolute
                && let Some(resolved) = self.resolve_exports(current, request, conditions)?
            {
                return Ok(Some(resolved));
            }
            let base = resolve(current, request);
            let rc = self.stat(&base);
            let mut found = None;
            if !trailing_slash {
                if rc == Stat::File {
                    found = Some(self.real_path(&base));
                }
                if found.is_none() {
                    found = self.try_extensions(&base, is_main);
                }
            }
            if found.is_none() && rc == Stat::Dir {
                found = self.try_package(&base, is_main)?;
            }
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }

    fn try_file(&mut self, path: &Path) -> Option<PathBuf> {
        (self.stat(path) == Stat::File).then(|| self.real_path(path))
    }

    /// `.js`, `.json`, `.node`: Module._extensions' keys, in order.
    fn try_extensions(&mut self, base: &Path, _is_main: bool) -> Option<PathBuf> {
        for extension in [".js", ".json", ".node"] {
            let mut candidate = base.as_os_str().to_owned();
            candidate.push(extension);
            if let Some(found) = self.try_file(Path::new(&candidate)) {
                return Some(found);
            }
        }
        None
    }

    /// tryPackage: a directory, by its package.json's `main`, then index.
    fn try_package(&mut self, dir: &Path, is_main: bool) -> Result<Option<PathBuf>, Failure> {
        let package = self.package(dir)?;
        let main = package
            .as_ref()
            .and_then(|p| p.main.clone())
            .filter(|m| !m.is_empty());
        let Some(main) = main else {
            return Ok(self.try_extensions(&dir.join("index"), is_main));
        };
        let filename = resolve(dir, &main);
        let actual = self
            .try_file(&filename)
            .or_else(|| self.try_extensions(&filename, is_main))
            .or_else(|| self.try_extensions(&filename.join("index"), is_main));
        if actual.is_some() {
            return Ok(actual);
        }
        match self.try_extensions(&dir.join("index"), is_main) {
            Some(index) => {
                let package_json = package.as_ref().unwrap().path.clone();
                self.warnings.push(Warning {
                    message: format!(
                        "Invalid 'main' field in '{}' of '{main}'. Please either fix that or report it to the module author",
                        display(&package_json)
                    ),
                    kind: "DeprecationWarning",
                    code: "DEP0128",
                });
                Ok(Some(index))
            }
            None => Err(Failure::BadMain {
                filename,
                package_json: package.unwrap().path.clone(),
            }),
        }
    }

    /// resolveExports: `pkg/sub` through `node_modules/pkg`'s `exports`.
    fn resolve_exports(
        &mut self,
        nm_path: &Path,
        request: &str,
        conditions: &[&str],
    ) -> Result<Option<PathBuf>, Failure> {
        let Some((name, expansion)) = split_exports_request(request) else {
            return Ok(None);
        };
        let package_dir = resolve(nm_path, name);
        let Some(package) = self.package(&package_dir)? else {
            return Ok(None);
        };
        let Some(exports) = &package.exports else {
            return Ok(None);
        };
        let subpath = format!(".{expansion}");
        match self.package_exports_resolve(&package_dir, &subpath, exports, None, conditions) {
            Ok(target) => self
                .finalize_esm(&target, None, Some(package_dir.clone()))
                .map(Some),
            Err(Failure::Node {
                code: "ERR_MODULE_NOT_FOUND",
                ..
            }) => Err(Failure::EsmNotFound {
                request: request.to_string(),
                path: Some(package_dir.join("package.json")),
            }),
            Err(other) => Err(other),
        }
    }

    /// finalizeEsmResolution: what an `exports`/`imports` target named, as a
    /// file that exists (by its real path).
    fn finalize_esm(
        &mut self,
        target: &UrlPath,
        parent: Option<&Path>,
        error_path: Option<PathBuf>,
    ) -> Result<PathBuf, Failure> {
        if target.has_encoded_separator() {
            return Err(node(
                "ERR_INVALID_MODULE_SPECIFIER",
                vec![
                    some(target.href()),
                    some("must not include encoded \"/\" or \"\\\" characters"),
                    parent.map(display),
                ],
            ));
        }
        let filename = target.to_path();
        match self.try_file(&filename) {
            Some(found) => Ok(found),
            None => Err(Failure::EsmNotFound {
                request: display(&filename),
                path: error_path,
            }),
        }
    }

    // ---- exports and imports ---------------------------------------------

    /// packageExportsResolve: `subpath` (`.` or `./x`) through `exports`.
    fn package_exports_resolve(
        &mut self,
        package_dir: &Path,
        subpath: &str,
        exports: &Value,
        base: Option<&Path>,
        conditions: &[&str],
    ) -> Result<UrlPath, Failure> {
        let package_json = package_dir.join("package.json");
        let sugar = is_conditional_sugar(exports, &package_json, base)?;
        let wrapped;
        let exports = if sugar {
            wrapped = serde_json::json!({ ".": exports });
            &wrapped
        } else {
            exports
        };
        let map = exports
            .as_object()
            .expect("exports is an object or sugar for one");
        if let Some(target) = map.get(subpath)
            && !subpath.contains('*')
            && !subpath.ends_with('/')
        {
            return self
                .resolve_target(
                    package_dir,
                    target,
                    "",
                    subpath,
                    base,
                    false,
                    false,
                    false,
                    conditions,
                )?
                .found()
                .ok_or_else(|| exports_not_found(subpath, package_dir, base));
        }
        let (best, best_subpath) = best_pattern_match(map, subpath);
        if let Some(best) = best {
            if subpath.ends_with('/') {
                self.warnings.push(Warning {
                    message: format!(
                        "Use of deprecated trailing slash pattern mapping \"{subpath}\" in the \"exports\" field module resolution of the package at {}{}. Mapping specifiers ending in \"/\" is no longer supported.",
                        display(&package_json),
                        base.map(|b| format!(" imported from {}", display(b))).unwrap_or_default()
                    ),
                    kind: "DeprecationWarning",
                    code: "DEP0155",
                });
            }
            let target = &map[&best];
            return self
                .resolve_target(
                    package_dir,
                    target,
                    &best_subpath,
                    &best,
                    base,
                    true,
                    false,
                    subpath.ends_with('/'),
                    conditions,
                )?
                .found()
                .ok_or_else(|| exports_not_found(subpath, package_dir, base));
        }
        Err(exports_not_found(subpath, package_dir, base))
    }

    /// packageImportsResolve: a `#name` through the package's `imports`.
    fn package_imports_resolve(
        &mut self,
        name: &str,
        base: &Path,
        conditions: &[&str],
    ) -> Result<UrlPath, Failure> {
        if name == "#" || name.ends_with('/') {
            return Err(node(
                "ERR_INVALID_MODULE_SPECIFIER",
                vec![
                    some(name),
                    some("is not a valid internal imports specifier name"),
                    some(display(base)),
                ],
            ));
        }
        let package = self.package_scope(base)?;
        let mut package_dir = None;
        if let Some(package) = &package {
            let dir = package.path.parent().unwrap().to_path_buf();
            package_dir = Some(dir.clone());
            if let Some(Value::Object(imports)) = &package.imports {
                if let Some(target) = imports.get(name)
                    && !name.contains('*')
                {
                    if let Some(found) = self
                        .resolve_target(
                            &dir,
                            target,
                            "",
                            name,
                            Some(base),
                            false,
                            true,
                            false,
                            conditions,
                        )?
                        .found()
                    {
                        return Ok(found);
                    }
                } else {
                    let (best, best_subpath) = best_pattern_match(imports, name);
                    if let Some(best) = best
                        && let Some(found) = self
                            .resolve_target(
                                &dir,
                                &imports[&best],
                                &best_subpath,
                                &best,
                                Some(base),
                                true,
                                true,
                                false,
                                conditions,
                            )?
                            .found()
                    {
                        return Ok(found);
                    }
                }
            }
        }
        Err(node(
            "ERR_PACKAGE_IMPORT_NOT_DEFINED",
            vec![
                some(name),
                package_dir.map(|d| with_slash(&d)),
                some(display(base)),
            ],
        ))
    }

    /// resolvePackageTarget: one `exports`/`imports` value, which may be a
    /// string, an array of fallbacks, a conditions object, or null. Node
    /// tells "no condition matched" (undefined: try the next condition)
    /// from "blocked" (null: stop), and so does [`Target`].
    #[allow(clippy::too_many_arguments)]
    fn resolve_target(
        &mut self,
        package_dir: &Path,
        target: &Value,
        subpath: &str,
        key: &str,
        base: Option<&Path>,
        pattern: bool,
        internal: bool,
        is_path_map: bool,
        conditions: &[&str],
    ) -> Result<Target, Failure> {
        match target {
            Value::String(target) => self
                .resolve_target_string(
                    target,
                    subpath,
                    key,
                    package_dir,
                    base,
                    pattern,
                    internal,
                    is_path_map,
                    conditions,
                )
                .map(Target::Found),
            Value::Array(targets) => {
                if targets.is_empty() {
                    return Ok(Target::Null);
                }
                // undefined, null, or the last invalid-target error, as in
                // Node's loop over the fallbacks.
                let mut last: Option<Result<Target, Failure>> = None;
                for target in targets {
                    match self.resolve_target(
                        package_dir,
                        target,
                        subpath,
                        key,
                        base,
                        pattern,
                        internal,
                        is_path_map,
                        conditions,
                    ) {
                        Ok(Target::Found(found)) => return Ok(Target::Found(found)),
                        Ok(Target::Undefined) => {}
                        Ok(Target::Null) => last = Some(Ok(Target::Null)),
                        Err(
                            error @ Failure::Node {
                                code: "ERR_INVALID_PACKAGE_TARGET",
                                ..
                            },
                        ) => last = Some(Err(error)),
                        Err(other) => return Err(other),
                    }
                }
                last.unwrap_or(Ok(Target::Undefined))
            }
            Value::Object(map) => {
                if map.keys().any(|condition| is_array_index(condition)) {
                    return Err(node(
                        "ERR_INVALID_PACKAGE_CONFIG",
                        vec![
                            some(display(&package_dir.join("package.json"))),
                            base.map(display),
                            some("\"exports\" cannot contain numeric property keys."),
                        ],
                    ));
                }
                for (condition, value) in map {
                    if condition == "default" || conditions.contains(&condition.as_str()) {
                        match self.resolve_target(
                            package_dir,
                            value,
                            subpath,
                            key,
                            base,
                            pattern,
                            internal,
                            is_path_map,
                            conditions,
                        )? {
                            Target::Undefined => continue,
                            other => return Ok(other),
                        }
                    }
                }
                Ok(Target::Undefined)
            }
            Value::Null => Ok(Target::Null),
            other => Err(invalid_target(
                package_dir,
                key,
                &other.to_string(),
                internal,
                base,
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_target_string(
        &mut self,
        target: &str,
        subpath: &str,
        key: &str,
        package_dir: &Path,
        base: Option<&Path>,
        pattern: bool,
        internal: bool,
        is_path_map: bool,
        conditions: &[&str],
    ) -> Result<UrlPath, Failure> {
        // Node passes a string target as it is (`${target}`); the message
        // JSON-quotes it.
        let invalid = || invalid_target(package_dir, key, target, internal, base);
        if !subpath.is_empty() && !pattern && !target.ends_with('/') {
            return Err(invalid());
        }
        if !target.starts_with("./") {
            if internal
                && !target.starts_with('/')
                && !target.starts_with("../")
                && !can_parse_url(target)
            {
                let export_target = if pattern {
                    target.replace('*', subpath)
                } else {
                    format!("{target}{subpath}")
                };
                let base_file = package_dir.join("package.json");
                return match self.package_resolve(&export_target, &base_file, conditions)? {
                    Resolved::File(path) => Ok(UrlPath::from_path(&path)),
                    Resolved::Builtin(name) => Ok(UrlPath::builtin(&name)),
                    Resolved::Url(url) => Ok(UrlPath::other(&url)),
                };
            }
            return Err(invalid());
        }
        let rest = &target[2..];
        if has_invalid_segment(rest) {
            if !has_deprecated_invalid_segment(rest) {
                if !is_path_map {
                    let request = if pattern {
                        key.replacen('*', subpath, 1)
                    } else {
                        format!("{key}{subpath}")
                    };
                    let resolved = if pattern {
                        target.replace('*', subpath)
                    } else {
                        target.to_string()
                    };
                    self.invalid_segment_warning(
                        &resolved,
                        &request,
                        key,
                        package_dir,
                        internal,
                        base,
                        true,
                    );
                }
            } else {
                return Err(invalid());
            }
        }
        let package_url = UrlPath::from_dir(package_dir);
        let resolved = package_url.join(target);
        if !resolved.path.starts_with(&package_url.path) {
            return Err(invalid());
        }
        if subpath.is_empty() {
            return Ok(resolved);
        }
        if has_invalid_segment(subpath) {
            let request = if pattern {
                key.replacen('*', subpath, 1)
            } else {
                format!("{key}{subpath}")
            };
            if !has_deprecated_invalid_segment(subpath) {
                if !is_path_map {
                    let resolved = if pattern {
                        target.replace('*', subpath)
                    } else {
                        target.to_string()
                    };
                    self.invalid_segment_warning(
                        &resolved,
                        &request,
                        key,
                        package_dir,
                        internal,
                        base,
                        false,
                    );
                }
            } else {
                let reason = format!(
                    "request is not a valid match in pattern \"{key}\" for the \"{}\" resolution of {}",
                    if internal { "imports" } else { "exports" },
                    display(&package_dir.join("package.json"))
                );
                return Err(node(
                    "ERR_INVALID_MODULE_SPECIFIER",
                    vec![some(request), some(reason), base.map(display)],
                ));
            }
        }
        if pattern {
            return Ok(UrlPath::parse_href(&resolved.href().replace('*', subpath)));
        }
        Ok(resolved.join(subpath))
    }

    #[allow(clippy::too_many_arguments)]
    fn invalid_segment_warning(
        &mut self,
        target: &str,
        request: &str,
        key: &str,
        package_dir: &Path,
        internal: bool,
        base: Option<&Path>,
        is_target: bool,
    ) {
        let double = if is_target { target } else { request }.contains("//");
        self.warnings.push(Warning {
            message: format!(
                "Use of deprecated {} resolving \"{target}\" for module request \"{request}\" {}in the \"{}\" field module resolution of the package at {}{}.",
                if double { "double slash" } else { "leading or trailing slash matching" },
                if request != key { format!("matched to \"{key}\" ") } else { String::new() },
                if internal { "imports" } else { "exports" },
                display(&package_dir.join("package.json")),
                base.map(|b| format!(" imported from {}", display(b))).unwrap_or_default()
            ),
            kind: "DeprecationWarning",
            code: "DEP0166",
        });
    }

    // ---- import ------------------------------------------------------------

    /// What `import(specifier)` names from the module at `parent`
    /// (moduleResolve, with finalizeResolution).
    pub fn import(&mut self, specifier: &str, parent: &Path) -> Result<Resolved, Failure> {
        match self.module_resolve(specifier, parent) {
            Err(Failure::Node {
                code: code @ ("ERR_MODULE_NOT_FOUND" | "ERR_UNSUPPORTED_DIR_IMPORT"),
                args,
                ..
            }) => {
                let hint = self.commonjs_hint(specifier, parent);
                Err(Failure::Node { code, args, hint })
            }
            other => other,
        }
    }

    /// decorateErrorWithCommonJSHints: what require() would have found, as
    /// a suggestion, if it differs from what was asked for.
    fn commonjs_hint(&mut self, specifier: &str, parent: &Path) -> Option<String> {
        let specifier = specifier
            .strip_prefix("file://")
            .map_or(specifier, |rest| rest);
        // resolveAsCommonJS makes a parent module with no filename, so
        // require's self-reference and `#imports` don't apply, a relative
        // specifier is looked for from the working directory (Node's
        // lookup for a parent without a filename), and node_modules is
        // searched from the parent's own path.
        let warnings = self.warnings.len();
        let paths = if is_relative(specifier) || specifier == "." || specifier == ".." {
            vec![PathBuf::from(".")]
        } else {
            let mut paths = node_module_paths(parent);
            paths.extend(self.global_paths.iter().cloned());
            paths
        };
        let found = self.find_path(specifier, &paths, false, REQUIRE_CONDITIONS);
        self.warnings.truncate(warnings);
        let found = found.ok()??;
        let found = if is_relative(specifier) || specifier == "." || specifier == ".." {
            let from = UrlPath::from_path(parent).path;
            let from = &from[..from.rfind('/').unwrap_or(0)];
            let to = UrlPath::from_path(&found).path;
            let relative = posix_relative(from, &to);
            if relative.starts_with("../") {
                relative
            } else {
                format!("./{relative}")
            }
        } else if !specifier.starts_with('/')
            && !specifier.starts_with('.')
            && !specifier.is_empty()
        {
            let package = specifier.split('/').next().unwrap_or(specifier);
            let needle = format!("/node_modules/{package}/");
            let text = display(&found);
            match text.rfind(&needle) {
                Some(index) => {
                    let inside = &text[index + needle.len()..];
                    let encoded: Vec<String> =
                        inside.split('/').map(encode_uri_component).collect();
                    format!("{package}/{}", encoded.join("/"))
                }
                None => UrlPath::from_path(&found).href(),
            }
        } else {
            display(&found)
        };
        (found != specifier).then(|| format!("Did you mean to import {}?", Value::String(found)))
    }

    fn module_resolve(&mut self, specifier: &str, parent: &Path) -> Result<Resolved, Failure> {
        let conditions = IMPORT_CONDITIONS;
        let resolved = if is_relative_or_absolute(specifier) {
            UrlPath::from_path(parent).join(specifier)
        } else if specifier.starts_with('#') {
            self.package_imports_resolve(specifier, parent, conditions)?
        } else if let Some(url) = absolute_url(specifier) {
            url
        } else {
            match self.package_resolve(specifier, parent, conditions)? {
                Resolved::File(path) => UrlPath::from_path(&path),
                other => return Ok(other),
            }
        };
        if let Some(name) = &resolved.builtin {
            return Ok(Resolved::Builtin(name.clone()));
        }
        if let Some(url) = &resolved.other {
            return Ok(Resolved::Url(url.clone()));
        }
        self.finalize_resolution(&resolved, parent)
            .map(Resolved::File)
    }

    /// finalizeResolution: the file an import resolved to, by its real path.
    fn finalize_resolution(&mut self, resolved: &UrlPath, base: &Path) -> Result<PathBuf, Failure> {
        if resolved.has_encoded_separator() {
            return Err(node(
                "ERR_INVALID_MODULE_SPECIFIER",
                vec![
                    some(resolved.path.clone()),
                    some("must not include encoded \"/\" or \"\\\" characters"),
                    some(display(base)),
                ],
            ));
        }
        let path = resolved.to_path();
        // Node stats `path.slice(-1)` for a path ending in `/`: its last
        // character, `/` itself, which is a directory, so every such
        // specifier is a directory import.
        let shown = display(&path);
        let stat = if shown.ends_with('/') {
            Stat::Dir
        } else {
            self.stat(&path)
        };
        match stat {
            Stat::Dir => Err(node(
                "ERR_UNSUPPORTED_DIR_IMPORT",
                vec![
                    some(display(&path)),
                    some(display(base)),
                    some(resolved.href()),
                ],
            )),
            Stat::Missing => Err(node(
                "ERR_MODULE_NOT_FOUND",
                vec![
                    some(display(&path)),
                    some(display(base)),
                    some(resolved.href()),
                ],
            )),
            Stat::File => Ok(self.real_path(&path)),
        }
    }

    /// packageResolve: a bare specifier, from `base`.
    fn package_resolve(
        &mut self,
        specifier: &str,
        base: &Path,
        conditions: &[&str],
    ) -> Result<Resolved, Failure> {
        let (name, subpath) = parse_package_name(specifier, base)?;
        if let Some(scope) = self.package_scope(base)?
            && let Some(exports) = &scope.exports
            && scope.name.as_deref() == Some(name.as_str())
        {
            let dir = scope.path.parent().unwrap().to_path_buf();
            let exports = exports.clone();
            return self
                .package_exports_resolve(&dir, &subpath, &exports, Some(base), conditions)
                .map(|u| u.into_resolved());
        }
        let mut dir = base.parent().unwrap_or(Path::new("/")).to_path_buf();
        loop {
            let candidate = dir.join("node_modules").join(&name);
            if self.stat(&candidate) == Stat::Dir {
                let package = self.package(&candidate)?;
                let package = package.unwrap_or_default_at(&candidate);
                if let Some(exports) = &package.exports {
                    let exports = exports.clone();
                    return self
                        .package_exports_resolve(
                            &candidate,
                            &subpath,
                            &exports,
                            Some(base),
                            conditions,
                        )
                        .map(|u| u.into_resolved());
                }
                if subpath == "." {
                    let main = package.main.clone();
                    return self.legacy_main_resolve(&candidate, main.as_deref(), base);
                }
                return Ok(Resolved::File(
                    UrlPath::from_dir(&candidate).join(&subpath).to_path(),
                ));
            }
            // Up one: `../../../node_modules/<name>` from where the last
            // candidate's package.json would have been (`../` more for a
            // scope), which is the parent directory, node_modules or not.
            if !dir.pop() {
                break;
            }
        }
        Err(node(
            "ERR_MODULE_NOT_FOUND",
            vec![some(name), some(display(base)), None],
        ))
    }

    /// legacyMainResolve: a package with no `exports`, imported by name.
    fn legacy_main_resolve(
        &mut self,
        package_dir: &Path,
        main: Option<&str>,
        base: &Path,
    ) -> Result<Resolved, Failure> {
        if let Some(main) = main {
            for suffix in [
                "",
                ".js",
                ".json",
                ".node",
                "/index.js",
                "/index.json",
                "/index.node",
            ] {
                let candidate = resolve(package_dir, &format!("{main}{suffix}"));
                if self.stat(&candidate) == Stat::File {
                    if !suffix.is_empty() {
                        self.legacy_index_warning(&candidate, package_dir, base, Some(main));
                    }
                    return Ok(Resolved::File(candidate));
                }
            }
        }
        for index in ["index.js", "index.json", "index.node"] {
            let candidate = package_dir.join(index);
            if self.stat(&candidate) == Stat::File {
                self.legacy_index_warning(&candidate, package_dir, base, main);
                return Ok(Resolved::File(candidate));
            }
        }
        // FSLegacyMainResolve names the file it started from: `main`, or
        // index.js without one.
        let initial = match main {
            Some(main) => resolve(package_dir, main),
            None => package_dir.join("index.js"),
        };
        Err(node(
            "ERR_MODULE_NOT_FOUND",
            vec![some(display(&initial)), some(display(base)), None],
        ))
    }

    /// Whether `path` is an ES module by its name alone: `.mjs`, or `.js`
    /// in a package scope of type "module" (defaultGetFormatWithoutErrors).
    fn is_module_by_name(&mut self, path: &Path) -> bool {
        match path.extension().and_then(|e| e.to_str()) {
            Some("mjs") => true,
            Some("js") => matches!(
                self.package_scope(path),
                Ok(Some(scope)) if scope.module_type.as_deref() == Some("module")
            ),
            _ => false,
        }
    }

    /// DEP0151, which Node emits only for an index or extension it found
    /// for an ES module.
    fn legacy_index_warning(
        &mut self,
        resolved: &Path,
        package_dir: &Path,
        base: &Path,
        main: Option<&str>,
    ) {
        if !self.is_module_by_name(resolved) {
            return;
        }
        let path = display(resolved);
        let package_path = with_slash(package_dir);
        let relative = path.strip_prefix(&package_path).unwrap_or(&path);
        let message = match main {
            Some(main) => format!(
                "Package {package_path} has a \"main\" field set to \"{main}\", excluding the full filename and extension to the resolved file at \"{relative}\", imported from {}.\n Automatic extension resolution of the \"main\" field is deprecated for ES modules.",
                display(base)
            ),
            None => format!(
                "No \"main\" or \"exports\" field defined in the package.json for {package_path} resolving the main entry point \"{relative}\", imported from {}.\nDefault \"index\" lookups for the main are deprecated for ES modules.",
                display(base)
            ),
        };
        self.warnings.push(Warning {
            message,
            kind: "DeprecationWarning",
            code: "DEP0151",
        });
    }
}

trait OrDefaultAt {
    fn unwrap_or_default_at(self, dir: &Path) -> Rc<Package>;
}

impl OrDefaultAt for Option<Rc<Package>> {
    fn unwrap_or_default_at(self, dir: &Path) -> Rc<Package> {
        self.unwrap_or_else(|| {
            Rc::new(Package {
                path: dir.join("package.json"),
                ..Package::default()
            })
        })
    }
}

/// What resolvePackageTarget returns: a URL, undefined (nothing matched),
/// or null (an explicit block).
enum Target {
    Found(UrlPath),
    Undefined,
    Null,
}

impl Target {
    fn found(self) -> Option<UrlPath> {
        match self {
            Target::Found(found) => Some(found),
            _ => None,
        }
    }
}

/// An `exports` or `imports` resolution in progress: a file URL's path,
/// percent-encoded and with `.` and `..` resolved as the URL parser resolves
/// them, or a URL of another kind.
#[derive(Clone, Debug)]
struct UrlPath {
    path: String,
    builtin: Option<String>,
    other: Option<String>,
}

impl UrlPath {
    fn from_path(path: &Path) -> Self {
        Self {
            path: encode_path(&display(path)),
            builtin: None,
            other: None,
        }
    }

    fn from_dir(dir: &Path) -> Self {
        Self::from_path(Path::new(&with_slash(dir)))
    }

    fn builtin(name: &str) -> Self {
        Self {
            path: String::new(),
            builtin: Some(name.to_string()),
            other: None,
        }
    }

    fn other(url: &str) -> Self {
        Self {
            path: String::new(),
            builtin: None,
            other: Some(url.to_string()),
        }
    }

    fn href(&self) -> String {
        format!("file://{}", self.path)
    }

    fn parse_href(href: &str) -> Self {
        let path = href.strip_prefix("file://").unwrap_or(href);
        let mut url = Self::from_path(Path::new("/"));
        url.path = String::new();
        url.join(path)
    }

    /// `new URL(relative, this)` for a path-only reference: `\` is `/`, a
    /// query or fragment ends the path, `.` and `..` segments (and their
    /// percent-encoded spellings) are resolved, and characters a URL path
    /// can't hold are percent-encoded.
    fn join(&self, relative: &str) -> Self {
        let relative = relative.replace('\\', "/");
        let relative = relative.split(['?', '#']).next().unwrap_or_default();
        let mut segments: Vec<String> = if relative.starts_with('/') {
            Vec::new()
        } else {
            let base = &self.path[..self.path.rfind('/').map_or(0, |i| i + 1)];
            base.split('/')
                .skip(1)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };
        let parts: Vec<&str> = relative.split('/').collect();
        let mut trailing = false;
        for (i, part) in parts.iter().enumerate() {
            let last = i == parts.len() - 1;
            let lower = part.to_ascii_lowercase();
            if lower == "." || lower == "%2e" {
                trailing = last;
            } else if matches!(lower.as_str(), ".." | ".%2e" | "%2e." | "%2e%2e") {
                segments.pop();
                trailing = last;
            } else if part.is_empty() && i == 0 && relative.starts_with('/') {
                continue;
            } else {
                segments.push(encode_url_segment(part));
                trailing = false;
            }
        }
        let mut path = format!("/{}", segments.join("/"));
        if trailing && !path.ends_with('/') {
            path.push('/');
        }
        Self {
            path,
            builtin: None,
            other: None,
        }
    }

    fn has_encoded_separator(&self) -> bool {
        let lower = self.path.to_ascii_lowercase();
        lower.contains("%2f") || lower.contains("%5c")
    }

    /// fileURLToPath.
    fn to_path(&self) -> PathBuf {
        PathBuf::from(percent_decode(&self.path))
    }

    fn into_resolved(self) -> Resolved {
        if let Some(name) = self.builtin {
            return Resolved::Builtin(name);
        }
        if let Some(url) = self.other {
            return Resolved::Url(url);
        }
        Resolved::File(self.to_path())
    }
}

/// Percent-encodes what the WHATWG URL parser encodes in a path: C0
/// controls, space, `"`, `#`, `<`, `>`, `?`, `` ` ``, `{`, `}`, and bytes
/// outside ASCII; and `%` itself, which url.pathToFileURL encodes so that a
/// path's `%` survives.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'%' => out.push_str("%25"),
            0..=0x20 | b'"' | b'#' | b'<' | b'>' | b'?' | b'`' | b'{' | b'}' | 0x7F..=0xFF => {
                out.push_str(&format!("%{byte:02X}"));
            }
            _ => out.push(byte as char),
        }
    }
    out
}

/// What the URL parser encodes in a path segment of a relative reference:
/// the same set as [`encode_path`] but not `%`, which the parser leaves as
/// it is (so `%20` in a specifier is a space once decoded).
fn encode_url_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            0..=0x20 | b'"' | b'#' | b'<' | b'>' | b'?' | b'`' | b'{' | b'}' | 0x7F..=0xFF => {
                out.push_str(&format!("%{byte:02X}"));
            }
            _ => out.push(byte as char),
        }
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(a), Some(b)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(a * 16 + b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// encodeURIComponent.
fn encode_uri_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// path.posix.relative(from, to), for absolute paths.
fn posix_relative(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

fn hex(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|d| d as u8)
}

fn with_slash(dir: &Path) -> String {
    let text = display(dir);
    if text.ends_with('/') {
        text
    } else {
        format!("{text}/")
    }
}

fn exports_not_found(subpath: &str, package_dir: &Path, base: Option<&Path>) -> Failure {
    node(
        "ERR_PACKAGE_PATH_NOT_EXPORTED",
        vec![
            some(with_slash(package_dir)),
            some(subpath),
            base.map(display),
        ],
    )
}

fn invalid_target(
    package_dir: &Path,
    key: &str,
    target: &str,
    internal: bool,
    base: Option<&Path>,
) -> Failure {
    node(
        "ERR_INVALID_PACKAGE_TARGET",
        vec![
            some(with_slash(package_dir)),
            some(key),
            some(target),
            some(if internal { "true" } else { "false" }),
            base.map(display),
        ],
    )
}

/// isConditionalExportsMainSugar.
fn is_conditional_sugar(
    exports: &Value,
    package_json: &Path,
    base: Option<&Path>,
) -> Result<bool, Failure> {
    let map = match exports {
        Value::String(_) | Value::Array(_) => return Ok(true),
        Value::Object(map) => map,
        _ => return Ok(false),
    };
    let mut sugar = false;
    for (i, key) in map.keys().enumerate() {
        let current = key.is_empty() || !key.starts_with('.');
        if i == 0 {
            sugar = current;
        } else if sugar != current {
            return Err(node(
                "ERR_INVALID_PACKAGE_CONFIG",
                vec![
                    some(display(package_json)),
                    base.map(display),
                    some(
                        "\"exports\" cannot contain some keys starting with '.' and some not. The exports object must either be an object of package subpath keys or an object of main entry condition name keys only.",
                    ),
                ],
            ));
        }
    }
    Ok(sugar)
}

/// The `*` key that matches `request` best (patternKeyCompare's order), and
/// what its `*` stands for.
fn best_pattern_match(
    map: &serde_json::Map<String, Value>,
    request: &str,
) -> (Option<String>, String) {
    let mut best: Option<String> = None;
    let mut best_subpath = String::new();
    for key in map.keys() {
        let Some(star) = key.find('*') else {
            continue;
        };
        if !request.starts_with(&key[..star]) {
            continue;
        }
        let trailer = &key[star + 1..];
        if request.len() >= key.len()
            && request.ends_with(trailer)
            && pattern_key_compare(best.as_deref().unwrap_or(""), key) == 1
            && key.rfind('*') == Some(star)
        {
            best = Some(key.clone());
            best_subpath = request[star..request.len() - trailer.len()].to_string();
        }
    }
    (best, best_subpath)
}

fn pattern_key_compare(a: &str, b: &str) -> i32 {
    let a_star = a.find('*');
    let b_star = b.find('*');
    let base_a = a_star.map_or(a.len(), |i| i + 1);
    let base_b = b_star.map_or(b.len(), |i| i + 1);
    if base_a > base_b {
        return -1;
    }
    if base_b > base_a {
        return 1;
    }
    if a_star.is_none() {
        return 1;
    }
    if b_star.is_none() {
        return -1;
    }
    if a.len() > b.len() {
        return -1;
    }
    if b.len() > a.len() {
        return 1;
    }
    0
}

fn is_array_index(key: &str) -> bool {
    key.parse::<u64>()
        .is_ok_and(|n| n.to_string() == key && n < 0xFFFF_FFFF)
}

/// One segment matched by invalidSegmentRegEx's middle part: `.`, `..`,
/// `node_modules`, in any case and with any of their letters
/// percent-encoded.
fn is_named_invalid_segment(segment: &str) -> bool {
    // Only the spellings the regex allows: each letter literal or %XX.
    let literal_or_encoded = |word: &str| {
        let mut rest = segment.to_ascii_lowercase();
        for c in word.chars() {
            let encoded = format!("%{:02x}", c as u8);
            if let Some(after) = rest.strip_prefix(c) {
                rest = after.to_string();
            } else if let Some(after) = rest.strip_prefix(&encoded) {
                rest = after.to_string();
            } else {
                return false;
            }
        }
        rest.is_empty()
    };
    literal_or_encoded(".") || literal_or_encoded("..") || literal_or_encoded("node_modules")
}

fn segments(text: &str) -> impl Iterator<Item = &str> {
    text.split(['/', '\\'])
}

/// invalidSegmentRegEx: an empty segment, or a named one.
fn has_invalid_segment(text: &str) -> bool {
    segments(text).any(|s| s.is_empty() || is_named_invalid_segment(s))
}

/// deprecatedInvalidSegmentRegEx: a named invalid segment (not just an
/// empty one, which is only deprecated).
fn has_deprecated_invalid_segment(text: &str) -> bool {
    segments(text).any(is_named_invalid_segment)
}

/// EXPORTS_PATTERN in the CommonJS loader: `name` (optionally scoped, not
/// starting with `.`, no `%` or `\`) and the rest, which starts with `/`.
fn split_exports_request(request: &str) -> Option<(&str, &str)> {
    let bytes = request.as_bytes();
    let mut end = 0;
    if bytes.first() == Some(&b'@') {
        let slash = request.find('/')?;
        let scope = &request[1..slash];
        if scope.is_empty() || scope.contains(['%', '\\']) {
            return None;
        }
        end = slash + 1;
    }
    let rest = &request[end..];
    let first = rest.bytes().next()?;
    if matches!(first, b'.' | b'/' | b'\\' | b'%') {
        return None;
    }
    let name_end = end + rest.find('/').unwrap_or(rest.len());
    let name = &request[..name_end];
    if name[end..].contains(['%', '\\']) {
        return None;
    }
    Some((name, &request[name_end..]))
}

/// parsePackageName, for an import.
fn parse_package_name(specifier: &str, base: &Path) -> Result<(String, String), Failure> {
    let mut separator = specifier.find('/');
    let mut valid = true;
    if specifier.starts_with('@') {
        match separator {
            None => valid = false,
            Some(at) => separator = specifier[at + 1..].find('/').map(|i| i + at + 1),
        }
    }
    let name = match separator {
        None => specifier,
        Some(at) => &specifier[..at],
    };
    // invalidPackageNameRegEx: /^\.|%|\\/
    if name.starts_with('.') || name.contains('%') || name.contains('\\') {
        valid = false;
    }
    if !valid {
        return Err(node(
            "ERR_INVALID_MODULE_SPECIFIER",
            vec![
                some(specifier),
                some("is not a valid package name"),
                some(display(base)),
            ],
        ));
    }
    let subpath = match separator {
        None => ".".to_string(),
        Some(at) => format!(".{}", &specifier[at..]),
    };
    Ok((name.to_string(), subpath))
}

fn is_relative(request: &str) -> bool {
    request == "." || request == ".." || request.starts_with("./") || request.starts_with("../")
}

/// shouldBeTreatedAsRelativeOrAbsolutePath.
fn is_relative_or_absolute(specifier: &str) -> bool {
    specifier.starts_with('/') || is_relative(specifier)
}

/// URLParse for an import specifier: `node:x` is a builtin, `file:` a path,
/// any other scheme a URL. None when the specifier isn't an absolute URL.
fn absolute_url(specifier: &str) -> Option<UrlPath> {
    let colon = specifier.find(':')?;
    let scheme = &specifier[..colon];
    let valid = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return None;
    }
    match scheme.to_ascii_lowercase().as_str() {
        "node" => Some(UrlPath::builtin(&specifier[colon + 1..])),
        "file" => Some(UrlPath::parse_href(specifier)),
        _ => Some(UrlPath::other(specifier)),
    }
}

fn can_parse_url(text: &str) -> bool {
    absolute_url(text).is_some()
}

/// Whether a require request ends in a way that means "a directory":
/// `/`, or a final `.` or `..` segment.
fn has_trailing_slash(request: &str) -> bool {
    request.ends_with('/')
        || request == "."
        || request == ".."
        || request.ends_with("/.")
        || request.ends_with("/..")
}

/// path.resolve(base, relative), without touching the filesystem.
fn resolve(base: &Path, relative: &str) -> PathBuf {
    let joined = if relative.starts_with('/') {
        PathBuf::from(relative)
    } else if base.is_absolute() {
        base.join(relative)
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(base)
            .join(relative)
    };
    normalize(&joined)
}

/// path.normalize: `.` and `..` resolved lexically, a trailing slash dropped.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !path.is_absolute() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Module._nodeModulePaths(from): `<dir>/node_modules` for `from` and each
/// directory above it, skipping directories that are themselves
/// node_modules, nearest first, ending with `/node_modules`.
pub fn node_module_paths(from: &Path) -> Vec<PathBuf> {
    let from = resolve(Path::new("/"), &display(from));
    let mut paths = Vec::new();
    let mut current = Some(from.as_path());
    while let Some(dir) = current {
        if dir.parent().is_none() {
            break;
        }
        if dir.file_name().is_some_and(|name| name != "node_modules") {
            paths.push(dir.join("node_modules"));
        }
        current = dir.parent();
    }
    paths.push(PathBuf::from("/node_modules"));
    paths
}
