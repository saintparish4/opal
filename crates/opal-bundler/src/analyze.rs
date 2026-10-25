//! What linking needs to know about each module, read from its compiled AST.
//!
//! For an ES module: its requested modules in source order (ECMA-262
//! §16.2.1.7, `ModuleRequests`), its import and export entries, its
//! top-level bindings, and for each of those every scope it is read from
//! (which is what the renamer checks for capture). For a CommonJS module:
//! whether it is strict, and the globals it reads. For both: the globals,
//! since nothing in the bundle may shadow one, and anything `SEMANTICS.md`
//! refuses.

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Declaration, ExportDefaultDeclarationKind,
    Expression, ImportDeclarationSpecifier, ImportExpression, ImportMeta, ModuleDeclaration,
    ModuleExportName, Program, Statement, StaticMemberExpression, WithClause,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SymbolFlags, SymbolId};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::{ScopeFlags, ScopeId};

use crate::diagnostic::{Code, Diagnostic};
use crate::load::{Format, Parsed, Source};

/// An import or `export … from` target, deduplicated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub specifier: String,
    /// `with { type: 'json' }`.
    pub json: bool,
    /// Any attribute other than `type: 'json'`.
    pub other_attribute: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImportName {
    Name(String),
    Namespace,
}

#[derive(Clone, Debug)]
pub struct Import {
    pub local: SymbolId,
    pub local_name: String,
    pub request: usize,
    pub name: ImportName,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportTarget {
    /// A top-level binding of this module, possibly an import binding.
    Local(SymbolId),
    /// The `*default*` binding `export default <expression>` and an
    /// anonymous `export default function`/`class` create.
    Default,
    /// `export { a as b } from 'm'` and `export * as ns from 'm'`.
    Indirect { request: usize, name: ImportName },
}

#[derive(Clone, Debug)]
pub struct Export {
    pub name: String,
    pub target: ExportTarget,
    pub span: Span,
}

/// What the `*default*` binding holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultKind {
    Expression,
    AnonymousFunction,
    AnonymousClass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    Var,
    Function,
    /// `let`, `const`, `class`: in the TDZ until their declaration runs.
    Lexical,
    Class,
    Import,
}

#[derive(Clone, Debug)]
pub struct TopLevel {
    pub symbol: SymbolId,
    pub name: String,
    pub kind: BindingKind,
}

/// Where a top-level binding is read or written from.
#[derive(Clone, Debug, Default)]
pub struct Uses {
    /// Distinct scopes containing a reference.
    pub scopes: Vec<ScopeId>,
    pub written: bool,
}

#[derive(Clone, Debug, Default)]
pub struct EsmInfo {
    pub requests: Vec<Request>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub stars: Vec<(usize, Span)>,
    pub default: Option<DefaultKind>,
    pub top_level: Vec<TopLevel>,
    pub uses: HashMap<SymbolId, Uses>,
    /// Top-level `await` or `for await`.
    pub top_level_await: Option<Span>,
    /// Each top-level statement, for tree shaking.
    pub statements: Vec<crate::shake::TopStatement>,
}

/// Facts about any module.
#[derive(Clone, Debug)]
pub struct Info {
    pub path: String,
    pub format: Format,
    pub esm: EsmInfo,
    /// For CommonJS: begins with `"use strict"`.
    pub strict: bool,
    /// For CommonJS: contains a direct `eval`, which would see the bundle's
    /// own scope if the module were inlined.
    pub direct_eval: bool,
    /// Names read but never declared: globals.
    pub globals: Vec<String>,
    /// Every name the module declares or reads, for picking a helper
    /// prefix nothing in the program uses.
    pub names: HashSet<String>,
    /// `import()` calls with a literal specifier, in source order.
    pub dynamic_imports: Vec<(String, Span)>,
    /// An `import()` of anything else, resolved when it runs (rule 10).
    pub runtime_imports: bool,
}

impl Info {
    /// A JSON or opaque module, which declares and reads nothing.
    pub fn opaque(source: &Source) -> Self {
        Self {
            path: source.path.clone(),
            format: source.format,
            esm: EsmInfo::default(),
            strict: false,
            direct_eval: false,
            globals: Vec::new(),
            names: HashSet::new(),
            dynamic_imports: Vec::new(),
            runtime_imports: false,
        }
    }
}

