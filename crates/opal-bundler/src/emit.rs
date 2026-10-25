//! Printing the bundle.
//!
//! Each module is rewritten in its own AST, renamed through its scoping, and
//! printed with a source map. Then the pieces are put in order: imports of
//! externals and builtins, the runtime, module definitions (CommonJS
//! records, JSON, lazy modules), namespace objects, and the eager modules'
//! bodies in evaluation order with each CommonJS and JSON module's step at
//! its place.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use oxc_allocator::{Allocator, TakeIn};
use oxc_ast::ast::{
    Argument, AssignmentOperator, AssignmentTarget, BindingPattern, ClassElement, ClassType,
    ExportDefaultDeclarationKind, Expression, Program, SimpleAssignmentTarget, Statement,
};
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_semantic::{Scoping, SymbolId};
use oxc_sourcemap::SourceMap;
use oxc_span::{GetSpan, SourceType, Span};

use crate::analyze::{DefaultKind, Info};
use crate::define::Defines;
use crate::diagnostic::{Code, Diagnostic, LineIndex};
use crate::link::{Binding, Linked, Reference, Role, Step, Target};
use crate::load::{Format, Parsed, Source};
use crate::options::{BuildOptions, Target as Runtime};
use crate::runtime;

/// One module, as emit sees it.
pub struct Unit<'a> {
    pub source: &'a Source,
    pub info: &'a Info,
    pub parsed: Option<Parsed<'a>>,
    pub allocator: &'a Allocator,
    /// URL of the original file relative to the out-dir, percent-encoded.
    pub rel: String,
}

pub struct Bundle {
    pub code: String,
    pub map: String,
    /// Files to write next to the bundle: path relative to the out-dir, and
    /// bytes (rule 17).
    pub assets: Vec<(String, Vec<u8>)>,
}

/// The text of the bundle so far, with each piece's source map and the line
/// it starts on.
struct Out {
    text: String,
    lines: u32,
    maps: Vec<(SourceMap<'static>, u32)>,
}

impl Out {
    fn push(&mut self, text: &str) {
        self.lines += text.bytes().filter(|b| *b == b'\n').count() as u32;
        self.text.push_str(text);
        if !text.ends_with('\n') && !text.is_empty() {
            self.text.push('\n');
            self.lines += 1;
        }
    }

