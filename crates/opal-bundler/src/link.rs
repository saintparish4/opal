//! Linking: what every import names, the order modules run in, and what
//! every top-level binding is called in the bundle.
//!
//! - Each request is looked up in the graph opal-core resolved. Nothing is
//!   resolved here.
//! - Imports and re-exports are resolved by ECMA-262's ResolveExport
//!   (§16.2.1.7.2.2) and GetExportedNames (§16.2.1.7.2.1), including the
//!   ambiguity rule for `export *`.
//! - Eager modules are ordered by a depth-first post-order walk from the
//!   entry, which is the order InnerModuleEvaluation (§16.2.1.6.1.3.1) runs
//!   module bodies in when no module is async.
//! - Modules reachable only through `import()` are lazy, and the cycles among
//!   them are found, because a module in a cycle shares its cycle's
//!   evaluation error.
//! - Every top-level binding of an eager module gets a name in the bundle's
//!   one scope: its own when nothing else takes it and no reference to it
//!   would be captured by an inner binding of that name.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use oxc_semantic::{Scoping, SymbolId};
use oxc_syntax::scope::ScopeId;

use opal_core::graph::{DependencyKind, DependencyTarget, ModuleGraph, ModuleId};

use crate::analyze::{BindingKind, ExportTarget, ImportName, Info};
use crate::diagnostic::{Code, Diagnostic, LineIndex};
use crate::load::Format;
use crate::options::{BuildOptions, Target as Runtime};

/// Where a request leads.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Module(usize),
    /// Kept out of the bundle by `--external`; the specifier as written.
    External(String),
    /// A Node builtin; the specifier as written.
    Builtin(String),
}

/// A binding an import or export can name, after resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Binding {
    Local {
        module: usize,
        symbol: SymbolId,
    },
    Default {
        module: usize,
    },
    Namespace {
        module: usize,
    },
    /// A named export of a CommonJS module's snapshot, including `default`
    /// and `module.exports`.
    Cjs {
        module: usize,
        name: String,
    },
    Json {
        module: usize,
    },
    /// The URL of a copied asset (rule 17).
    Asset {
        module: usize,
    },
    /// A binding of an external or builtin, by its index in `externals`;
    /// `None` is its namespace.
    External {
        index: usize,
        name: Option<String>,
    },
    /// A builtin's export the bundle supplies itself, because Node's would
    /// see the original files instead of the bundled modules:
    /// `createRequire` from `node:module`.
    Helper(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Resolution {
    Found(Binding),
    Ambiguous,
    NotFound,
}

/// What a module is in the bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Excluded,
    /// Runs in the eager order, hoisted into the bundle's scope.
    Eager,
    /// Runs on its first dynamic import, in a closure.
    Lazy,
    Cjs,
    Json,
    /// Loaded by Node at run time from its original location.
    Opaque,
    /// A file an ES module imports under `--assets` (rule 17), copied into
    /// the out-dir.
    Asset,
}

/// One step of the eager evaluation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Esm(usize),
    /// A CommonJS module's place in the ESM order, where its synthetic
    /// module record runs it (if no `require()` has) and takes the snapshot.
    Cjs(usize),
    Json(usize),
    /// A stylesheet's place in the order: it is linked into the document
    /// when this step runs (rule 17).
    Stylesheet(usize),
}

/// An external or builtin an ES module imports, hoisted to the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct External {
    pub specifier: String,
    /// The names imported, in first-use order; and whether the namespace is.
    pub names: Vec<String>,
    pub namespace: bool,
}

/// How code refers to a binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reference {
    /// An identifier in the bundle's top-level scope.
    Name(String),
    /// `object.property`, for a binding inside a lazy module's closure.
    Member(String, String),
}

pub struct Linked {
    pub roles: Vec<Role>,
    pub entry: usize,
    pub order: Vec<Step>,
    pub lazy: Vec<usize>,
    /// Lazy module -> the cycle (strongly connected component) it is in.
    pub cycle_of: HashMap<usize, usize>,
    /// Per module, per request.
    pub targets: Vec<Vec<Target>>,
    /// Per module, per import entry.
    pub imports: Vec<Vec<Binding>>,
    /// Modules whose namespace object the bundle builds, and its exports,
    /// sorted by code unit as §10.4.6.11 requires.
    pub namespaces: BTreeMap<usize, Vec<(String, Binding)>>,
    pub entry_exports: Vec<(String, Binding)>,
    pub externals: Vec<External>,
    /// Final names of eager modules' own top-level bindings.
    pub names: HashMap<(usize, SymbolId), String>,
    /// Exported names of each CommonJS module an ES module imports.
    pub cjs_exports: BTreeMap<usize, Vec<String>>,
    /// Literal `require()` and `import()` targets, per module.
    pub requires: Vec<Vec<(String, DependencyKind, Target)>>,
    /// The prefix of every name the bundler introduces.
    pub prefix: String,
    /// Top-level statements each eager module leaves out (rule 15), by
    /// index in its program body.
    pub removed: HashMap<usize, HashSet<usize>>,
    /// Of those, the ones the bundler could not prove pure and removed on
    /// the package's `sideEffects` word, per module.
    pub trusted: BTreeMap<usize, usize>,
    /// The entry awaits at its top level, so it is still evaluating after
    /// the bundle's synchronous code.
    pub entry_async: bool,
    /// Some import() is resolved when it runs, so every module has a
    /// namespace it can be found by (rule 10).
    pub runtime_imports: bool,
    /// Modules before this index are the graph's; the rest are the
    /// bundler's own helpers.
    pub graph_modules: usize,
}