/// Names a CommonJS module reads from its wrapper, not from the global
/// scope.
pub const WRAPPER_NAMES: &[&str] = &["exports", "require", "module", "__filename", "__dirname"];

pub fn analyze(source: &Source, parsed: &Parsed<'_>) -> Result<Info, Diagnostic> {
    let scoping = &parsed.scoping;
    let root = scoping.root_scope_id();
    let mut info = Info {
        path: source.path.clone(),
        format: source.format,
        esm: EsmInfo::default(),
        strict: parsed
            .program
            .directives
            .iter()
            .any(|directive| directive.directive == "use strict"),
        direct_eval: false,
        globals: Vec::new(),
        names: HashSet::new(),
        dynamic_imports: Vec::new(),
        runtime_imports: false,
    };
    for name in scoping.symbol_names() {
        info.names.insert(name.to_string());
    }
    let mut globals: Vec<String> = scoping
        .root_unresolved_references()
        .keys()
        .map(|name| name.to_string())
        .collect();
    globals.sort();
    for name in &globals {
        info.names.insert(name.clone());
    }
    if source.format == Format::Cjs {
        globals.retain(|name| !WRAPPER_NAMES.contains(&name.as_str()) && name != "arguments");
    }
    info.globals = globals;

    let refuse = |code: Code, span: Span, message: String| {
        Diagnostic::new(code, &source.path, message)
            .at(parsed.lines.locate(parsed.text, span.start))
    };

    let mut walker = Walker {
        scoping,
        root,
        format: source.format,
        strict: info.strict,
        scopes: vec![root],
        function_depth: 0,
        uses: HashMap::new(),
        top_level_await: None,
        dynamic_imports: Vec::new(),
        runtime_imports: false,
        refusal: None,
        direct_eval: false,
        skip_meta: HashSet::new(),
    };
    walker.visit_program(&parsed.program);
    if let Some((code, span, message)) = walker.refusal.take() {
        return Err(refuse(code, span, message));
    }
    info.direct_eval = walker.direct_eval;
    info.dynamic_imports = walker.dynamic_imports;
    info.runtime_imports = walker.runtime_imports;

    if source.format == Format::Esm {
        info.esm = esm_entries(&parsed.program, scoping, root, &refuse)?;
        info.esm.uses = walker.uses;
        info.esm.top_level_await = walker.top_level_await;
        info.esm.statements = crate::shake::statements(&parsed.program, scoping);
        let mut top_level: Vec<TopLevel> = scoping
            .get_bindings(root)
            .iter()
            .map(|(name, symbol)| {
                let flags = scoping.symbol_flags(*symbol);
                let kind = if flags.contains(SymbolFlags::Import) {
                    BindingKind::Import
                } else if flags.contains(SymbolFlags::Class) {
                    BindingKind::Class
                } else if flags.contains(SymbolFlags::Function) {
                    BindingKind::Function
                } else if flags
                    .intersects(SymbolFlags::BlockScopedVariable | SymbolFlags::ConstVariable)
                {
                    BindingKind::Lexical
                } else {
                    BindingKind::Var
                };
                TopLevel {
                    symbol: *symbol,
                    name: name.to_string(),
                    kind,
                }
            })
            .collect();
        top_level.sort_by_key(|binding| binding.symbol.index());
        info.esm.top_level = top_level;
    }
    Ok(info)
}