    fn push_mapped(&mut self, text: &str, map: Option<SourceMap<'static>>) {
        if let Some(map) = map {
            self.maps.push((map, self.lines));
        }
        self.push(text);
    }
}

pub fn emit(
    units: &mut [Unit<'_>],
    linked: &Linked,
    options: &BuildOptions,
    defines: &Defines,
    root_rel: &str,
    file_name: &str,
) -> Result<Bundle, Diagnostic> {
    let p = linked.prefix.as_str();
    let node = options.target == Runtime::Node;

    // What the runtime has to include.
    let uses_create_require = linked
        .imports
        .iter()
        .flatten()
        .any(|binding| *binding == Binding::Helper("createRequire"));
    let has_cjs = linked
        .roles
        .iter()
        .any(|role| matches!(role, Role::Cjs | Role::Json))
        || uses_create_require;
    // A lazy module's bindings keep their written names, inside its closure.
    let mut lazy_names: HashMap<(usize, SymbolId), String> = HashMap::new();
    for module in &linked.lazy {
        for binding in &units[*module].info.esm.top_level {
            lazy_names.insert((*module, binding.symbol), binding.name.clone());
        }
    }
    let lazy_symbols = |module: usize, symbol: SymbolId| -> String {
        lazy_names
            .get(&(module, symbol))
            .cloned()
            .unwrap_or_default()
    };
    let reference = |binding: &Binding| linked.reference(binding, &lazy_symbols);

    // Bindings other modules read out of each lazy module.
    let mut exposed: HashMap<usize, BTreeSet<String>> = HashMap::new();
    let expose = |binding: &Binding, exposed: &mut HashMap<usize, BTreeSet<String>>| {
        if let Reference::Member(_, property) = reference(binding) {
            let module = match binding {
                Binding::Local { module, .. } | Binding::Default { module } => *module,
                _ => return,
            };
            exposed.entry(module).or_default().insert(property);
        }
    };
    for bindings in &linked.imports {
        for binding in bindings {
            expose(binding, &mut exposed);
        }
    }
    for exports in linked.namespaces.values() {
        for (_, binding) in exports {
            expose(binding, &mut exposed);
        }
    }

    // Rewrite and print each module.
    let mut pieces: HashMap<usize, Piece> = HashMap::new();
    let mut fixups: Vec<String> = Vec::new();
    let mut paths: BTreeSet<usize> = BTreeSet::new();
    let mut uses = Uses::default();
    #[allow(clippy::needless_range_loop)]
    for module in 0..units.len() {
        let role = linked.roles[module];
        if !matches!(role, Role::Eager | Role::Lazy | Role::Cjs) {
            continue;
        }
        let piece = print_module(
            module,
            &mut units[module],
            linked,
            options,
            defines,
            &reference,
            &mut uses,
        )?;
        if role == Role::Eager {
            fixups.extend(piece.fixups.iter().cloned());
        }
        if piece.needs_paths {
            paths.insert(module);
        }
        pieces.insert(module, piece);
    }

    let mut out = Out {
        text: String::new(),
        lines: 0,
        maps: Vec::new(),
    };
    let entry_unit = &units[linked.entry];
    if let Some(parsed) = &entry_unit.parsed
        && let Some(hashbang) = &parsed.program.hashbang
    {
        out.push(&format!("#!{}", hashbang.value));
    }

    // Node's own modules, which the runtime needs.
    let needs_paths = !paths.is_empty() || has_cjs;
    if node && needs_paths {
        out.push(&runtime::with_prefix(
            "import { fileURLToPath as $$fileURLToPath, pathToFileURL as $$pathToFileURL } from \"node:url\";\nimport { dirname as $$dirname, relative as $$relative, sep as $$sep } from \"node:path\";",
            p,
        ));
    }
    if node && uses.import_any {
        out.push(&runtime::with_prefix(
            "import { existsSync as $$existsSync } from \"node:fs\";\nimport { isBuiltin as $$isBuiltin, _nodeModulePaths as $$nodeModulePaths } from \"node:module\";\nimport { join as $$join } from \"node:path\";",
            p,
        ));
    }
    if node && has_cjs {
        out.push(&runtime::with_prefix(
            "import { createRequire as $$createRequireReal, Module as $$NodeModule } from \"node:module\";\nimport { compileFunction as $$compileFunction } from \"node:vm\";",
            p,
        ));
    }
    for (index, external) in linked.externals.iter().enumerate() {
        let specifier = json_string(&external.specifier);
        if external.namespace {
            out.push(&format!("import * as {p}x{index} from {specifier};"));
        }
        if !external.names.is_empty() {
            let names: Vec<String> = external
                .names
                .iter()
                .enumerate()
                .map(|(position, name)| {
                    format!("{} as {p}x{index}_{position}", export_name_text(name))
                })
                .collect();
            out.push(&format!(
                "import {{ {} }} from {specifier};",
                names.join(", ")
            ));
        }
    }

    // The runtime.
    let mut text = String::new();
    text.push_str(runtime::CAPTURE);
    let needs_ns = !linked.namespaces.is_empty();
    if needs_ns {
        text.push_str(runtime::NAMESPACE);
    }
    if !linked.lazy.is_empty() {
        text.push_str(runtime::LAZY);
    }
    if uses.import || uses.import_any {
        text.push_str(runtime::IMPORT);
    }
    if uses.import_any {
        text.push_str(if node {
            runtime::IMPORT_ANY_NODE
        } else {
            runtime::IMPORT_ANY_BROWSER
        });
    }
    if !fixups.is_empty() || uses.set_name || has_cjs {
        text.push_str(runtime::SET_NAME);
    }
    if uses.const_assign {
        text.push_str(runtime::CONST_ASSIGN);
    }
    if needs_paths {
        text.push_str(if node {
            runtime::PATHS_NODE
        } else {
            runtime::PATHS_BROWSER
        });
    }
    if has_cjs {
        let cjs = if node {
            runtime::CJS_NODE.replace("%ROOT%", root_rel)
        } else {
            runtime::CJS_BROWSER.to_string()
        };
        text.push_str(&cjs);
        text.push_str(runtime::CJS_IMPORT);
        if uses_create_require && node {
            text.push_str(runtime::CREATE_REQUIRE);
        }
        if !needs_ns {
            text.push_str(runtime::NAMESPACE);
        }
    }
    if units[linked.entry].info.format == Format::Cjs {
        text.push_str(runtime::CJS_MAIN);
    }
    out.push(&runtime::with_prefix(&text, p));
    if linked.entry_async {
        out.push(&format!(
            "let {p}entryFinish;\nconst {p}entryDone = new {p}Promise((resolve) => {{ {p}entryFinish = resolve; }});"
        ));
    }

    // Paths of the modules that read their own location.
    for module in &paths {
        let rel = json_string(&units[*module].rel);
        if node {
            out.push(&format!(
                "const {p}file{module} = {p}abs({rel}), {p}url{module} = {p}pathToFileURL({p}file{module}).href, {p}dir{module} = {p}dirname({p}file{module});"
            ));
        } else {
            out.push(&format!("const {p}url{module} = {p}fileURL({rel});"));
        }
    }

    // Assets (rule 17): each file copied under assets/ with its content's
    // hash in the name, and its URL from the bundle's own location.
    let mut assets = Vec::new();
    for (module, role) in linked.roles.iter().enumerate() {
        if *role != Role::Asset {
            continue;
        }
        let source = units[module].source;
        let bytes = std::fs::read(&source.absolute).map_err(|error| {
            Diagnostic::new(Code::Io, &source.path, format!("cannot read: {error}"))
        })?;
        if source.path.ends_with(".css") {
            let text = String::from_utf8_lossy(&bytes);
            if text.contains("url(") || text.contains("@import") {
                return Err(Diagnostic::new(
                    Code::Unsupported,
                    &source.path,
                    "a stylesheet with url() or @import: the bundler copies stylesheets as they are and does not follow what they reference",
                ));
            }
        }
        let file = std::path::Path::new(&source.path);
        let stem = file
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().to_string());
        let extension = file
            .extension()
            .map_or_else(String::new, |e| format!(".{}", e.to_string_lossy()));
        let hash = opal_core::hash::ContentHash::of(&bytes).to_hex();
        let name = format!("assets/{stem}-{}{extension}", &hash[..8]);
        out.push(&format!(
            "const {p}asset{module} = new {p}URL({}, import.meta.url).href;",
            json_string(&format!("./{name}"))
        ));
        assets.push((name, bytes));
    }
    if assets.iter().any(|(name, _)| name.ends_with(".css")) {
        out.push(&runtime::with_prefix(runtime::STYLESHEET, p));
    }

    // Bundle-level bindings of CommonJS snapshots and JSON modules.
    let mut declared = Vec::new();
    for (module, names) in &linked.cjs_exports {
        let names_json: Vec<String> = names.iter().map(|n| json_string(n)).collect();
        out.push(&format!(
            "const {p}names{module} = [{}];",
            names_json.join(", ")
        ));
        let mut sorted = names.clone();
        sorted.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        let sorted: Vec<String> = sorted.iter().map(|n| json_string(n)).collect();
        out.push(&format!(
            "const {p}sorted{module} = [{}];",
            sorted.join(", ")
        ));
        for index in 0..names.len() {
            declared.push(format!("{p}c{module}_{index}"));
        }
    }
    for (module, role) in linked.roles.iter().enumerate() {
        if *role == Role::Json {
            declared.push(format!("{p}json{module}"));
        }
        if matches!(role, Role::Json | Role::Cjs) && linked.namespaces.contains_key(&module) {
            declared.push(format!("{p}ns{module}"));
        }
        if *role == Role::Lazy {
            declared.push(format!("{p}b{module}"));
        }
    }
    if !declared.is_empty() {
        out.push(&format!("var {};", declared.join(", ")));
    }

    // CommonJS and JSON records.
    for (module, role) in linked.roles.iter().enumerate() {
        let unit = &units[module];
        let rel = json_string(&unit.rel);
        let key = json_string(&unit.source.path);
        match role {
            Role::Json => {
                out.push(&format!(
                    "const {p}m{module} = {p}json({rel}, {key}, {});",
                    json_string(&unit.source.text)
                ));
            }
            Role::Cjs => {
                let piece = pieces
                    .remove(&module)
                    .expect("every CommonJS module is printed");
                let deps = require_table(module, linked);
                if piece.sloppy {
                    out.push(&format!(
                        "const {p}m{module} = {p}cjs({rel}, {key}, () => ({deps}), undefined, {});",
                        json_string(&piece.code)
                    ));
                } else {
                    out.push(&format!(
                        "const {p}m{module} = {p}cjs({rel}, {key}, () => ({deps}), function (exports, require, module, __filename, __dirname) {{"
                    ));
                    out.push_mapped(&piece.code, piece.map);
                    out.push("}, undefined);");
                }
            }
            _ => {}
        }
    }

    // Namespace objects of eager modules: getters into the bundle's scope,
    // created before any module runs, which is harmless since reading one
    // early throws as the binding does.
    // A lazy module's namespace is made here too, not inside its closure:
    // its getters read bindings of other modules by their bundle names,
    // which the closure's own top-level bindings would capture. Its own
    // bindings it reads through the bindings object its closure fills.
    for (module, exports) in &linked.namespaces {
        if !matches!(linked.roles[*module], Role::Eager | Role::Lazy) {
            continue;
        }
        out.push(&format!(
            "var {p}ns{module} = {};",
            namespace_call(p, exports, &reference)
        ));
    }

    // Every module an import() resolved at run time can find, by the URL
    // of its original file.
    if uses.import_any {
        for (module, role) in linked.roles.iter().enumerate() {
            let thunk = match role {
                Role::Eager => format!("() => {p}ns{module}"),
                Role::Lazy => format!("() => ({p}init{module}(), {p}ns{module})"),
                Role::Cjs => {
                    format!("() => {p}cjsNs({p}m{module}, {p}names{module}, {p}sorted{module})")
                }
                _ => continue,
            };
            if module >= linked.graph_modules {
                continue;
            }
            out.push(&format!(
                "{p}byURL[{p}fileURL({})] = {thunk};",
                json_string(&units[module].rel)
            ));
        }
    }

    // Lazy modules.
    for module in &linked.lazy {
        let piece = pieces.remove(module).expect("every lazy module is printed");
        let cycle = linked.cycle_of[module];
        out.push(&format!(
            "const {p}init{module} = {p}lazy({cycle}, () => {{"
        ));
        let mut getters = Vec::new();
        for name in exposed.get(module).into_iter().flatten() {
            let local = if name == &format!("{p}default") {
                format!("{p}default{module}")
            } else {
                name.clone()
            };
            getters.push(format!(
                "get {}() {{ return {local}; }}",
                export_name_text(name)
            ));
        }
        out.push(&format!(
            "{p}b{module} = {{ __proto__: null, {} }};",
            getters.join(", ")
        ));
        for fixup in &piece.fixups {
            out.push(fixup);
        }
        out.push(&dependency_steps(*module, linked, units, p));
        out.push_mapped(&piece.code, piece.map);
        out.push("});");
    }

    for fixup in &fixups {
        out.push(fixup);
    }

    // The eager modules, in evaluation order.
    for step in &linked.order {
        match step {
            Step::Esm(module) => {
                let piece = pieces
                    .remove(module)
                    .expect("every eager module is printed");
                out.push_mapped(&piece.code, piece.map);
            }
            Step::Cjs(module) if *module == linked.entry => {
                out.push(&format!("{p}runMain({p}m{module});"));
            }
            Step::Cjs(module) => out.push(&cjs_step(*module, linked, p)),
            Step::Json(module) => out.push(&json_step(*module, linked, p)),
            Step::Stylesheet(module) => out.push(&format!("{p}css({p}asset{module});")),
        }
    }

    if linked.entry_async {
        out.push(&format!("{p}entryFinish();"));
    }

    // The bundle exports what the entry exports.
    if !linked.entry_exports.is_empty() {
        let specifiers: Vec<String> = linked
            .entry_exports
            .iter()
            .map(|(name, binding)| match reference(binding) {
                Reference::Name(local) => format!("{local} as {}", export_name_text(name)),
                Reference::Member(..) => unreachable!("the entry is eager"),
            })
            .collect();
        out.push(&format!("export {{ {} }};", specifiers.join(", ")));
    }

    let map = combine_maps(&out.maps, units, linked, file_name);
    out.text
        .push_str(&format!("//# sourceMappingURL={file_name}.map\n"));
    Ok(Bundle {
        code: out.text,
        map,
        assets,
    })
}

fn namespace_call(
    p: &str,
    exports: &[(String, Binding)],
    reference: &dyn Fn(&Binding) -> Reference,
) -> String {
    let keys: Vec<String> = exports.iter().map(|(name, _)| json_string(name)).collect();
    let getters: Vec<String> = exports
        .iter()
        .map(|(name, binding)| {
            let value = match reference(binding) {
                Reference::Name(local) => local,
                Reference::Member(object, property) => {
                    format!("{object}[{}]", json_string(&property))
                }
            };
            format!("{}: () => {value}", json_string(name))
        })
        .collect();
    format!(
        "{p}ns([{}], {{ __proto__: null, {} }})",
        keys.join(", "),
        getters.join(", ")
    )
}

/// The table a CommonJS module's `require` reads: every literal specifier
/// the graph resolved from it, to a bundled record or to `0` for Node to
/// load.
fn require_table(module: usize, linked: &Linked) -> String {
    let p = &linked.prefix;
    let mut entries = vec!["__proto__: null".to_string()];
    let mut seen = HashSet::new();
    for (specifier, kind, target) in &linked.requires[module] {
        if !kind.is_require_like() || !seen.insert(specifier.clone()) {
            continue;
        }
        let value = match target {
            Target::Module(t) if matches!(linked.roles[*t], Role::Cjs | Role::Json) => {
                format!("{p}m{t}")
            }
            _ => "0".to_string(),
        };
        entries.push(format!("{}: {value}", json_string(specifier)));
    }
    format!("{{ {} }}", entries.join(", "))
}

/// A CommonJS module's place in the ESM order.
fn cjs_step(module: usize, linked: &Linked, p: &str) -> String {
    let mut text = format!("{{ const s = {p}importCjs({p}m{module}, {p}names{module});");
    if let Some(names) = linked.cjs_exports.get(&module) {
        for (index, name) in names.iter().enumerate() {
            let _ = write!(text, " {p}c{module}_{index} = s[{}];", json_string(name));
        }
    }
    if linked.namespaces.contains_key(&module) {
        let _ = write!(
            text,
            " {p}ns{module} = {p}cjsNs({p}m{module}, {p}names{module}, {p}sorted{module});"
        );
    }
    text.push_str(" }");
    text
}

fn json_step(module: usize, linked: &Linked, p: &str) -> String {
    let mut text = format!("{p}json{module} = {p}load({p}m{module}, undefined, false);");
    if linked.namespaces.contains_key(&module) {
        let _ = write!(
            text,
            " {p}ns{module} = {p}ns([\"default\"], {{ __proto__: null, default: () => {p}json{module} }});"
        );
    }
    text
}

/// What a lazy module does before its body: its requested modules, in
/// order (§16.2.1.6.1.3.1 step 11).
fn dependency_steps(module: usize, linked: &Linked, units: &[Unit<'_>], p: &str) -> String {
    let mut text = String::new();
    let mut seen = HashSet::new();
    for target in &linked.targets[module] {
        let Target::Module(t) = target else {
            continue;
        };
        if !seen.insert(*t) {
            continue;
        }
        match linked.roles[*t] {
            Role::Lazy => {
                let _ = writeln!(text, "{p}init{t}();");
            }
            Role::Cjs => {
                let _ = writeln!(text, "{}", cjs_step(*t, linked, p));
            }
            Role::Json => {
                let _ = writeln!(text, "{}", json_step(*t, linked, p));
            }
            Role::Asset if units[*t].source.path.ends_with(".css") => {
                let _ = writeln!(text, "{p}css({p}asset{t});");
            }
            _ => {}
        }
    }
    let _ = units;
    text
}

#[derive(Default)]
struct Uses {
    import: bool,
    import_any: bool,
    set_name: bool,
    const_assign: bool,
}

struct Piece {
    code: String,
    map: Option<SourceMap<'static>>,
    /// Statements that give renamed functions their names back, run before
    /// any module body.
    fixups: Vec<String>,
    sloppy: bool,
    needs_paths: bool,
}

fn print_module(
    module: usize,
    unit: &mut Unit<'_>,
    linked: &Linked,
    options: &BuildOptions,
    defines: &Defines,
    reference: &dyn Fn(&Binding) -> Reference,
    uses: &mut Uses,
) -> Result<Piece, Diagnostic> {
    let allocator = unit.allocator;
    let info = unit.info;
    let role = linked.roles[module];
    let p = linked.prefix.clone();
    let parsed = unit.parsed.as_mut().expect("a printed module was parsed");
    let mut fixups = Vec::new();

    // Import bindings, and how each prints.
    let mut imports: HashMap<SymbolId, Reference> = HashMap::new();
    if matches!(role, Role::Eager | Role::Lazy) {
        for (import, binding) in info.esm.imports.iter().zip(&linked.imports[module]) {
            imports.insert(import.local, reference(binding));
        }
    }
    // Own top-level bindings renamed in the bundle: symbol -> (written, final).
    let mut renamed: HashMap<SymbolId, (String, String)> = HashMap::new();
    if role == Role::Eager {
        for binding in &info.esm.top_level {
            if let Some(name) = linked.names.get(&(module, binding.symbol))
                && *name != binding.name
            {
                renamed.insert(binding.symbol, (binding.name.clone(), name.clone()));
            }
        }
    }

    let mut rewriter = Rewriter {
        allocator,
        scoping: &parsed.scoping,
        module,
        role,
        prefix: &p,
        imports: &imports,
        renamed: &renamed,
        linked,
        is_entry: module == linked.entry,
        entry_async: linked.entry_async,
        node: options.target == Runtime::Node,
        defines,
        error: None,
        needs_paths: false,
        uses,
        class_inner: HashMap::new(),
        class_self: Vec::new(),
        source_path: &unit.source.path,
        lines: &parsed.lines,
        text: parsed.text,
    };

    if matches!(role, Role::Eager | Role::Lazy) {
        rewriter.rewrite_module_items(&mut parsed.program, &mut fixups, info);
    }
    rewriter.visit_program(&mut parsed.program);
    if let Some(error) = rewriter.error.take() {
        return Err(error);
    }
    let needs_paths = rewriter.needs_paths;
    let removed_symbols: HashSet<SymbolId> = linked
        .removed
        .get(&module)
        .into_iter()
        .flatten()
        .flat_map(|index| info.esm.statements[*index].declares.iter().copied())
        .collect();
    for (symbol, (written, _)) in &renamed {
        if parsed.scoping.symbol_flags(*symbol).is_function() && !removed_symbols.contains(symbol) {
            let name = &linked.names[&(module, *symbol)];
            fixups.push(format!("{p}setName({name}, {});", json_string(written)));
        }
    }
    fixups.sort();

    // Names, through scoping: the bundle's names for this module's own
    // top-level bindings, and each import binding's target's name.
    let mut scoping = std::mem::take(&mut parsed.scoping);
    for (symbol, (_, name)) in &renamed {
        scoping.set_symbol_name(*symbol, allocator.alloc_str(name).into());
    }
    for (symbol, printed) in &imports {
        if let Reference::Name(name) = printed {
            scoping.set_symbol_name(*symbol, allocator.alloc_str(name).into());
        }
    }

    parsed.program.directives.clear();
    parsed.program.hashbang = None;
    let sloppy = role == Role::Cjs && (!info.strict || info.direct_eval);
    let mut codegen_options = CodegenOptions::default();
    if !sloppy {
        codegen_options.source_map_path = Some(PathBuf::from(unit.rel.clone()));
    }
    if role == Role::Cjs {
        // A CommonJS module keeps its "use strict".
        if info.strict {}
    }
    let printed = Codegen::new()
        .with_options(codegen_options)
        .with_scoping(Some(scoping))
        .build(&parsed.program);
    let mut code = printed.code;
    if role == Role::Cjs && info.strict {
        code.insert_str(0, "\"use strict\";\n");
    }
    let map = if sloppy {
        None
    } else {
        printed.map.map(|map| {
            let mut map = map.into_owned();
            if role == Role::Cjs && info.strict {
                map = shift_map(map, 1);
            }
            map
        })
    };
    Ok(Piece {
        code,
        map,
        fixups,
        sloppy,
        needs_paths,
    })
}

/// A map whose generated lines start `lines` later.
fn shift_map(map: SourceMap<'static>, lines: u32) -> SourceMap<'static> {
    let builder = oxc_sourcemap::ConcatSourceMapBuilder::from_sourcemaps(&[(&map, lines)]);
    builder.into_sourcemap().into_owned()
}

fn combine_maps(
    maps: &[(SourceMap<'static>, u32)],
    units: &[Unit<'_>],
    linked: &Linked,
    file_name: &str,
) -> String {
    let refs: Vec<(&SourceMap<'static>, u32)> =
        maps.iter().map(|(map, line)| (map, *line)).collect();
    let mut combined =
        oxc_sourcemap::ConcatSourceMapBuilder::from_sourcemaps(&refs).into_sourcemap();
    combined.set_file(file_name);
    // Every source's text, so the map stands on its own.
    let by_rel: HashMap<&str, &str> = units
        .iter()
        .zip(&linked.roles)
        .filter(|(_, role)| matches!(role, Role::Eager | Role::Lazy | Role::Cjs))
        .map(|(unit, _)| (unit.rel.as_str(), unit.source.text.as_str()))
        .collect();
    let contents: Vec<Option<&str>> = combined
        .get_sources()
        .map(|source| by_rel.get(source).copied())
        .collect::<Vec<_>>();
    combined.set_source_contents(contents);
    combined.to_json_string()
}

pub fn json_string(text: &str) -> String {
    serde_json::to_string(text).expect("strings serialize")
}

/// An export or import name as it appears after `as`: an identifier, or a
/// string literal for anything else.
fn export_name_text(name: &str) -> String {
    let mut chars = name.chars();
    let is_identifier = chars
        .next()
        .is_some_and(|c| c == '_' || c == '$' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c == '$' || c.is_alphanumeric());
    if is_identifier {
        name.to_string()
    } else {
        json_string(name)
    }
}

/// Rewrites one module's AST for the bundle.
struct Rewriter<'a, 'r> {
    allocator: &'a Allocator,
    scoping: &'r Scoping,
    module: usize,
    role: Role,
    prefix: &'r str,
    imports: &'r HashMap<SymbolId, Reference>,
    renamed: &'r HashMap<SymbolId, (String, String)>,
    linked: &'r Linked,
    is_entry: bool,
    /// The entry module uses top-level await.
    entry_async: bool,
    node: bool,
    defines: &'r Defines,
    error: Option<Diagnostic>,
    needs_paths: bool,
    uses: &'r mut Uses,
    /// Renamed top-level classes: inner name -> the class's symbol.
    class_inner: HashMap<String, SymbolId>,
    /// Inside a renamed class: its symbol and inner name.
    class_self: Vec<(SymbolId, String)>,
    source_path: &'r str,
    lines: &'r LineIndex,
    text: &'r str,
}

impl<'a> Rewriter<'a, '_> {
    fn fail(&mut self, code: Code, span: Span, message: impl Into<String>) {
        if self.error.is_none() {
            self.error = Some(
                Diagnostic::new(code, self.source_path, message.into())
                    .at(self.lines.locate(self.text, span.start)),
            );
        }
    }

    fn symbol_of(&self, reference: &oxc_ast::ast::IdentifierReference<'_>) -> Option<SymbolId> {
        let id = reference.reference_id.get()?;
        self.scoping.get_reference(id).symbol_id()
    }

    /// Parses `text` as an expression, with each `__hole<n>` replaced by
    /// `holes[n]`, and every new node given `span`.
    fn template(&self, text: &str, holes: Vec<Expression<'a>>, span: Span) -> Expression<'a> {
        let text = self.allocator.alloc_str(text);
        let mut expression = Parser::new(self.allocator, text, SourceType::mjs())
            .parse_expression()
            .unwrap_or_else(|_| panic!("template parses: {text}"));
        let mut filler = Filler {
            holes: holes.into_iter().map(Some).collect(),
            span,
        };
        filler.visit_expression(&mut expression);
        expression
    }

    fn template_statement(
        &self,
        text: &str,
        holes: Vec<Expression<'a>>,
        span: Span,
    ) -> Statement<'a> {
        let text = self.allocator.alloc_str(text);
        let mut program = Parser::new(self.allocator, text, SourceType::mjs())
            .parse()
            .program;
        let mut statement = program.body.remove(0);
        let mut filler = Filler {
            holes: holes.into_iter().map(Some).collect(),
            span,
        };
        filler.visit_statement(&mut statement);
        statement
    }

    /// Imports and exports out; declarations stay; `export default`
    /// becomes its binding.
    fn rewrite_module_items(
        &mut self,
        program: &mut Program<'a>,
        fixups: &mut Vec<String>,
        info: &Info,
    ) {
        let p = self.prefix.to_string();
        let module = self.module;
        let body = program.body.take_in(&self.allocator);
        let mut out = oxc_allocator::Vec::with_capacity_in(body.len(), &self.allocator);
        let removed = self.linked.removed.get(&module);
        for (index, statement) in body.into_iter().enumerate() {
            if removed.is_some_and(|removed| removed.contains(&index)) {
                continue;
            }
            let statement = match statement {
                Statement::ImportDeclaration(_)
                | Statement::ExportNamedDeclaration(_)
                | Statement::ExportFromDeclaration(_)
                | Statement::ExportAllDeclaration(_)
                | Statement::TSNamespaceExportDeclaration(_) => continue,
                Statement::ExportDeclaration(mut export) => {
                    Statement::from(export.declaration.take_in(&self.allocator))
                }
                Statement::ExportDefaultDeclaration(mut export) => {
                    let span = export.span;
                    let name = format!("{p}default{module}");
                    match export.declaration.take_in(&self.allocator) {
                        ExportDefaultDeclarationKind::FunctionDeclaration(mut function) => {
                            if function.id.is_none() {
                                let ident = self.allocator.alloc_str(&name);
                                let builder = oxc_ast::builder::AstBuilder::new(self.allocator);
                                function.id = Some(oxc_ast::ast::BindingIdentifier::new(
                                    Span::empty(span.start),
                                    ident,
                                    &builder,
                                ));
                                fixups.push(format!("{p}setName({name}, \"default\");"));
                                self.uses.set_name = true;
                            }
                            Statement::FunctionDeclaration(function)
                        }
                        ExportDefaultDeclarationKind::ClassDeclaration(mut class) => {
                            if class.id.is_none() {
                                class.r#type = ClassType::ClassExpression;
                                let expression = Expression::ClassExpression(class);
                                self.template_statement(
                                    &format!("let {name} = ({{ default: __hole0 }}).default;"),
                                    vec![expression],
                                    span,
                                )
                            } else {
                                Statement::ClassDeclaration(class)
                            }
                        }
                        kind => {
                            if !kind.is_expression() {
                                continue;
                            }
                            let expression = kind.into_expression();
                            let expression = if is_anonymous_function(&expression) {
                                self.template(
                                    "({ default: __hole0 }).default",
                                    vec![expression],
                                    span,
                                )
                            } else {
                                expression
                            };
                            debug_assert!(matches!(
                                info.esm.default,
                                Some(DefaultKind::Expression)
                            ));
                            self.template_statement(
                                &format!("const {name} = __hole0;"),
                                vec![expression],
                                span,
                            )
                        }
                    }
                }
                other => other,
            };
            // A renamed top-level class: `let F = class <inner> {…}`, so its
            // `.name` and its body's references to itself stay what they were.
            let statement = match statement {
                Statement::ClassDeclaration(mut class) if self.role == Role::Eager => {
                    let symbol = class.id.as_ref().and_then(|id| id.symbol_id.get());
                    match symbol.and_then(|s| self.renamed.get(&s).map(|names| (s, names.clone())))
                    {
                        Some((symbol, (written, _))) => {
                            let span = class.span;
                            let inner = format!("{p}class{}", symbol.index());
                            let id = class.id.take().expect("a renamed class has a name");
                            let builder = oxc_ast::builder::AstBuilder::new(self.allocator);
                            class.id = Some(oxc_ast::ast::BindingIdentifier::new(
                                id.span,
                                self.allocator.alloc_str(&inner),
                                &builder,
                            ));
                            class.r#type = ClassType::ClassExpression;
                            let block = self.static_set_class_name(&written, span);
                            class.body.body.insert(0, block);
                            self.uses.set_name = true;
                            self.class_inner.insert(inner, symbol);
                            let expression = Expression::ClassExpression(class);
                            let mut statement = self.template_statement(
                                "let __binding = __hole0;",
                                vec![expression],
                                span,
                            );
                            if let Statement::VariableDeclaration(declaration) = &mut statement
                                && let BindingPattern::BindingIdentifier(binding) =
                                    &mut declaration.declarations[0].id
                            {
                                binding.symbol_id.set(Some(symbol));
                                binding.name = id.name;
                                binding.span = id.span;
                            }
                            statement
                        }
                        None => Statement::ClassDeclaration(class),
                    }
                }
                other => other,
            };
            out.push(statement);
        }
        program.body = out;
    }

    fn static_set_class_name(&self, written: &str, span: Span) -> ClassElement<'a> {
        let text = self.allocator.alloc_str(&format!(
            "(class {{ static {{ {}setClassName(this, {}); }} }})",
            self.prefix,
            json_string(written)
        ));
        let expression = Parser::new(self.allocator, text, SourceType::mjs())
            .parse_expression()
            .expect("the class name block parses");
        let Expression::ParenthesizedExpression(mut parenthesized) = expression else {
            unreachable!("a parenthesized class")
        };
        let Expression::ClassExpression(mut class) =
            parenthesized.expression.take_in(&self.allocator)
        else {
            unreachable!("a class expression")
        };
        let mut element = class.body.body.remove(0);
        let mut filler = Filler {
            holes: Vec::new(),
            span,
        };
        filler.visit_class_element(&mut element);
        element
    }

    /// `({ "N": <expression> })["N"]`: the same NamedEvaluation the original binding
    /// name gave it (§10.2.9).
    fn named(&self, name: &str, expression: Expression<'a>) -> Expression<'a> {
        // String keys: a literal property name gives NamedEvaluation the
        // same name (§13.2.5.5), and the template then writes no identifier
        // a source map could point at the wrong token for.
        let span = expression.span();
        let key = json_string(name);
        self.template(
            &format!("({{ {key}: __hole0 }})[{key}]"),
            vec![expression],
            span,
        )
    }

    fn renamed_written(&self, symbol: SymbolId) -> Option<String> {
        self.renamed
            .get(&symbol)
            .map(|(written, _)| written.clone())
    }

    fn import_reference(
        &self,
        reference: &oxc_ast::ast::IdentifierReference<'_>,
    ) -> Option<(SymbolId, &Reference)> {
        let symbol = self.symbol_of(reference)?;
        self.imports.get(&symbol).map(|printed| (symbol, printed))
    }
}

impl<'a> VisitMut<'a> for Rewriter<'a, '_> {
    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        let span = expression.span();
        // import.meta.<property>
        if let Expression::StaticMemberExpression(member) = expression
            && let Expression::ImportMeta(_) = &member.object
        {
            let p = self.prefix;
            let m = self.module;
            let property = member.property.name.as_str();
            let replacement = match (property, self.node) {
                ("url", _) => Some(format!("{p}url{m}")),
                ("filename", true) => Some(format!("{p}file{m}")),
                ("dirname", true) => Some(format!("{p}dir{m}")),
                ("main", _) if self.is_entry => None,
                ("main", _) => Some("false".to_string()),
                _ => {
                    self.fail(
                        Code::ImportMeta,
                        span,
                        format!("import.meta.{property} has no meaning under --target browser"),
                    );
                    return;
                }
            };
            if let Some(replacement) = replacement {
                if replacement != "false" {
                    self.needs_paths = true;
                }
                *expression = self.template(&replacement, Vec::new(), span);
            }
            return;
        }
        // import.meta.resolve('./relative')
        if let Expression::CallExpression(call) = expression
            && let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::ImportMeta(_) = &member.object
            && let Some(Argument::StringLiteral(specifier)) = call.arguments.first()
        {
            self.needs_paths = true;
            let text = format!(
                "new {}URL({}, {}url{}).href",
                self.prefix,
                json_string(&specifier.value),
                self.prefix,
                self.module
            );
            *expression = self.template(&text, Vec::new(), span);
            return;
        }
        // import('literal')
        if let Expression::ImportExpression(import) = expression {
            let specifier = match &import.source {
                Expression::StringLiteral(literal) => Some(literal.value.to_string()),
                Expression::TemplateLiteral(template) => template
                    .quasis
                    .first()
                    .and_then(|quasi| quasi.value.cooked.as_ref())
                    .map(|cooked| cooked.to_string()),
                _ => None,
            };
            if import.options.is_some() {
                self.fail(Code::Dynamic, span, "import() with options is not bundled");
                return;
            }
            let target = specifier.as_ref().and_then(|specifier| {
                self.linked.requires[self.module]
                    .iter()
                    .find(|(s, kind, _)| s == specifier && !kind.is_require_like())
                    .map(|(_, _, target)| target.clone())
            });
            let p = self.prefix;
            if target.is_none() {
                // Resolved when it runs (rule 10): an expression, or a
                // literal the graph did not resolve.
                self.uses.import_any = true;
                self.needs_paths = true;
                let mut source = import.source.take_in(&self.allocator);
                self.visit_expression(&mut source);
                let text = format!("{p}importAny(__hole0, {p}url{})", self.module);
                *expression = self.template(&text, vec![source], span);
                return;
            }
            let thunk = match target {
                Some(Target::Module(t)) => match self.linked.roles[t] {
                    // The entry, while its top-level await is pending, is
                    // still evaluating: an import() of it settles when it
                    // completes, as §16.2.1.6.1.3 has it wait.
                    Role::Eager if t == self.linked.entry && self.entry_async => {
                        format!("() => {p}entryDone.then(() => {p}ns{t})")
                    }
                    Role::Eager => format!("() => {p}ns{t}"),
                    Role::Lazy => format!("() => ({p}init{t}(), {p}ns{t})"),
                    Role::Cjs => format!("() => {p}cjsNs({p}m{t}, {p}names{t}, {p}sorted{t})"),
                    _ => return,
                },
                // External or builtin: Node loads it.
                _ => return,
            };
            self.uses.import = true;
            *expression = self.template(&format!("{p}import({thunk})"), Vec::new(), span);
            return;
        }
        // A write to an import binding throws, after the right-hand side.
        if let Expression::AssignmentExpression(assignment) = expression
            && let AssignmentTarget::AssignmentTargetIdentifier(target) = &assignment.left
            && self.import_reference(target).is_some()
        {
            self.uses.const_assign = true;
            let p = self.prefix;
            let (name, target_span, reference_id) = (
                target.name.to_string(),
                target.span,
                target.reference_id.get(),
            );
            let mut right = assignment.right.take_in(&self.allocator);
            self.visit_expression(&mut right);
            let mut left = self.template(&name, Vec::new(), target_span);
            if let Expression::Identifier(ident) = &mut left {
                ident.reference_id.set(reference_id);
            }
            self.visit_expression(&mut left);
            let text = match assignment.operator {
                AssignmentOperator::Assign => format!("(__hole1, {p}constAssign())"),
                AssignmentOperator::LogicalAnd => {
                    format!("(__hole0 && (__hole1, {p}constAssign()))")
                }
                AssignmentOperator::LogicalOr => {
                    format!("(__hole0 || (__hole1, {p}constAssign()))")
                }
                AssignmentOperator::LogicalNullish => {
                    format!("(__hole0 ?? (__hole1, {p}constAssign()))")
                }
                operator => format!(
                    "((__hole0 {} __hole1), {p}constAssign())",
                    operator.to_binary_operator().map_or("+", |op| op.as_str())
                ),
            };
            *expression = self.template(&text, vec![left, right], span);
            return;
        }
        if let Expression::UpdateExpression(update) = expression
            && let SimpleAssignmentTarget::AssignmentTargetIdentifier(target) = &update.argument
            && self.import_reference(target).is_some()
        {
            self.uses.const_assign = true;
            let mut left = self.template(target.name.as_str(), Vec::new(), target.span);
            if let Expression::Identifier(ident) = &mut left {
                ident.reference_id.set(target.reference_id.get());
            }
            self.visit_expression(&mut left);
            let p = self.prefix;
            *expression = self.template(
                &format!("({p}toNumeric(__hole0), {p}constAssign())"),
                vec![left],
                span,
            );
            return;
        }
        // NamedEvaluation through a renamed binding: `x = function () {}`.
        if let Expression::AssignmentExpression(assignment) = expression
            && matches!(
                assignment.operator,
                AssignmentOperator::Assign
                    | AssignmentOperator::LogicalAnd
                    | AssignmentOperator::LogicalOr
                    | AssignmentOperator::LogicalNullish
            )
            && let AssignmentTarget::AssignmentTargetIdentifier(target) = &assignment.left
            && let Some(symbol) = self.symbol_of(target)
            && let Some(written) = self.renamed_written(symbol)
            && is_anonymous_function(&assignment.right)
        {
            let right = assignment.right.take_in(&self.allocator);
            assignment.right = self.named(&written, right);
        }
        // An import binding of a lazy module: a property of its bindings
        // object. A call through it passes `this` as undefined.
        if let Expression::CallExpression(call) = expression
            && let Expression::Identifier(callee) = &call.callee
            && let Some((_, Reference::Member(object, property))) = self.import_reference(callee)
        {
            let text = format!("(0, {object}[{}])", json_string(property));
            call.callee = self.template(&text, Vec::new(), callee.span);
        }
        if let Expression::TaggedTemplateExpression(tagged) = expression
            && let Expression::Identifier(tag) = &tagged.tag
            && let Some((_, Reference::Member(object, property))) = self.import_reference(tag)
        {
            let text = format!("(0, {object}[{}])", json_string(property));
            tagged.tag = self.template(&text, Vec::new(), tag.span);
        }
        if let Expression::Identifier(ident) = expression {
            if let Some((_, Reference::Member(object, property))) = self.import_reference(ident) {
                let text = format!("{object}[{}]", json_string(property));
                *expression = self.template(&text, Vec::new(), span);
                return;
            }
            if let Some(symbol) = self.symbol_of(ident)
                && let Some((_, inner)) = self.class_self.iter().rev().find(|(s, _)| *s == symbol)
            {
                // Inside a renamed class, its own name is the inner binding.
                let inner = self.allocator.alloc_str(inner);
                ident.name = inner.into();
                ident.reference_id.set(None);
                return;
            }
        }
        // --define
        if let Some(value) = self.defines.matches(expression, self.scoping) {
            *expression = self.template(&format!("({value})"), Vec::new(), span);
            return;
        }
        walk_mut::walk_expression(self, expression);
    }

    fn visit_simple_assignment_target(&mut self, target: &mut SimpleAssignmentTarget<'a>) {
        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) = target
            && self.import_reference(ident).is_some()
        {
            self.fail(
                Code::Unsupported,
                ident.span,
                format!(
                    "a destructuring or for-in/of assignment to the import binding {:?}, which throws TypeError when it runs; the bundler does not rewrite this form",
                    ident.name.as_str()
                ),
            );
            return;
        }
        if self.defines.matches_target(target, self.scoping) {
            let span = target.span();
            self.fail(Code::Define, span, "assignment to a --define'd path");
            return;
        }
        walk_mut::walk_simple_assignment_target(self, target);
    }

    fn visit_variable_declarator(&mut self, declarator: &mut oxc_ast::ast::VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Some(symbol) = id.symbol_id.get()
            && let Some(written) = self.renamed_written(symbol)
            && let Some(init) = &mut declarator.init
            && is_anonymous_function(init)
        {
            let expression = init.take_in(&self.allocator);
            *init = self.named(&written, expression);
        }
        walk_mut::walk_variable_declarator(self, declarator);
    }

    fn visit_assignment_pattern(&mut self, pattern: &mut oxc_ast::ast::AssignmentPattern<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &pattern.left
            && let Some(symbol) = id.symbol_id.get()
            && let Some(written) = self.renamed_written(symbol)
            && is_anonymous_function(&pattern.right)
        {
            let expression = pattern.right.take_in(&self.allocator);
            pattern.right = self.named(&written, expression);
        }
        walk_mut::walk_assignment_pattern(self, pattern);
    }

    fn visit_assignment_target_with_default(
        &mut self,
        target: &mut oxc_ast::ast::AssignmentTargetWithDefault<'a>,
    ) {
        if let AssignmentTarget::AssignmentTargetIdentifier(id) = &target.binding
            && let Some(symbol) = self.symbol_of(id)
            && let Some(written) = self.renamed_written(symbol)
            && is_anonymous_function(&target.init)
        {
            let expression = target.init.take_in(&self.allocator);
            target.init = self.named(&written, expression);
        }
        walk_mut::walk_assignment_target_with_default(self, target);
    }

    fn visit_assignment_target_property_identifier(
        &mut self,
        property: &mut oxc_ast::ast::AssignmentTargetPropertyIdentifier<'a>,
    ) {
        if let Some(symbol) = self.symbol_of(&property.binding)
            && let Some(written) = self.renamed_written(symbol)
            && let Some(init) = &mut property.init
            && is_anonymous_function(init)
        {
            let expression = init.take_in(&self.allocator);
            *init = self.named(&written, expression);
        }
        if self.import_reference(&property.binding).is_some() {
            self.fail(
                Code::Unsupported,
                property.span,
                "a destructuring assignment to an import binding, which throws TypeError when it runs; the bundler does not rewrite this form",
            );
            return;
        }
        walk_mut::walk_assignment_target_property_identifier(self, property);
    }

    fn visit_class(&mut self, class: &mut oxc_ast::ast::Class<'a>) {
        let inner = class.id.as_ref().and_then(|id| {
            self.class_inner
                .get(id.name.as_str())
                .map(|symbol| (*symbol, id.name.to_string()))
        });
        match inner {
            Some(inner) => {
                self.class_self.push(inner);
                walk_mut::walk_class(self, class);
                self.class_self.pop();
            }
            None => walk_mut::walk_class(self, class),
        }
    }
}

/// IsAnonymousFunctionDefinition (§8.4.3), looking through parentheses.
fn is_anonymous_function(expression: &Expression<'_>) -> bool {
    match expression.without_parentheses() {
        Expression::FunctionExpression(function) => function.id.is_none(),
        Expression::ArrowFunctionExpression(_) => true,
        Expression::ClassExpression(class) => class.id.is_none(),
        _ => false,
    }
}

/// Puts holes into a parsed template, and gives the template's own nodes
/// empty spans, so the only mappings are the holes' own.
struct Filler<'a> {
    holes: Vec<Option<Expression<'a>>>,
    /// Where the template's own nodes sit: an empty span, so they carry no
    /// mapping and nothing points at original code they did not come from.
    span: Span,
}

impl<'a> VisitMut<'a> for Filler<'a> {
    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        if let Expression::Identifier(ident) = expression
            && let Some(index) = ident.name.strip_prefix("__hole")
            && let Ok(index) = index.parse::<usize>()
            && let Some(hole) = self.holes.get_mut(index).and_then(Option::take)
        {
            *expression = hole;
            return;
        }
        walk_mut::walk_expression(self, expression);
    }

    fn visit_span(&mut self, span: &mut Span) {
        *span = Span::empty(self.span.start);
    }
}