impl Linked {
    /// How code refers to `binding`. `lazy_name` gives the written name of
    /// a lazy module's binding, which keeps it inside the module's closure.
    pub fn reference(
        &self,
        binding: &Binding,
        lazy_name: &dyn Fn(usize, SymbolId) -> String,
    ) -> Reference {
        let p = &self.prefix;
        match binding {
            Binding::Local { module, symbol } => match self.roles[*module] {
                Role::Lazy => {
                    Reference::Member(format!("{p}b{module}"), lazy_name(*module, *symbol))
                }
                _ => Reference::Name(self.names[&(*module, *symbol)].clone()),
            },
            Binding::Default { module } => match self.roles[*module] {
                Role::Lazy => Reference::Member(format!("{p}b{module}"), format!("{p}default")),
                _ => Reference::Name(format!("{p}default{module}")),
            },
            Binding::Namespace { module } => Reference::Name(format!("{p}ns{module}")),
            Binding::Cjs { module, name } => {
                let index = self.cjs_exports[module]
                    .iter()
                    .position(|candidate| candidate == name)
                    .expect("a resolved CommonJS export is one of its names");
                Reference::Name(format!("{p}c{module}_{index}"))
            }
            Binding::Json { module } => Reference::Name(format!("{p}json{module}")),
            Binding::Asset { module } => Reference::Name(format!("{p}asset{module}")),
            Binding::Helper(name) => Reference::Name(format!("{p}{name}")),
            Binding::External { index, name: None } => Reference::Name(format!("{p}x{index}")),
            Binding::External {
                index,
                name: Some(name),
            } => {
                let position = self.externals[*index]
                    .names
                    .iter()
                    .position(|candidate| candidate == name)
                    .expect("an external binding is one of its names");
                Reference::Name(format!("{p}x{index}_{position}"))
            }
        }
    }
}

/// What linking reads of each module.
pub struct LinkModule<'s> {
    pub info: &'s Info,
    pub scoping: &'s Scoping,
    pub lines: &'s LineIndex,
    pub text: &'s str,
    /// The graph module this came from; `None` for a virtual module.
    pub graph_id: Option<ModuleId>,
    /// CommonJS export names Node's lexer finds, and the specifiers it
    /// re-exports, for a CommonJS module.
    pub cjs_lexed: Option<(Vec<String>, Vec<String>)>,
    /// Its package's `sideEffects` says it has none (rule 15).
    pub side_effect_free: bool,
}

/// Globals the bundler's own code reads. No binding may take these names.
pub const RUNTIME_GLOBALS: &[&str] = &[
    "Array",
    "Error",
    "Function",
    "JSON",
    "Object",
    "Promise",
    "Proxy",
    "Reflect",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URL",
    "globalThis",
    "process",
    "undefined",
];

/// Virtual modules: helpers the TypeScript transform imports, which the
/// bundler supplies itself.
pub const VIRTUAL_PREFIX: &str = "@oxc-project/runtime/helpers/";