fn esm_entries(
    program: &Program<'_>,
    scoping: &Scoping,
    root: ScopeId,
    refuse: &dyn Fn(Code, Span, String) -> Diagnostic,
) -> Result<EsmInfo, Diagnostic> {
    let mut esm = EsmInfo::default();
    let mut request_index: HashMap<(String, bool, Option<String>), usize> = HashMap::new();
    let mut request = |esm: &mut EsmInfo,
                       specifier: &str,
                       with: Option<&WithClause<'_>>,
                       span: Span|
     -> Result<usize, Diagnostic> {
        let (json, other) = attributes(with);
        let key = (specifier.to_string(), json, other.clone());
        if let Some(index) = request_index.get(&key) {
            return Ok(*index);
        }
        let index = esm.requests.len();
        esm.requests.push(Request {
            specifier: specifier.to_string(),
            json,
            other_attribute: other,
            span,
        });
        request_index.insert(key, index);
        Ok(index)
    };

    for statement in &program.body {
        let Some(declaration) = statement.as_module_declaration() else {
            continue;
        };
        match declaration {
            ModuleDeclaration::ImportDeclaration(import) => {
                if import.phase.is_some() {
                    return Err(refuse(
                        Code::Unsupported,
                        import.span,
                        "source and defer phase imports are not supported".to_string(),
                    ));
                }
                let index = request(
                    &mut esm,
                    &import.source.value,
                    import.with_clause.as_deref(),
                    import.span,
                )?;
                for specifier in import.specifiers.iter().flatten() {
                    let (local, name) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            (&s.local, ImportName::Name(export_name(&s.imported)))
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            (&s.local, ImportName::Name("default".to_string()))
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            (&s.local, ImportName::Namespace)
                        }
                    };
                    let Some(symbol) = local.symbol_id.get() else {
                        continue;
                    };
                    esm.imports.push(Import {
                        local: symbol,
                        local_name: local.name.to_string(),
                        request: index,
                        name,
                        span: local.span,
                    });
                }
            }
            ModuleDeclaration::ExportFromDeclaration(export) => {
                let index = request(
                    &mut esm,
                    &export.source.value,
                    export.with_clause.as_deref(),
                    export.span,
                )?;
                for specifier in &export.specifiers {
                    esm.exports.push(Export {
                        name: export_name(&specifier.exported),
                        target: ExportTarget::Indirect {
                            request: index,
                            name: ImportName::Name(export_name(&specifier.local)),
                        },
                        span: specifier.span,
                    });
                }
            }
            ModuleDeclaration::ExportDeclaration(export) => {
                for (name, symbol) in declared(&export.declaration) {
                    esm.exports.push(Export {
                        name,
                        target: ExportTarget::Local(symbol),
                        span: export.span,
                    });
                }
            }
            ModuleDeclaration::ExportNamedDeclaration(export) => {
                for specifier in &export.specifiers {
                    let symbol = match &specifier.local {
                        ModuleExportName::IdentifierReference(reference) => reference
                            .reference_id
                            .get()
                            .and_then(|id| scoping.get_reference(id).symbol_id()),
                        ModuleExportName::IdentifierName(name) => {
                            scoping.get_binding(root, name.name.as_str().into())
                        }
                        ModuleExportName::StringLiteral(_) => None,
                    };
                    let Some(symbol) = symbol else {
                        return Err(refuse(
                            Code::Parse,
                            specifier.span,
                            format!("Export '{}' is not defined", export_name(&specifier.local)),
                        ));
                    };
                    esm.exports.push(Export {
                        name: export_name(&specifier.exported),
                        target: ExportTarget::Local(symbol),
                        span: specifier.span,
                    });
                }
            }
            ModuleDeclaration::ExportDefaultDeclaration(export) => {
                let target = match &export.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                        match &function.id {
                            Some(id) => id.symbol_id.get().map(ExportTarget::Local),
                            None => {
                                esm.default = Some(DefaultKind::AnonymousFunction);
                                Some(ExportTarget::Default)
                            }
                        }
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(class) => match &class.id {
                        Some(id) => id.symbol_id.get().map(ExportTarget::Local),
                        None => {
                            esm.default = Some(DefaultKind::AnonymousClass);
                            Some(ExportTarget::Default)
                        }
                    },
                    _ => {
                        esm.default = Some(DefaultKind::Expression);
                        Some(ExportTarget::Default)
                    }
                };
                if let Some(target) = target {
                    esm.exports.push(Export {
                        name: "default".to_string(),
                        target,
                        span: export.span,
                    });
                }
            }
            ModuleDeclaration::ExportAllDeclaration(export) => {
                let index = request(
                    &mut esm,
                    &export.source.value,
                    export.with_clause.as_deref(),
                    export.span,
                )?;
                match &export.exported {
                    Some(name) => esm.exports.push(Export {
                        name: export_name(name),
                        target: ExportTarget::Indirect {
                            request: index,
                            name: ImportName::Namespace,
                        },
                        span: export.span,
                    }),
                    None => esm.stars.push((index, export.span)),
                }
            }
            ModuleDeclaration::TSExportAssignment(assignment) => {
                return Err(refuse(
                    Code::Ts,
                    assignment.span,
                    "`export =` is CommonJS; an ES module cannot use it".to_string(),
                ));
            }
            ModuleDeclaration::TSNamespaceExportDeclaration(_) => {}
        }
    }
    Ok(esm)
}

/// `type: 'json'`, and the first other attribute if any.
fn attributes(with: Option<&WithClause<'_>>) -> (bool, Option<String>) {
    let Some(with) = with else {
        return (false, None);
    };
    let mut json = false;
    let mut other = None;
    for entry in &with.with_entries {
        let key = match &entry.key {
            oxc_ast::ast::ImportAttributeKey::Identifier(name) => name.name.to_string(),
            oxc_ast::ast::ImportAttributeKey::StringLiteral(literal) => literal.value.to_string(),
        };
        if key == "type" && entry.value.value == "json" {
            json = true;
        } else if other.is_none() {
            other = Some(format!("{key}: {:?}", entry.value.value.as_str()));
        }
    }
    (json, other)
}

pub fn export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(name) => name.name.to_string(),
        ModuleExportName::IdentifierReference(reference) => reference.name.to_string(),
        ModuleExportName::StringLiteral(literal) => literal.value.to_string(),
    }
}

/// The names and symbols a declaration binds, in source order.
pub fn declared(declaration: &Declaration<'_>) -> Vec<(String, SymbolId)> {
    let mut out = Vec::new();
    match declaration {
        Declaration::VariableDeclaration(variables) => {
            for declarator in &variables.declarations {
                collect_pattern(&declarator.id, &mut out);
            }
        }
        Declaration::FunctionDeclaration(function) => {
            if let Some(id) = &function.id
                && let Some(symbol) = id.symbol_id.get()
            {
                out.push((id.name.to_string(), symbol));
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id
                && let Some(symbol) = id.symbol_id.get()
            {
                out.push((id.name.to_string(), symbol));
            }
        }
        _ => {}
    }
    out
}

fn collect_pattern(pattern: &BindingPattern<'_>, out: &mut Vec<(String, SymbolId)>) {
    for id in pattern.get_binding_identifiers() {
        if let Some(symbol) = id.symbol_id.get() {
            out.push((id.name.to_string(), symbol));
        }
    }
}

struct Walker<'s> {
    scoping: &'s Scoping,
    root: ScopeId,
    format: Format,
    strict: bool,
    scopes: Vec<ScopeId>,
    function_depth: u32,
    uses: HashMap<SymbolId, Uses>,
    top_level_await: Option<Span>,
    dynamic_imports: Vec<(String, Span)>,
    runtime_imports: bool,
    refusal: Option<(Code, Span, String)>,
    direct_eval: bool,
    /// `import.meta` nodes already accounted for by their parent.
    skip_meta: HashSet<u32>,
}

impl Walker<'_> {
    fn refuse(&mut self, code: Code, span: Span, message: impl Into<String>) {
        if self.refusal.is_none() {
            self.refusal = Some((code, span, message.into()));
        }
    }
}

const IMPORT_META_PROPERTIES: &[&str] = &["url", "filename", "dirname", "main"];