pub fn link(
    graph: &ModuleGraph,
    modules: &[LinkModule<'_>],
    entry: usize,
    virtual_ids: &HashMap<String, usize>,
    options: &BuildOptions,
) -> Result<Linked, Diagnostic> {
    let mut linker = Linker {
        graph,
        modules,
        options,
        virtual_ids,
        targets: vec![Vec::new(); modules.len()],
        requires: vec![Vec::new(); modules.len()],
        roles: vec![Role::Excluded; modules.len()],
        externals: Vec::new(),
        external_index: HashMap::new(),
        cjs_names: HashMap::new(),
        runtime_imports: false,
    };
    linker.resolve_requests(entry)?;
    let order = linker.evaluation_order(entry);
    for step in &order {
        if let Step::Esm(module) = step {
            linker.roles[*module] = Role::Eager;
        }
    }
    let (lazy, cycle_of) = linker.lazy_modules();
    linker.check_top_level_await(entry, &order)?;
    linker.check_lazy_waits_for_entry(entry, &lazy)?;
    for module in &lazy {
        if let Some(span) = linker.info(*module).top_level_arguments {
            return Err(linker.refuse(
                Code::Unsupported,
                *module,
                span,
                "`arguments` at the top level of a module reached only through import(): the bundle runs that module in a generator, whose own arguments it would read".to_string(),
            ));
        }
    }
    // Every CommonJS module an ES module requests gets a module record, and
    // so export names, even when nothing names one of them (`import 'x'`).
    for module in 0..modules.len() {
        if !matches!(linker.roles[module], Role::Eager | Role::Lazy) {
            continue;
        }
        for target in linker.targets[module].clone() {
            if let Target::Module(t) = target
                && linker.roles[t] == Role::Cjs
            {
                linker.cjs_export_names(t);
            }
        }
    }
    let imports = linker.resolve_imports()?;
    linker.check_indirect_exports()?;

    let entry_exports = match modules[entry].info.format {
        Format::Esm => linker.namespace_exports(entry)?,
        _ => Vec::new(),
    };
    let runtime_imports = linker.runtime_imports;
    let namespaces = linker.namespaces(&imports, &entry_exports, runtime_imports)?;

    let removed = shake(
        modules,
        &linker.roles,
        &imports,
        &namespaces,
        &entry_exports,
        entry,
    );
    let trusted: BTreeMap<usize, usize> = removed
        .iter()
        .filter_map(|(module, indexes)| {
            let statements = &modules[*module].info.esm.statements;
            let count = indexes.iter().filter(|i| !statements[**i].pure).count();
            (count > 0).then_some((*module, count))
        })
        .collect();
    let prefix = choose_prefix(modules, &linker.roles);
    let names = rename(modules, &linker.roles, &imports, &prefix);
    let cjs_exports: BTreeMap<usize, Vec<String>> = linker
        .cjs_names
        .iter()
        .map(|(module, names)| (*module, names.clone()))
        .collect();
    Ok(Linked {
        roles: linker.roles,
        entry,
        order,
        lazy,
        cycle_of,
        targets: linker.targets,
        imports,
        namespaces,
        entry_exports,
        externals: linker.externals,
        names,
        cjs_exports,
        requires: linker.requires,
        prefix,
        removed,
        trusted,
        entry_async: modules[entry].info.esm.top_level_await.is_some(),
        runtime_imports,
        graph_modules: graph.len(),
    })
}

/// Rule 15 over the eager modules.
fn shake(
    modules: &[LinkModule<'_>],
    roles: &[Role],
    imports: &[Vec<Binding>],
    namespaces: &BTreeMap<usize, Vec<(String, Binding)>>,
    entry_exports: &[(String, Binding)],
    entry: usize,
) -> HashMap<usize, HashSet<usize>> {
    use crate::shake::{Live, removable};
    let live = |binding: &Binding| match binding {
        Binding::Local { module, symbol } => Some(Live::Local(*module, *symbol)),
        Binding::Default { module } => Some(Live::Default(*module)),
        _ => None,
    };
    let mut statements = HashMap::new();
    let mut roots = Vec::new();
    for (module, role) in roles.iter().enumerate() {
        match role {
            Role::Eager => {
                let mut list = modules[module].info.esm.statements.clone();
                // The package's word that evaluating the module has no
                // effect: every statement may go when nothing reads it.
                if modules[module].side_effect_free && module != entry {
                    for statement in &mut list {
                        statement.pure = true;
                    }
                }
                statements.insert(module, list);
            }
            // A lazy module is not shaken, so everything it imports is read.
            Role::Lazy => roots.extend(imports[module].iter().filter_map(live)),
            _ => {}
        }
    }
    for exports in namespaces.values() {
        roots.extend(exports.iter().filter_map(|(_, binding)| live(binding)));
    }
    roots.extend(
        entry_exports
            .iter()
            .filter_map(|(_, binding)| live(binding)),
    );
    let import_target = |module: usize, symbol: SymbolId| -> Option<Live> {
        let position = modules[module]
            .info
            .esm
            .imports
            .iter()
            .position(|i| i.local == symbol)?;
        live(&imports[module][position])
    };
    removable(&statements, &roots, &import_target)
}

struct Linker<'l, 's> {
    graph: &'l ModuleGraph,
    modules: &'l [LinkModule<'s>],
    options: &'l BuildOptions,
    virtual_ids: &'l HashMap<String, usize>,
    targets: Vec<Vec<Target>>,
    requires: Vec<Vec<(String, DependencyKind, Target)>>,
    roles: Vec<Role>,
    externals: Vec<External>,
    external_index: HashMap<String, usize>,
    /// CommonJS module -> its export names as an ES module sees them.
    cjs_names: HashMap<usize, Vec<String>>,
    /// Some import() is resolved when it runs.
    runtime_imports: bool,
}

impl Linker<'_, '_> {
    fn info(&self, module: usize) -> &Info {
        self.modules[module].info
    }

    fn refuse(
        &self,
        code: Code,
        module: usize,
        span: oxc_span::Span,
        message: String,
    ) -> Diagnostic {
        let m = &self.modules[module];
        let diagnostic = Diagnostic::new(code, &m.info.path, message);
        if span == oxc_span::Span::default() {
            diagnostic
        } else {
            diagnostic.at(m.lines.locate(m.text, span.start))
        }
    }

    /// The graph's answer for `specifier` from `module`.
    fn lookup(&self, module: usize, specifier: &str, require_like: bool) -> Option<Target> {
        if let Some(rest) = specifier.strip_prefix(VIRTUAL_PREFIX)
            && let Some(id) = self.virtual_ids.get(rest)
        {
            return Some(Target::Module(*id));
        }
        if self.options.is_external(specifier) {
            return Some(Target::External(specifier.to_string()));
        }
        let graph_id = self.modules[module].graph_id?;
        let dependency = self
            .graph
            .module(graph_id)
            .dependencies
            .iter()
            .find(|d| d.specifier == specifier && d.kind.is_require_like() == require_like)?;
        match &dependency.target {
            DependencyTarget::Module { id } => {
                let path = self.graph.module(*id).path.as_str();
                if self.options.is_external(path) {
                    return Some(Target::External(specifier.to_string()));
                }
                Some(Target::Module(id.index()))
            }
            DependencyTarget::Builtin { .. } => Some(Target::Builtin(specifier.to_string())),
            DependencyTarget::Unresolved { .. } => None,
        }
    }

    fn unresolved_reason(&self, module: usize, specifier: &str) -> String {
        let Some(graph_id) = self.modules[module].graph_id else {
            return "not resolved".to_string();
        };
        self.graph
            .module(graph_id)
            .dependencies
            .iter()
            .find_map(|d| match &d.target {
                DependencyTarget::Unresolved { reason } if d.specifier == specifier => {
                    Some(reason.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| "the module graph has no edge for it".to_string())
    }

    /// Resolves every request of every module reachable from the entry, and
    /// gives each reached module its role.
    fn resolve_requests(&mut self, entry: usize) -> Result<(), Diagnostic> {
        let mut seen = HashSet::from([entry]);
        let mut queue = vec![entry];
        let modules = self.modules;
        while let Some(module) = queue.pop() {
            let info = modules[module].info;
            match info.format {
                Format::Esm => {
                    let mut targets = Vec::new();
                    for request in &info.esm.requests {
                        let Some(target) = self.lookup(module, &request.specifier, false) else {
                            let reason = self.unresolved_reason(module, &request.specifier);
                            return Err(self.refuse(
                                Code::Link,
                                module,
                                request.span,
                                format!("Cannot find module {:?}: {reason}", request.specifier),
                            ));
                        };
                        if let Some(attribute) = &request.other_attribute {
                            return Err(self.refuse(
                                Code::Link,
                                module,
                                request.span,
                                format!("Import attribute {attribute} is not supported"),
                            ));
                        }
                        if let Target::Module(t) = &target {
                            let format = self.info(*t).format;
                            match (format, request.json) {
                                (Format::Json, false) => {
                                    return Err(self.refuse(
                                        Code::Link,
                                        module,
                                        request.span,
                                        format!(
                                            "Module {:?} needs an import attribute of \"type: json\"",
                                            request.specifier
                                        ),
                                    ));
                                }
                                (Format::Json, true) => {}
                                (_, true) => {
                                    return Err(self.refuse(
                                        Code::Link,
                                        module,
                                        request.span,
                                        format!(
                                            "Module {:?} is not a JSON module, but was imported with \"type: json\"",
                                            request.specifier
                                        ),
                                    ));
                                }
                                (Format::Opaque, _) if self.options.assets => {
                                    let path = &self.info(*t).path;
                                    if path.ends_with(".css")
                                        && self.options.target != Runtime::Browser
                                    {
                                        return Err(self.refuse(
                                            Code::Unsupported,
                                            module,
                                            request.span,
                                            format!(
                                                "{:?}: a stylesheet has nowhere to go outside a browser",
                                                request.specifier
                                            ),
                                        ));
                                    }
                                    self.roles[*t] = Role::Asset;
                                }
                                (Format::Opaque, _) => {
                                    return Err(self.refuse(
                                        Code::Unsupported,
                                        module,
                                        request.span,
                                        format!(
                                            "{:?} is not JavaScript or JSON; the bundler does not load it",
                                            request.specifier
                                        ),
                                    ));
                                }
                                _ => {}
                            }
                        }
                        if let Target::Builtin(_) = &target
                            && self.options.target == Runtime::Browser
                        {
                            return Err(self.refuse(
                                Code::Builtin,
                                module,
                                request.span,
                                format!(
                                    "{:?} is a Node builtin, and there is none in a browser",
                                    request.specifier
                                ),
                            ));
                        }
                        targets.push(target);
                    }
                    for target in &targets {
                        if let Target::Module(t) = target
                            && seen.insert(*t)
                        {
                            queue.push(*t);
                        }
                    }
                    self.targets[module] = targets;
                    let mut requires = Vec::new();
                    if info.runtime_imports {
                        self.runtime_imports = true;
                    }
                    for (specifier, _) in &info.dynamic_imports {
                        match self.lookup(module, specifier, false) {
                            // JSON needs an import attribute, which a plain
                            // import() does not give: Node rejects it, and
                            // so does the run-time import the bundle makes.
                            Some(Target::Module(t)) if self.info(t).format == Format::Json => {
                                self.runtime_imports = true;
                            }
                            Some(target) => {
                                if let Target::Module(t) = &target
                                    && seen.insert(*t)
                                {
                                    queue.push(*t);
                                }
                                requires.push((
                                    specifier.clone(),
                                    DependencyKind::DynamicImport,
                                    target,
                                ));
                            }
                            // Not resolved now: resolved when it runs (rule 10).
                            None => self.runtime_imports = true,
                        }
                    }
                    self.requires[module] = requires;
                    if self.roles[module] == Role::Excluded {
                        self.roles[module] = Role::Lazy;
                    }
                }
                Format::Cjs => {
                    self.roles[module] = Role::Cjs;
                    if info.runtime_imports {
                        self.runtime_imports = true;
                    }
                    let Some(graph_id) = self.modules[module].graph_id else {
                        continue;
                    };
                    let mut requires = Vec::new();
                    for dependency in &self.graph.module(graph_id).dependencies {
                        let require_like = dependency.kind.is_require_like();
                        if !require_like && dependency.kind != DependencyKind::DynamicImport {
                            continue;
                        }
                        let Some(target) = self.lookup(module, &dependency.specifier, require_like)
                        else {
                            continue;
                        };
                        if let Target::Module(t) = &target {
                            if require_like && self.info(*t).format == Format::Esm {
                                return Err(Diagnostic::new(
                                    Code::RequireEsm,
                                    &self.info(module).path,
                                    format!(
                                        "require({:?}) loads an ES module ({}), which the bundle does not support",
                                        dependency.specifier,
                                        self.info(*t).path
                                    ),
                                ));
                            }
                            if seen.insert(*t) {
                                queue.push(*t);
                            }
                        }
                        requires.push((dependency.specifier.clone(), dependency.kind, target));
                    }
                    self.requires[module] = requires;
                }
                Format::Json => self.roles[module] = Role::Json,
                Format::Opaque => {
                    if self.roles[module] != Role::Asset {
                        self.roles[module] = Role::Opaque;
                    }
                }
            }
        }
        Ok(())
    }

    /// The post-order walk from the entry over static requests.
    fn evaluation_order(&self, entry: usize) -> Vec<Step> {
        let mut order = Vec::new();
        let mut visited = HashSet::new();
        // (module, index of the next request to look at)
        let mut stack: Vec<(usize, usize)> = Vec::new();
        if self.info(entry).format == Format::Esm {
            visited.insert(entry);
            stack.push((entry, 0));
        } else {
            return vec![match self.info(entry).format {
                Format::Json => Step::Json(entry),
                _ => Step::Cjs(entry),
            }];
        }
        while let Some((module, next)) = stack.last_mut() {
            let module = *module;
            let targets = &self.targets[module];
            if *next >= targets.len() {
                order.push(Step::Esm(module));
                stack.pop();
                continue;
            }
            let target = &targets[*next];
            *next += 1;
            let Target::Module(t) = target else {
                continue;
            };
            if !visited.insert(*t) {
                continue;
            }
            match self.info(*t).format {
                Format::Esm => stack.push((*t, 0)),
                Format::Cjs => order.push(Step::Cjs(*t)),
                Format::Json => order.push(Step::Json(*t)),
                Format::Opaque => {
                    if self.info(*t).path.ends_with(".css") {
                        order.push(Step::Stylesheet(*t));
                    }
                }
            }
        }
        order
    }

    /// Lazy modules, sorted, and the cycle each is in (Tarjan's algorithm
    /// over static edges between lazy modules).
    fn lazy_modules(&mut self) -> (Vec<usize>, HashMap<usize, usize>) {
        let lazy: Vec<usize> = (0..self.modules.len())
            .filter(|m| self.roles[*m] == Role::Lazy)
            .collect();
        let lazy_set: HashSet<usize> = lazy.iter().copied().collect();
        let edges = |m: usize| -> Vec<usize> {
            self.targets[m]
                .iter()
                .filter_map(|t| match t {
                    Target::Module(t) if lazy_set.contains(t) => Some(*t),
                    _ => None,
                })
                .collect()
        };
        let mut index_of: HashMap<usize, usize> = HashMap::new();
        let mut low: HashMap<usize, usize> = HashMap::new();
        let mut on_stack = HashSet::new();
        let mut stack = Vec::new();
        let mut cycle_of = HashMap::new();
        let mut next_index = 0;
        let mut next_cycle = 0;
        for &start in &lazy {
            if index_of.contains_key(&start) {
                continue;
            }
            let mut work: Vec<(usize, Vec<usize>, usize)> = vec![(start, edges(start), 0)];
            index_of.insert(start, next_index);
            low.insert(start, next_index);
            next_index += 1;
            stack.push(start);
            on_stack.insert(start);
            while let Some((node, children, position)) = work.last_mut() {
                let node = *node;
                if *position < children.len() {
                    let child = children[*position];
                    *position += 1;
                    if let std::collections::hash_map::Entry::Vacant(entry) = index_of.entry(child)
                    {
                        entry.insert(next_index);
                        low.insert(child, next_index);
                        next_index += 1;
                        stack.push(child);
                        on_stack.insert(child);
                        let grandchildren = edges(child);
                        work.push((child, grandchildren, 0));
                    } else if on_stack.contains(&child) {
                        let value = low[&node].min(index_of[&child]);
                        low.insert(node, value);
                    }
                    continue;
                }
                work.pop();
                if let Some((parent, _, _)) = work.last() {
                    let value = low[parent].min(low[&node]);
                    low.insert(*parent, value);
                }
                if low[&node] == index_of[&node] {
                    while let Some(member) = stack.pop() {
                        on_stack.remove(&member);
                        cycle_of.insert(member, next_cycle);
                        if member == node {
                            break;
                        }
                    }
                    next_cycle += 1;
                }
            }
        }
        (lazy, cycle_of)
    }

    /// `SEMANTICS.md` rule 11.
    fn check_top_level_await(&self, entry: usize, order: &[Step]) -> Result<(), Diagnostic> {
        for (module, role) in self.roles.iter().enumerate() {
            if !matches!(role, Role::Eager | Role::Lazy) {
                continue;
            }
            let Some(span) = self.info(module).esm.top_level_await else {
                continue;
            };
            if module != entry {
                return Err(self.refuse(
                    Code::Tla,
                    module,
                    span,
                    "top-level await outside the entry module: ECMA-262 runs sibling modules while an async module is suspended, and the bundle cannot interleave them".to_string(),
                ));
            }
            if self.reaches(entry, entry, order) {
                return Err(self.refuse(
                    Code::Tla,
                    module,
                    span,
                    "top-level await in an entry module that is in an import cycle".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// A lazy module that statically depends on an entry awaiting at its
    /// top level would, in Node, wait for the entry while the rest of its
    /// graph runs (§16.2.1.6.1.3.2, ExecuteAsyncModule); the bundle runs a
    /// lazy module's graph at once, so it refuses instead.
    fn check_lazy_waits_for_entry(&self, entry: usize, lazy: &[usize]) -> Result<(), Diagnostic> {
        if self.info(entry).esm.top_level_await.is_none() {
            return Ok(());
        }
        for module in lazy {
            if self.reaches(*module, entry, &[]) {
                return Err(Diagnostic::new(
                    Code::Tla,
                    &self.info(*module).path,
                    format!(
                        "this module is imported with import(), and it depends on the entry ({}), which awaits at its top level: Node makes it wait for the entry, and the bundle cannot",
                        self.info(entry).path
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Whether `to` is reachable from `from` over at least one static edge.
    fn reaches(&self, from: usize, to: usize, _order: &[Step]) -> bool {
        let mut seen = HashSet::new();
        let mut stack: Vec<usize> = self.targets[from]
            .iter()
            .filter_map(|t| match t {
                Target::Module(t) => Some(*t),
                _ => None,
            })
            .collect();
        while let Some(module) = stack.pop() {
            if module == to {
                return true;
            }
            if !seen.insert(module) || self.info(module).format != Format::Esm {
                continue;
            }
            for target in &self.targets[module] {
                if let Target::Module(t) = target {
                    stack.push(*t);
                }
            }
        }
        false
    }

    fn external_binding(&mut self, specifier: &str, name: Option<&str>) -> Binding {
        if matches!(specifier, "module" | "node:module") && name == Some("createRequire") {
            return Binding::Helper("createRequire");
        }
        let index = match self.external_index.get(specifier) {
            Some(index) => *index,
            None => {
                let index = self.externals.len();
                self.externals.push(External {
                    specifier: specifier.to_string(),
                    names: Vec::new(),
                    namespace: false,
                });
                self.external_index.insert(specifier.to_string(), index);
                index
            }
        };
        let external = &mut self.externals[index];
        match name {
            None => external.namespace = true,
            Some(name) => {
                if !external.names.iter().any(|n| n == name) {
                    external.names.push(name.to_string());
                }
            }
        }
        Binding::External {
            index,
            name: name.map(str::to_string),
        }
    }

    /// The export names a CommonJS module has as an ES module sees it:
    /// what Node's lexer finds, transitively through its re-exports, plus
    /// `default` and `module.exports` (translators.js:203-275, 420-470).
    fn cjs_export_names(&mut self, module: usize) -> Vec<String> {
        if let Some(names) = self.cjs_names.get(&module) {
            return names.clone();
        }
        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut visited = HashSet::new();
        let mut stack = vec![module];
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            let Some((exports, reexports)) = &self.modules[current].cjs_lexed else {
                continue;
            };
            names.extend(exports.iter().cloned());
            for specifier in reexports {
                if let Some(Target::Module(target)) = self.lookup(current, specifier, true)
                    && self.info(target).format == Format::Cjs
                {
                    stack.push(target);
                }
            }
        }
        names.insert("default".to_string());
        names.insert("module.exports".to_string());
        let names: Vec<String> = names.into_iter().collect();
        self.cjs_names.insert(module, names.clone());
        names
    }

    fn resolve_export(
        &mut self,
        module: usize,
        name: &str,
        resolve_set: &mut Vec<(usize, String)>,
    ) -> Resolution {
        match self.info(module).format {
            Format::Cjs => {
                return if self.cjs_export_names(module).iter().any(|n| n == name) {
                    Resolution::Found(Binding::Cjs {
                        module,
                        name: name.to_string(),
                    })
                } else {
                    Resolution::NotFound
                };
            }
            Format::Json => {
                return if name == "default" {
                    Resolution::Found(Binding::Json { module })
                } else {
                    Resolution::NotFound
                };
            }
            // An asset's one export is its URL (rule 17); a stylesheet has
            // none, since Vite gives a plain CSS import no value.
            Format::Opaque => {
                let path = &self.info(module).path;
                return if name == "default"
                    && self.roles[module] == Role::Asset
                    && !path.ends_with(".css")
                {
                    Resolution::Found(Binding::Asset { module })
                } else {
                    Resolution::NotFound
                };
            }
            Format::Esm => {}
        }
        if resolve_set.iter().any(|(m, n)| *m == module && n == name) {
            // A circular import request: §16.2.1.7.2.2 step 2.
            return Resolution::NotFound;
        }
        resolve_set.push((module, name.to_string()));
        let exports = self.info(module).esm.exports.clone();
        if let Some(export) = exports.iter().find(|e| e.name == name) {
            return match &export.target {
                ExportTarget::Local(symbol) => self.resolve_local(module, *symbol, resolve_set),
                ExportTarget::Default => Resolution::Found(Binding::Default { module }),
                ExportTarget::Indirect { request, name } => {
                    match self.targets[module][*request].clone() {
                        Target::Module(target) => match name {
                            ImportName::Namespace => {
                                Resolution::Found(Binding::Namespace { module: target })
                            }
                            ImportName::Name(imported) => {
                                self.resolve_export(target, imported, resolve_set)
                            }
                        },
                        Target::External(s) | Target::Builtin(s) => Resolution::Found(match name {
                            ImportName::Namespace => self.external_binding(&s, None),
                            ImportName::Name(imported) => self.external_binding(&s, Some(imported)),
                        }),
                    }
                }
            };
        }
        if name == "default" {
            return Resolution::NotFound;
        }
        let mut star_resolution: Option<Binding> = None;
        let stars = self.info(module).esm.stars.clone();
        for (request, _) in stars {
            match self.targets[module][request].clone() {
                Target::Module(target) => match self.resolve_export(target, name, resolve_set) {
                    Resolution::Ambiguous => return Resolution::Ambiguous,
                    Resolution::NotFound => {}
                    Resolution::Found(binding) => match &star_resolution {
                        None => star_resolution = Some(binding),
                        Some(existing) if *existing == binding => {}
                        Some(_) => return Resolution::Ambiguous,
                    },
                },
                // An external's names are not known until it runs.
                Target::External(_) | Target::Builtin(_) => {}
            }
        }
        match star_resolution {
            Some(binding) => Resolution::Found(binding),
            None => Resolution::NotFound,
        }
    }

    /// A local export: the binding itself, or what the import it names
    /// resolves to.
    fn resolve_local(
        &mut self,
        module: usize,
        symbol: SymbolId,
        resolve_set: &mut Vec<(usize, String)>,
    ) -> Resolution {
        let import = self
            .info(module)
            .esm
            .imports
            .iter()
            .find(|import| import.local == symbol)
            .cloned();
        let Some(import) = import else {
            return Resolution::Found(Binding::Local { module, symbol });
        };
        match self.targets[module][import.request].clone() {
            Target::Module(target) => match &import.name {
                ImportName::Namespace => Resolution::Found(Binding::Namespace { module: target }),
                ImportName::Name(name) => self.resolve_export(target, name, resolve_set),
            },
            Target::External(s) | Target::Builtin(s) => Resolution::Found(match &import.name {
                ImportName::Namespace => self.external_binding(&s, None),
                ImportName::Name(name) => self.external_binding(&s, Some(name)),
            }),
        }
    }

    fn link_error(
        &self,
        module: usize,
        request: usize,
        name: &str,
        resolution: &Resolution,
    ) -> Diagnostic {
        let info = self.info(module);
        let request_info = &info.esm.requests[request];
        let specifier = &request_info.specifier;
        let message = match resolution {
            Resolution::Ambiguous => format!(
                "The requested module '{specifier}' contains conflicting star exports for name '{name}'"
            ),
            _ => match &self.targets[module][request] {
                Target::Module(t) if self.info(*t).format == Format::Cjs => format!(
                    "Named export '{name}' not found. The requested module '{specifier}' is a CommonJS module, which may not support all module.exports as named exports."
                ),
                _ => format!(
                    "The requested module '{specifier}' does not provide an export named '{name}'"
                ),
            },
        };
        self.refuse(Code::Link, module, request_info.span, message)
    }

    fn resolve_imports(&mut self) -> Result<Vec<Vec<Binding>>, Diagnostic> {
        let mut all = vec![Vec::new(); self.modules.len()];
        #[allow(clippy::needless_range_loop)]
        for module in 0..self.modules.len() {
            if !matches!(self.roles[module], Role::Eager | Role::Lazy) {
                continue;
            }
            let imports = self.info(module).esm.imports.clone();
            let mut bindings = Vec::with_capacity(imports.len());
            for import in &imports {
                let binding = match self.targets[module][import.request].clone() {
                    Target::Module(target) => match &import.name {
                        ImportName::Namespace => Binding::Namespace { module: target },
                        ImportName::Name(name) => {
                            match self.resolve_export(target, name, &mut Vec::new()) {
                                Resolution::Found(binding) => binding,
                                other => {
                                    return Err(self.link_error(
                                        module,
                                        import.request,
                                        name,
                                        &other,
                                    ));
                                }
                            }
                        }
                    },
                    Target::External(s) | Target::Builtin(s) => match &import.name {
                        ImportName::Namespace => self.external_binding(&s, None),
                        ImportName::Name(name) => self.external_binding(&s, Some(name)),
                    },
                };
                bindings.push(binding);
            }
            all[module] = bindings;
        }
        Ok(all)
    }

    /// InitializeEnvironment checks every indirect export resolves, whether
    /// anything imports it or not (§16.2.1.7.3.1 step 1).
    fn check_indirect_exports(&mut self) -> Result<(), Diagnostic> {
        for module in 0..self.modules.len() {
            if !matches!(self.roles[module], Role::Eager | Role::Lazy) {
                continue;
            }
            let exports = self.info(module).esm.exports.clone();
            for export in exports {
                if let ExportTarget::Indirect {
                    request,
                    name: ImportName::Name(name),
                } = &export.target
                {
                    let resolution = self.resolve_export(module, &export.name, &mut Vec::new());
                    if !matches!(resolution, Resolution::Found(_)) {
                        return Err(self.link_error(module, *request, name, &resolution));
                    }
                }
            }
        }
        Ok(())
    }

    /// GetExportedNames, without the names an `export *` from an external
    /// would add, which are not known until it runs.
    fn exported_names(
        &mut self,
        module: usize,
        star_set: &mut Vec<usize>,
    ) -> Result<Vec<String>, Diagnostic> {
        match self.info(module).format {
            Format::Cjs => return Ok(self.cjs_export_names(module)),
            Format::Json => return Ok(vec!["default".to_string()]),
            Format::Opaque => return Ok(Vec::new()),
            Format::Esm => {}
        }
        if star_set.contains(&module) {
            return Ok(Vec::new());
        }
        star_set.push(module);
        let mut names: Vec<String> = Vec::new();
        for export in &self.info(module).esm.exports {
            if !names.contains(&export.name) {
                names.push(export.name.clone());
            }
        }
        let stars = self.info(module).esm.stars.clone();
        for (request, span) in stars {
            match self.targets[module][request].clone() {
                Target::Module(target) => {
                    for name in self.exported_names(target, star_set)? {
                        if name != "default" && !names.contains(&name) {
                            names.push(name);
                        }
                    }
                }
                Target::External(s) | Target::Builtin(s) => {
                    return Err(self.refuse(
                        Code::Link,
                        module,
                        span,
                        format!(
                            "`export * from {s:?}`: an external's names are not known until it runs, so this module's namespace cannot be built"
                        ),
                    ));
                }
            }
        }
        Ok(names)
    }

    /// A namespace's exports: every exported name that resolves, sorted by
    /// code unit (§16.2.1.13 GetModuleNamespace, §10.4.6.11).
    fn namespace_exports(&mut self, module: usize) -> Result<Vec<(String, Binding)>, Diagnostic> {
        let names = self.exported_names(module, &mut Vec::new())?;
        let mut exports = Vec::new();
        for name in names {
            if let Resolution::Found(binding) = self.resolve_export(module, &name, &mut Vec::new())
            {
                exports.push((name, binding));
            }
        }
        exports.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
        Ok(exports)
    }

    /// Every module whose namespace object some binding needs, with its
    /// exports. A namespace's own exports can name further namespaces.
    fn namespaces(
        &mut self,
        imports: &[Vec<Binding>],
        entry_exports: &[(String, Binding)],
        runtime_imports: bool,
    ) -> Result<BTreeMap<usize, Vec<(String, Binding)>>, Diagnostic> {
        let mut needed: Vec<usize> = Vec::new();
        if runtime_imports {
            // An import() resolved when it runs can reach any module.
            needed.extend(
                (0..self.modules.len())
                    .filter(|m| matches!(self.roles[*m], Role::Eager | Role::Lazy | Role::Cjs)),
            );
        }
        let note = |binding: &Binding, needed: &mut Vec<usize>| {
            if let Binding::Namespace { module } = binding {
                needed.push(*module);
            }
        };
        for bindings in imports {
            for binding in bindings {
                note(binding, &mut needed);
            }
        }
        for (_, binding) in entry_exports {
            note(binding, &mut needed);
        }
        for module in 0..self.modules.len() {
            for (_, _, target) in &self.requires[module] {
                if let Target::Module(t) = target
                    && matches!(self.roles[*t], Role::Eager | Role::Lazy | Role::Cjs)
                    && self.requires[module]
                        .iter()
                        .any(|(_, kind, tt)| *kind == DependencyKind::DynamicImport && tt == target)
                {
                    needed.push(*t);
                }
            }
        }
        let mut namespaces = BTreeMap::new();
        while let Some(module) = needed.pop() {
            if namespaces.contains_key(&module) {
                continue;
            }
            let exports = self.namespace_exports(module)?;
            for (_, binding) in &exports {
                if let Binding::Namespace { module } = binding {
                    needed.push(*module);
                }
            }
            namespaces.insert(module, exports);
        }
        Ok(namespaces)
    }
}

/// A prefix no identifier in the program starts with, so every name the
/// bundler introduces is free.
fn choose_prefix(modules: &[LinkModule<'_>], roles: &[Role]) -> String {
    let mut suffix = 0;
    loop {
        let prefix = if suffix == 0 {
            "__opal_".to_string()
        } else {
            format!("__opal{suffix}_")
        };
        let taken = modules
            .iter()
            .zip(roles)
            .filter(|(_, role)| **role != Role::Excluded)
            .any(|(module, _)| {
                module
                    .info
                    .names
                    .iter()
                    .any(|name| name.starts_with(&prefix))
            });
        if !taken {
            return prefix;
        }
        suffix += 1;
    }
}

/// One reference that will print as a different name than it was written
/// with, unless the name chosen is the written one.
struct Alias {
    module: usize,
    scope: ScopeId,
    written: String,
}

/// Final names for every eager module's own top-level bindings.
fn rename(
    modules: &[LinkModule<'_>],
    roles: &[Role],
    imports: &[Vec<Binding>],
    _prefix: &str,
) -> HashMap<(usize, SymbolId), String> {
    // Names nothing may take: every global any bundled module reads, and
    // the globals the bundler's own code reads.
    let mut used: HashSet<String> = RUNTIME_GLOBALS.iter().map(|s| s.to_string()).collect();
    for (module, role) in modules.iter().zip(roles) {
        if *role != Role::Excluded {
            used.extend(module.info.globals.iter().cloned());
        }
    }

    // Every reference that reaches each binding through an import.
    let mut aliases: HashMap<(usize, SymbolId), Vec<Alias>> = HashMap::new();
    for (importer, bindings) in imports.iter().enumerate() {
        for (import, binding) in modules[importer].info.esm.imports.iter().zip(bindings) {
            let Binding::Local { module, symbol } = binding else {
                continue;
            };
            if roles[*module] != Role::Eager {
                continue;
            }
            let Some(uses) = modules[importer].info.esm.uses.get(&import.local) else {
                continue;
            };
            for scope in &uses.scopes {
                aliases.entry((*module, *symbol)).or_default().push(Alias {
                    module: importer,
                    scope: *scope,
                    written: import.local_name.clone(),
                });
            }
        }
    }

    let mut candidates: Vec<(usize, SymbolId, String, bool)> = Vec::new();
    for (module, role) in roles.iter().enumerate() {
        if *role != Role::Eager {
            continue;
        }
        for binding in &modules[module].info.esm.top_level {
            if binding.kind == BindingKind::Import {
                continue;
            }
            let tdz = matches!(binding.kind, BindingKind::Lexical | BindingKind::Class);
            candidates.push((module, binding.symbol, binding.name.clone(), tdz));
        }
    }
    // Bindings with a temporal dead zone choose first: a TDZ error message
    // names the binding, so they are the ones whose name can be observed
    // without calling anything.
    candidates.sort_by_key(|(module, symbol, _, tdz)| (!*tdz, *module, symbol.index()));

    let mut names = HashMap::new();
    for (module, symbol, name, _) in candidates {
        let own: Vec<ScopeId> = modules[module]
            .info
            .esm
            .uses
            .get(&symbol)
            .map(|uses| uses.scopes.clone())
            .unwrap_or_default();
        let alias = aliases.get(&(module, symbol));
        let mut attempt = 0;
        let chosen = loop {
            let candidate = if attempt == 0 {
                name.clone()
            } else {
                format!("{name}${attempt}")
            };
            attempt += 1;
            if used.contains(&candidate) {
                continue;
            }
            let own_ok = candidate == name
                || own
                    .iter()
                    .all(|scope| !captured(modules, roles, module, *scope, &candidate));
            if !own_ok {
                continue;
            }
            let alias_ok = alias.is_none_or(|aliases| {
                aliases.iter().all(|alias| {
                    alias.written == candidate
                        || !captured(modules, roles, alias.module, alias.scope, &candidate)
                })
            });
            if alias_ok {
                break candidate;
            }
        };
        used.insert(chosen.clone());
        names.insert((module, symbol), chosen);
    }
    names
}

/// Whether `name`, printed at a reference in `scope` of `module`, would
/// resolve to a binding of that module's instead of the bundle's top level.
fn captured(
    modules: &[LinkModule<'_>],
    roles: &[Role],
    module: usize,
    scope: ScopeId,
    name: &str,
) -> bool {
    let scoping = modules[module].scoping;
    let root = scoping.root_scope_id();
    for current in scoping.scope_ancestors(scope) {
        if current == root {
            if roles[module] == Role::Lazy
                && let Some(symbol) = scoping.get_binding(root, name.into())
            {
                // A lazy module's own top-level bindings live in its
                // closure, but its import bindings are not declared there.
                return !modules[module]
                    .info
                    .esm
                    .imports
                    .iter()
                    .any(|i| i.local == symbol);
            }
            return false;
        }
        if scoping.get_binding(current, name.into()).is_some() {
            return true;
        }
    }
    false
}