impl<'a> Visit<'a> for Walker<'_> {
    fn enter_scope(&mut self, flags: ScopeFlags, scope_id: &std::cell::Cell<Option<ScopeId>>) {
        if let Some(scope) = scope_id.get() {
            self.scopes.push(scope);
        }
        if flags.is_function() {
            self.function_depth += 1;
        }
    }

    fn leave_scope(&mut self) {
        // Scopes and functions are entered and left in pairs; the flags of
        // the scope being left are those of the scope on top.
        if let Some(scope) = self.scopes.pop()
            && self.scoping.scope_flags(scope).is_function()
        {
            self.function_depth -= 1;
        }
    }

    fn visit_identifier_reference(&mut self, reference: &oxc_ast::ast::IdentifierReference<'a>) {
        let Some(reference_id) = reference.reference_id.get() else {
            return;
        };
        let resolved = self.scoping.get_reference(reference_id);
        let Some(symbol) = resolved.symbol_id() else {
            return;
        };
        if self.scoping.symbol_scope_id(symbol) != self.root {
            return;
        }
        let scope = *self.scopes.last().unwrap_or(&self.root);
        let uses = self.uses.entry(symbol).or_default();
        if !uses.scopes.contains(&scope) {
            uses.scopes.push(scope);
        }
        if resolved.is_write() {
            uses.written = true;
        }
    }

    fn visit_await_expression(&mut self, expression: &oxc_ast::ast::AwaitExpression<'a>) {
        if self.function_depth == 0 && self.top_level_await.is_none() {
            self.top_level_await = Some(expression.span);
        }
        walk::walk_await_expression(self, expression);
    }

    fn visit_for_of_statement(&mut self, statement: &oxc_ast::ast::ForOfStatement<'a>) {
        if statement.r#await && self.function_depth == 0 && self.top_level_await.is_none() {
            self.top_level_await = Some(statement.span);
        }
        walk::walk_for_of_statement(self, statement);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &call.callee
            && callee.name == "eval"
            && callee
                .reference_id
                .get()
                .is_some_and(|id| self.scoping.get_reference(id).symbol_id().is_none())
        {
            match self.format {
                Format::Esm => self.refuse(
                    Code::Eval,
                    call.span,
                    "direct eval can read any binding by its written name, and the bundle renames bindings",
                ),
                _ => self.direct_eval = true,
            }
        }
        // import.meta.resolve(...)
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::ImportMeta(meta) = &member.object
            && member.property.name == "resolve"
        {
            self.skip_meta.insert(meta.span.start);
            let relative = matches!(call.arguments.first(), Some(Argument::StringLiteral(s)) if s.value.starts_with("./") || s.value.starts_with("../"));
            if !relative || call.arguments.len() != 1 {
                self.refuse(
                    Code::ImportMeta,
                    call.span,
                    "import.meta.resolve is only bundled with one relative string literal",
                );
            }
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        if let Expression::ImportMeta(meta) = &member.object
            && IMPORT_META_PROPERTIES.contains(&member.property.name.as_str())
        {
            self.skip_meta.insert(meta.span.start);
        }
        walk::walk_static_member_expression(self, member);
    }

    fn visit_import_meta(&mut self, meta: &ImportMeta) {
        if !self.skip_meta.contains(&meta.span.start) {
            self.refuse(
                Code::ImportMeta,
                meta.span,
                "import.meta is only bundled when one of its properties is read: url, filename, dirname, main, or resolve('./relative')",
            );
        }
    }

    fn visit_import_expression(&mut self, expression: &ImportExpression<'a>) {
        let literal = match &expression.source {
            Expression::StringLiteral(literal) => Some(literal.value.to_string()),
            Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
                .quasis
                .first()
                .and_then(|quasi| quasi.value.cooked.as_ref())
                .map(|cooked| cooked.to_string()),
            _ => None,
        };
        if self.format == Format::Cjs && !self.strict {
            self.refuse(
                Code::Dynamic,
                expression.span,
                "import() in a sloppy-mode CommonJS module, which the bundle compiles at run time without a module loader",
            );
        } else if expression.options.is_some() {
            self.refuse(
                Code::Dynamic,
                expression.span,
                "import() with options: the bundler does not read import attributes at run time",
            );
        } else {
            match literal {
                None => self.runtime_imports = true,
                Some(specifier) => self.dynamic_imports.push((specifier, expression.span)),
            }
        }
        walk::walk_import_expression(self, expression);
    }

    fn visit_statement(&mut self, statement: &Statement<'a>) {
        walk::walk_statement(self, statement);
    }

    fn visit_member_expression(&mut self, member: &oxc_ast::ast::MemberExpression<'a>) {
        if self.format == Format::Cjs
            && let Some(object) = member.object().get_identifier_reference()
            && object
                .reference_id
                .get()
                .is_some_and(|id| self.scoping.get_reference(id).symbol_id().is_none())
            && let Some(property) = member.static_property_name()
        {
            let refused = matches!(
                (object.name.as_str(), property),
                ("require", "cache" | "extensions") | ("module", "constructor")
            );
            if refused {
                self.refuse(
                    Code::CjsApi,
                    member.span(),
                    format!(
                        "{}.{property} exposes Node's module system, which the bundle replaces",
                        object.name
                    ),
                );
            }
        }
        walk::walk_member_expression(self, member);
    }
}
