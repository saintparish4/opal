//! What babel-jest does to a file before Jest runs it, done with oxc.
//!
//! Three steps, each over the previous one's output:
//!
//! 1. oxc_transformer removes TypeScript (keeping what `emitDecoratorMetadata`
//!    needs), lowers legacy decorators and JSX, and oxc_codegen prints the
//!    result with a source map. Nothing is downleveled: Node runs what's left.
//! 2. ES modules become CommonJS the way @babel/plugin-transform-modules-
//!    commonjs writes them, because Jest's users depend on its interop:
//!    imports become `require`s hoisted to the top, an imported binding is
//!    read through its module object at every use (`(0, _m.f)()` for a call,
//!    so `this` stays undefined), and an export is a writable property of
//!    `exports`, which is what lets `jest.spyOn(module, 'f')` replace it.
//! 3. `jest.mock`, `unmock`, `enableAutomock`, `disableAutomock` and
//!    `deepUnmock` calls at the top of the file move above everything else,
//!    as babel-plugin-jest-hoist moves them, so a mock is registered before
//!    the require it governs runs. Vitest's `vi.mock`, `vi.unmock` and
//!    `vi.hoisted` move the same way, as Vitest hoists them.
//!
//! `import.meta` becomes the module's own `import.meta` object, which the
//! runtime's loader makes: `url`, `filename`, `dirname` and `resolve`, and in
//! Vitest's dialect `env`.
//!
//! Steps 2 and 3 edit the text of step 1's output rather than its tree; every
//! source map token is carried through those edits, so the runtime can map a
//! stack frame back to the line the user wrote.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::AstKind;
use oxc_ast::ast::{
    BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression,
    ImportDeclarationSpecifier, ModuleExportName, Statement,
};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType};
use oxc_transformer::{
    DecoratorOptions, HelperLoaderMode, JsxOptions, JsxRuntime, TransformOptions, Transformer,
};

/// The options a file is transformed under, from the project's tsconfig.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Options {
    pub legacy_decorators: bool,
    pub emit_decorator_metadata: bool,
    /// TypeScript's `useDefineForClassFields`; when false, a class field
    /// without an initializer is removed, as tsc removes it.
    pub use_define_for_class_fields: bool,
    /// `react` (classic) or `react-jsx` (automatic), with the import source.
    pub jsx_classic: bool,
    pub jsx_import_source: Option<String>,
}

pub struct Output {
    pub code: String,
    /// `[generated line, generated column, source line, source column]`,
    /// zero-based, sorted by generated position.
    pub mappings: Vec<[u32; 4]>,
}

pub fn transform(path: &Path, source: &str, options: &Options) -> Result<Output, String> {
    let (code1, tokens) = lower(path, source, options)?;
    let rewritten = to_commonjs(&code1)?;
    let lines1 = LineIndex::new(&code1);
    let lines2 = LineIndex::new(&rewritten.code);
    let mut mappings: Vec<[u32; 4]> = tokens
        .into_iter()
        .filter_map(|[dst_line, dst_col, src_line, src_col]| {
            let offset = lines1.offset(dst_line, dst_col)?;
            let moved = rewritten.translate(offset)?;
            let (line, col) = lines2.position(moved);
            Some([line, col, src_line, src_col])
        })
        .collect();
    mappings.sort_unstable();
    mappings.dedup_by(|a, b| a[0] == b[0] && a[1] == b[1]);
    Ok(Output {
        code: rewritten.code,
        mappings,
    })
}

// --- step 1 ------------------------------------------------------------------

fn source_type(path: &Path) -> SourceType {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("js");
    let base = match extension {
        "ts" | "mts" => SourceType::ts(),
        "cts" => SourceType::ts().with_commonjs(true),
        "tsx" => SourceType::tsx(),
        "cjs" => SourceType::cjs().with_jsx(true),
        _ => SourceType::mjs().with_jsx(true),
    };
    // A file with neither import nor export is a script to Node; parsing
    // every file as a module would reject `with`, octal escapes and the like
    // in old CommonJS. `unambiguous` decides from the syntax, as Babel's
    // default sourceType does.
    if matches!(extension, "ts" | "tsx" | "mts" | "js" | "jsx" | "mjs") {
        base.with_unambiguous(true)
    } else {
        base
    }
}

fn render(path: &Path, source: &str, message: &str, offset: Option<usize>) -> String {
    let detail = offset
        .map(|offset| {
            let offset = offset.min(source.len());
            let before = &source[..offset];
            let line = before.matches('\n').count() + 1;
            let column = offset - before.rfind('\n').map_or(0, |i| i + 1) + 1;
            format!(" ({line}:{column})")
        })
        .unwrap_or_default();
    format!("{}: {message}{detail}", path.display())
}

macro_rules! first_error {
    ($diagnostics:expr, $path:expr, $source:expr) => {
        if let Some(error) = $diagnostics.errors().next() {
            let offset = error.labels.first().map(|label| label.offset() as usize);
            return Err(render($path, $source, &error.message, offset));
        }
    };
}

type Tokens = Vec<[u32; 4]>;

fn lower(path: &Path, source: &str, options: &Options) -> Result<(String, Tokens), String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type(path)).parse();
    first_error!(parsed.diagnostics, path, source);
    let mut program = parsed.program;
    let semantic = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .with_enum_eval(true)
        .build(&program);
    let scoping = semantic.semantic.into_scoping();

    let mut transform_options = TransformOptions::default();
    // Helpers are read from a module-local `babelHelpers` the CommonJS step
    // defines, rather than from @oxc-project/runtime, which the project
    // doesn't have.
    transform_options.helper_loader.mode = HelperLoaderMode::External;
    transform_options.decorator = DecoratorOptions {
        legacy: options.legacy_decorators,
        emit_decorator_metadata: options.emit_decorator_metadata,
        ..DecoratorOptions::default()
    };
    if !options.use_define_for_class_fields {
        transform_options
            .typescript
            .remove_class_fields_without_initializer = true;
        transform_options.assumptions.set_public_class_fields = true;
    }
    transform_options.jsx = JsxOptions {
        runtime: if options.jsx_classic {
            JsxRuntime::Classic
        } else {
            JsxRuntime::Automatic
        },
        import_source: options.jsx_import_source.clone(),
        ..JsxOptions::default()
    };
    let transformed = Transformer::new(&allocator, path, &transform_options)
        .build_with_scoping(scoping, &mut program);
    first_error!(transformed.diagnostics, path, source);
    let printed = Codegen::new()
        .with_options(CodegenOptions {
            source_map_path: Some(path.to_path_buf()),
            ..CodegenOptions::default()
        })
        .build(&program);
    let tokens = printed
        .map
        .map(|map| {
            map.get_tokens()
                .map(|t| {
                    [
                        t.get_dst_line(),
                        t.get_dst_col(),
                        t.get_src_line(),
                        t.get_src_col(),
                    ]
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((printed.code, tokens))
}

// --- steps 2 and 3 -------------------------------------------------------------

const JEST_HOISTED: &[&str] = &[
    "mock",
    "unmock",
    "enableAutomock",
    "disableAutomock",
    "deepUnmock",
];
const VI_HOISTED: &[&str] = &["mock", "unmock", "hoisted"];

const IMPORT_META: &str = "require[Symbol.for(\"opal.importMeta\")]";

// The only helpers oxc emits here, since nothing is downleveled: legacy
// decorators, as tslib's __decorate, __param and __metadata. They read
// `Reflect` when called, so reflect-metadata, loaded by the test, applies.
const DECORATOR_HELPERS: &str = "var babelHelpers = { decorate: function (decorators, target, key, desc) { var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d; if (typeof Reflect === \"object\" && typeof Reflect.decorate === \"function\") r = Reflect.decorate(decorators, target, key, desc); else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r; return c > 3 && r && Object.defineProperty(target, key, r), r; }, decorateParam: function (index, decorator) { return function (target, key) { decorator(target, key, index); }; }, decorateMetadata: function (k, v) { if (typeof Reflect === \"object\" && typeof Reflect.metadata === \"function\") return Reflect.metadata(k, v); } };";

const INTEROP_DEFAULT: &str =
    "function _interopRequireDefault(e) { return e && e.__esModule ? e : { default: e }; }";
// @babel/helpers 7.29's interopRequireWildcard, as Babel inlines it.
const INTEROP_WILDCARD: &str = "function _interopRequireWildcard(e, t) { if (!t && e && e.__esModule) return e; var n = { __proto__: null, default: e }; if (Object(e) !== e) return n; var c = _interopRequireWildcard.c || (_interopRequireWildcard.c = new WeakMap()); if (c.has(e)) return c.get(e); c.set(e, n); for (var k in e) if (k !== \"default\" && {}.hasOwnProperty.call(e, k)) { var d = Object.getOwnPropertyDescriptor(e, k); d && (d.get || d.set) ? Object.defineProperty(n, k, d) : n[k] = e[k]; } return n; }";

struct Edit {
    start: usize,
    end: usize,
    text: String,
}

/// Where each stretch of the input landed in the output.
struct Piece {
    from: usize,
    to: usize,
    at: usize,
}

struct Rewritten {
    code: String,
    pieces: Vec<Piece>,
}

impl Rewritten {
    fn translate(&self, offset: usize) -> Option<usize> {
        self.pieces
            .iter()
            .find(|p| offset >= p.from && offset < p.to.max(p.from + 1))
            .map(|p| p.at + (offset - p.from).min(p.to.saturating_sub(p.from)))
    }
}

/// Copies `code[range]` into `out`, applying the edits inside it.
fn emit(
    code: &str,
    start: usize,
    end: usize,
    edits: &[Edit],
    out: &mut String,
    pieces: &mut Vec<Piece>,
) {
    let mut cursor = start;
    for edit in edits.iter().filter(|e| e.start >= start && e.end <= end) {
        if edit.start < cursor {
            continue;
        }
        pieces.push(Piece {
            from: cursor,
            to: edit.start,
            at: out.len(),
        });
        out.push_str(&code[cursor..edit.start]);
        if edit.end > edit.start {
            pieces.push(Piece {
                from: edit.start,
                to: edit.end,
                at: out.len(),
            });
        }
        out.push_str(&edit.text);
        cursor = edit.end;
    }
    pieces.push(Piece {
        from: cursor,
        to: end,
        at: out.len(),
    });
    out.push_str(&code[cursor..end]);
}

fn identifier_for(source: &str, taken: &mut Vec<String>) -> String {
    let stem: String = source
        .rsplit('/')
        .find(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
        .unwrap_or("module")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let base = if stem.chars().next().is_some_and(|c| c.is_ascii_digit()) || stem.is_empty() {
        format!("_m{stem}")
    } else {
        format!("_{stem}")
    };
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{base}{n}");
        n += 1;
    }
    taken.push(name.clone());
    name
}

fn export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::IdentifierName(id) => id.name.to_string(),
        ModuleExportName::IdentifierReference(id) => id.name.to_string(),
        ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

fn member(object: &str, property: &str) -> String {
    if property
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !property.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        format!("{object}.{property}")
    } else {
        format!("{object}[{property:?}]")
    }
}

/// Whether `expression` is `<root>.<method>(...)`, possibly chained, with a
/// method from `methods`.
fn is_hoisted_call(expression: &Expression, root: &str, methods: &[&str]) -> bool {
    let Expression::CallExpression(call) = expression else {
        return false;
    };
    let Some(member) = call.callee.as_member_expression() else {
        return false;
    };
    let Some(method) = member.static_property_name() else {
        return false;
    };
    if !methods.contains(&method) {
        return false;
    }
    match member.object() {
        Expression::Identifier(id) => id.name == root,
        inner => is_hoisted_call(inner, root, methods),
    }
}

/// A top-level statement that moves above the requires.
fn is_hoisted_statement(statement: &Statement, root: &str, methods: &[&str]) -> bool {
    match statement {
        Statement::ExpressionStatement(expression) => {
            is_hoisted_call(&expression.expression, root, methods)
        }
        // `const x = vi.hoisted(() => ...)`
        Statement::VariableDeclaration(declaration) => {
            methods.contains(&"hoisted")
                && declaration.declarations.len() == 1
                && declaration.declarations[0]
                    .init
                    .as_ref()
                    .is_some_and(|init| is_hoisted_call(init, root, &["hoisted"]))
        }
        _ => false,
    }
}

fn to_commonjs(code: &str) -> Result<Rewritten, String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, SourceType::mjs().with_unambiguous(true)).parse();
    if let Some(error) = parsed.diagnostics.errors().next() {
        return Err(format!(
            "opal test's CommonJS rewrite could not reparse its own output: {}",
            error.message
        ));
    }
    let program = &parsed.program;
    let semantic = SemanticBuilder::new()
        .with_build_nodes(true)
        .build(program)
        .semantic;
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();

    let mut edits: Vec<Edit> = Vec::new();
    let mut requires: Vec<String> = Vec::new();
    let mut header: Vec<String> = Vec::new();
    let mut void_exports: Vec<String> = Vec::new();
    let mut hoisted: Vec<(usize, usize)> = Vec::new();
    // Names the rewrite introduces (`_m`, `_before`) avoid every name the
    // file declares or reads, as Babel's generateUid does: a test with its
    // own `_before` would otherwise shadow the module `./before` became.
    let mut taken: Vec<String> = scoping
        .symbol_names()
        .map(str::to_string)
        .chain(
            scoping
                .root_unresolved_references()
                .keys()
                .map(|name| name.to_string()),
        )
        .collect();
    let mut uses_default = false;
    let mut uses_export_names = false;
    let mut uses_wildcard = false;
    let mut has_exports = false;
    let mut is_module = false;
    // Imported bindings: symbol -> how it is read.
    let mut imported: Vec<(oxc_semantic::SymbolId, String)> = Vec::new();
    // Exported local bindings: symbol -> the names it is exported as, so a
    // later assignment to it updates `exports` too, as Babel's does.
    let mut exported_locals: Vec<(oxc_semantic::SymbolId, String)> = Vec::new();
    // `jest` from @jest/globals and `vi` from vitest, when imported: the
    // local name, and how it reads once the import is a require.
    let mut hoist_roots: Vec<(String, String, &'static [&'static str])> = Vec::new();

    let statement_end = |end: usize| -> usize {
        // Past a trailing newline, so an inserted line lands after it.
        if code[end..].starts_with('\n') {
            end + 1
        } else {
            end
        }
    };

    for statement in &program.body {
        match statement {
            Statement::ImportDeclaration(decl) => {
                is_module = true;
                edits.push(Edit {
                    start: decl.span.start as usize,
                    end: statement_end(decl.span.end as usize),
                    text: String::new(),
                });
                if decl.import_kind.is_type() {
                    continue;
                }
                let source = decl.source.value.as_str();
                let quoted = &code[decl.source.span.start as usize..decl.source.span.end as usize];
                let Some(specifiers) = decl.specifiers.as_ref().filter(|s| !s.is_empty()) else {
                    requires.push(format!("require({quoted});"));
                    continue;
                };
                let has_default = specifiers
                    .iter()
                    .any(|s| matches!(s, ImportDeclarationSpecifier::ImportDefaultSpecifier(_)));
                let has_named = specifiers
                    .iter()
                    .any(|s| matches!(s, ImportDeclarationSpecifier::ImportSpecifier(_)));
                for specifier in specifiers {
                    if let ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns) = specifier {
                        uses_wildcard = true;
                        requires.push(format!(
                            "var {} = _interopRequireWildcard(require({quoted}));",
                            ns.local.name
                        ));
                    }
                }
                if !has_default && !has_named {
                    continue;
                }
                let name = identifier_for(source, &mut taken);
                let required = if has_default && has_named {
                    uses_wildcard = true;
                    format!("_interopRequireWildcard(require({quoted}))")
                } else if has_default {
                    uses_default = true;
                    format!("_interopRequireDefault(require({quoted}))")
                } else {
                    format!("require({quoted})")
                };
                let mut deferred = Vec::new();
                for specifier in specifiers {
                    let (local, access) = match specifier {
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(d) => {
                            (&d.local, format!("{name}.default"))
                        }
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            (&s.local, member(&name, &export_name(&s.imported)))
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => continue,
                    };
                    if source == "@jest/globals" && access.ends_with(".jest") {
                        hoist_roots.push((local.name.to_string(), access.clone(), JEST_HOISTED));
                    }
                    if source == "vitest"
                        && (access.ends_with(".vi") || access.ends_with(".vitest"))
                    {
                        hoist_roots.push((local.name.to_string(), access.clone(), VI_HOISTED));
                    }
                    if let Some(symbol) = local.symbol_id.get() {
                        deferred.push((symbol, access));
                    }
                }
                imported.extend(deferred);
                requires.push(format!("var {name} = {required};"));
            }
            Statement::ExportFromDeclaration(decl) => {
                is_module = true;
                edits.push(Edit {
                    start: decl.span.start as usize,
                    end: statement_end(decl.span.end as usize),
                    text: String::new(),
                });
                if decl.export_kind.is_type() {
                    continue;
                }
                has_exports = true;
                let source = &decl.source;
                let quoted = &code[source.span.start as usize..source.span.end as usize];
                let name = identifier_for(&source.value, &mut taken);
                requires.push(format!("var {name} = require({quoted});"));
                // Right after the require, as Babel places them: an `export *`
                // further down may read these, and the module has to be there.
                for specifier in &decl.specifiers {
                    requires.push(format!(
                        "Object.defineProperty(exports, {:?}, {{ enumerable: true, get: function () {{ return {}; }} }});",
                        export_name(&specifier.exported),
                        member(&name, &export_name(&specifier.local))
                    ));
                }
            }
            Statement::ExportDeclaration(decl) => {
                is_module = true;
                has_exports = true;
                {
                    let declaration = &decl.declaration;
                    edits.push(Edit {
                        start: decl.span.start as usize,
                        end: declaration.span().start as usize,
                        text: String::new(),
                    });
                    let names: Vec<String> = match declaration {
                        Declaration::VariableDeclaration(v) => v
                            .declarations
                            .iter()
                            .flat_map(|d| d.id.get_binding_identifiers())
                            .map(|id| {
                                if let Some(symbol) = id.symbol_id.get() {
                                    exported_locals.push((symbol, id.name.to_string()));
                                }
                                id.name.to_string()
                            })
                            .collect(),
                        Declaration::FunctionDeclaration(f) => {
                            if let Some(id) = &f.id {
                                header.push(format!("exports.{0} = {0};", id.name));
                                if let Some(symbol) = id.symbol_id.get() {
                                    exported_locals.push((symbol, id.name.to_string()));
                                }
                            }
                            continue;
                        }
                        Declaration::ClassDeclaration(c) => {
                            c.id.iter()
                                .map(|id| {
                                    if let Some(symbol) = id.symbol_id.get() {
                                        exported_locals.push((symbol, id.name.to_string()));
                                    }
                                    id.name.to_string()
                                })
                                .collect()
                        }
                        _ => continue,
                    };
                    let after: String = names
                        .iter()
                        .map(|n| format!("\nexports.{n} = {n};"))
                        .collect();
                    void_exports.extend(names);
                    edits.push(Edit {
                        start: decl.span.end as usize,
                        end: decl.span.end as usize,
                        text: after,
                    });
                }
            }
            Statement::ExportNamedDeclaration(decl) => {
                is_module = true;
                if decl.export_kind.is_type() {
                    edits.push(Edit {
                        start: decl.span.start as usize,
                        end: statement_end(decl.span.end as usize),
                        text: String::new(),
                    });
                    continue;
                }
                has_exports = true;
                {
                    let mut assignments = String::new();
                    for specifier in &decl.specifiers {
                        let exported = export_name(&specifier.exported);
                        let local = export_name(&specifier.local);
                        let symbol = match &specifier.local {
                            ModuleExportName::IdentifierReference(id) => id
                                .reference_id
                                .get()
                                .and_then(|r| scoping.get_reference(r).symbol_id()),
                            _ => None,
                        };
                        if let Some((_, access)) =
                            symbol.and_then(|s| imported.iter().find(|(i, _)| *i == s))
                        {
                            requires.push(format!(
                                "Object.defineProperty(exports, {exported:?}, {{ enumerable: true, get: function () {{ return {access}; }} }});"
                            ));
                        } else {
                            if let Some(symbol) = symbol {
                                exported_locals.push((symbol, exported.clone()));
                            }
                            void_exports.push(exported.clone());
                            assignments.push_str(&format!(
                                "{} = {local};\n",
                                member("exports", &exported)
                            ));
                        }
                    }
                    edits.push(Edit {
                        start: decl.span.start as usize,
                        end: statement_end(decl.span.end as usize),
                        text: assignments,
                    });
                }
            }
            Statement::ExportDefaultDeclaration(decl) => {
                is_module = true;
                has_exports = true;
                let start = decl.span.start as usize;
                match &decl.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                        let inner = f.span.start as usize;
                        match &f.id {
                            Some(id) => {
                                header.push(format!("exports.default = {};", id.name));
                                edits.push(Edit {
                                    start,
                                    end: inner,
                                    text: String::new(),
                                });
                            }
                            None => {
                                header.push("exports.default = _default;".to_string());
                                // `function () {}` -> `function _default() {}`
                                let text = &code[inner..f.span.end as usize];
                                let keyword_end = text.find('(').unwrap_or(0);
                                let head = &text[..keyword_end];
                                let named = format!("{} _default", head.trim_end());
                                edits.push(Edit {
                                    start,
                                    end: inner + keyword_end,
                                    text: named,
                                });
                            }
                        }
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                        let inner = c.span.start as usize;
                        let name =
                            c.id.as_ref()
                                .map_or_else(|| "_default".to_string(), |id| id.name.to_string());
                        if c.id.is_some() {
                            edits.push(Edit {
                                start,
                                end: inner,
                                text: String::new(),
                            });
                        } else {
                            edits.push(Edit {
                                start,
                                end: inner + "class".len(),
                                text: "class _default".to_string(),
                            });
                        }
                        void_exports.push("default".to_string());
                        edits.push(Edit {
                            start: decl.span.end as usize,
                            end: decl.span.end as usize,
                            text: format!("\nexports.default = {name};"),
                        });
                    }
                    other => {
                        if other.as_expression().is_some() {
                            void_exports.push("default".to_string());
                            let expression_start = other.span().start as usize;
                            edits.push(Edit {
                                start,
                                end: expression_start,
                                text: "var _default = exports.default = ".to_string(),
                            });
                        } else {
                            edits.push(Edit {
                                start,
                                end: decl.span.end as usize,
                                text: String::new(),
                            });
                        }
                    }
                }
            }
            Statement::ExportAllDeclaration(decl) => {
                is_module = true;
                has_exports = true;
                let quoted = &code[decl.source.span.start as usize..decl.source.span.end as usize];
                let name = identifier_for(&decl.source.value, &mut taken);
                match &decl.exported {
                    Some(exported) => {
                        uses_wildcard = true;
                        requires.push(format!(
                            "var {name} = _interopRequireWildcard(require({quoted}));"
                        ));
                        requires.push(format!(
                            "Object.defineProperty(exports, {:?}, {{ enumerable: true, get: function () {{ return {name}; }} }});",
                            export_name(exported)
                        ));
                    }
                    None => {
                        // A name this module exports itself wins over the
                        // same name from `export *`, as in ES modules;
                        // Babel keeps those names in `_exportNames`.
                        requires.push(format!(
                            "var {name} = require({quoted});\nObject.keys({name}).forEach(function (key) {{ if (key === \"default\" || key === \"__esModule\") return; if (Object.prototype.hasOwnProperty.call(_exportNames, key)) return; if (key in exports && exports[key] === {name}[key]) return; Object.defineProperty(exports, key, {{ enumerable: true, get: function () {{ return {name}[key]; }} }}); }});"
                        ));
                        uses_export_names = true;
                    }
                }
                edits.push(Edit {
                    start: decl.span.start as usize,
                    end: statement_end(decl.span.end as usize),
                    text: String::new(),
                });
            }
            _ => {}
        }
    }

    // Every read of an imported binding goes through its module object.
    let mut removed: Vec<(usize, usize)> = edits
        .iter()
        .filter(|e| e.text.is_empty() && e.end > e.start)
        .map(|e| (e.start, e.end))
        .collect();
    for (symbol, access) in &imported {
        for reference in scoping.get_resolved_references(*symbol) {
            let node = nodes.get_node(reference.node_id());
            let span = node.kind().span();
            let (start, end) = (span.start as usize, span.end as usize);
            if removed.iter().any(|(s, e)| start >= *s && end <= *e) {
                continue;
            }
            let parent = nodes.parent_kind(node.id());
            let text = match parent {
                AstKind::CallExpression(call) if call.callee.span() == span => {
                    format!("(0, {access})")
                }
                AstKind::TaggedTemplateExpression(tagged) if tagged.tag.span() == span => {
                    format!("(0, {access})")
                }
                AstKind::ObjectProperty(property) if property.shorthand => {
                    format!("{}: {access}", &code[start..end])
                }
                _ => access.clone(),
            };
            edits.push(Edit { start, end, text });
        }
    }

    // A module's own top-level `exports`, `module`, `require`, `__filename`
    // or `__dirname`, as in `const __dirname = dirname(fileURLToPath(
    // import.meta.url))`, would collide with the CommonJS wrapper's
    // parameters. Babel's CommonJS transform renames it, to `_require` and so
    // on, and so does this, everywhere it is used. An imported or exported one
    // keeps its name, since the rewrites above and below write it by name.
    if is_module {
        let names_in_use: Vec<&str> = scoping.symbol_names().collect();
        for wrapper_name in ["exports", "module", "require", "__filename", "__dirname"] {
            let Some(symbol) = scoping.get_root_binding(wrapper_name.into()) else {
                continue;
            };
            if imported.iter().any(|(s, _)| *s == symbol)
                || exported_locals.iter().any(|(s, _)| *s == symbol)
            {
                continue;
            }
            // Babel's generateUid: leading underscores dropped, then `_`, and
            // a number from 2 on until the name is free.
            let stem = wrapper_name.trim_start_matches('_');
            let mut renamed = format!("_{stem}");
            let mut n = 2;
            while names_in_use.contains(&renamed.as_str()) || taken.contains(&renamed) {
                renamed = format!("_{stem}{n}");
                n += 1;
            }
            taken.push(renamed.clone());
            let declared = scoping.symbol_span(symbol);
            edits.push(Edit {
                start: declared.start as usize,
                end: declared.end as usize,
                text: renamed.clone(),
            });
            for reference in scoping.get_resolved_references(symbol) {
                let node = nodes.get_node(reference.node_id());
                let span = node.kind().span();
                let (start, end) = (span.start as usize, span.end as usize);
                if removed.iter().any(|(s, e)| start >= *s && end <= *e) {
                    continue;
                }
                let text = match nodes.parent_kind(node.id()) {
                    AstKind::ObjectProperty(property) if property.shorthand => {
                        format!("{wrapper_name}: {renamed}")
                    }
                    _ => renamed.clone(),
                };
                edits.push(Edit { start, end, text });
            }
        }
    }

    // An assignment to an exported binding updates `exports` with it.
    for (symbol, _) in &exported_locals {
        let names: Vec<&String> = exported_locals
            .iter()
            .filter(|(s, _)| s == symbol)
            .map(|(_, n)| n)
            .collect();
        if names.iter().any(|n| {
            exported_locals
                .iter()
                .filter(|(s, m)| s == symbol && m == *n)
                .count()
                > 1
        }) {
            continue;
        }
        let targets: String = names
            .iter()
            .map(|n| format!("{} = ", member("exports", n)))
            .collect();
        let local = scoping.symbol_name(*symbol);
        for reference in scoping.get_resolved_references(*symbol) {
            if !reference.is_write() {
                continue;
            }
            let node = nodes.get_node(reference.node_id());
            let mut parent = nodes.parent_node(node.id());
            // An identifier target may sit in a wrapper node first.
            if !matches!(
                parent.kind(),
                AstKind::AssignmentExpression(_) | AstKind::UpdateExpression(_)
            ) {
                parent = nodes.parent_node(parent.id());
            }
            let span = parent.kind().span();
            let (start, end) = (span.start as usize, span.end as usize);
            if removed.iter().any(|(s, e)| start >= *s && end <= *e) {
                continue;
            }
            match parent.kind() {
                AstKind::AssignmentExpression(_) => {
                    edits.push(Edit {
                        start,
                        end: start,
                        text: format!("({targets}"),
                    });
                    edits.push(Edit {
                        start: end,
                        end,
                        text: ")".to_string(),
                    });
                }
                AstKind::UpdateExpression(update) if update.prefix => {
                    edits.push(Edit {
                        start,
                        end: start,
                        text: format!("({targets}"),
                    });
                    edits.push(Edit {
                        start: end,
                        end,
                        text: ")".to_string(),
                    });
                }
                AstKind::UpdateExpression(update) => {
                    let back = if update.operator.as_str() == "++" {
                        "- 1"
                    } else {
                        "+ 1"
                    };
                    edits.push(Edit {
                        start,
                        end: start,
                        text: "(".to_string(),
                    });
                    edits.push(Edit {
                        start: end,
                        end,
                        text: format!(", {targets}{local}, {local} {back})"),
                    });
                }
                _ => {}
            }
        }
    }
    exported_locals.dedup_by_key(|(s, _)| *s);

    // Dynamic import() and import.meta, anywhere.
    for node in nodes.iter() {
        if let AstKind::ImportMeta(meta) = node.kind() {
            is_module = true;
            edits.push(Edit {
                start: meta.span.start as usize,
                end: meta.span.end as usize,
                text: IMPORT_META.to_string(),
            });
        }
        if let AstKind::ImportExpression(import) = node.kind() {
            is_module = true;
            uses_wildcard = true;
            let specifier = import.source.span();
            let source_text = &code[specifier.start as usize..specifier.end as usize];
            edits.push(Edit {
                start: import.span.start as usize,
                end: import.span.end as usize,
                text: format!(
                    "Promise.resolve({source_text}).then(function (s) {{ return _interopRequireWildcard(require(s)); }})"
                ),
            });
        }
    }

    // jest.mock and friends, from the top level, go first. A global `jest`
    // or `vi` counts unless the file declares its own.
    let declares = |name: &str| {
        program.body.iter().any(|statement| match statement {
            Statement::VariableDeclaration(v) => v
                .declarations
                .iter()
                .any(|d| matches!(&d.id, BindingPattern::BindingIdentifier(id) if id.name == name)),
            _ => false,
        })
    };
    let mut roots: Vec<(String, &[&str])> = hoist_roots
        .iter()
        .map(|(local, _, methods)| (local.clone(), *methods))
        .collect();
    for (global, methods) in [("jest", JEST_HOISTED), ("vi", VI_HOISTED)] {
        if !hoist_roots.iter().any(|(_, _, m)| *m == methods) && !declares(global) {
            roots.push((global.to_string(), methods));
        }
    }
    for statement in &program.body {
        if roots
            .iter()
            .any(|(root, methods)| is_hoisted_statement(statement, root, methods))
        {
            let span = statement.span();
            hoisted.push((span.start as usize, statement_end(span.end as usize)));
        }
    }
    for &(start, end) in &hoisted {
        edits.push(Edit {
            start,
            end,
            text: String::new(),
        });
        removed.push((start, end));
    }

    let uses_helpers = code.contains("babelHelpers.");
    if !is_module && hoisted.is_empty() && !uses_helpers {
        return Ok(Rewritten {
            code: code.to_string(),
            pieces: vec![Piece {
                from: 0,
                to: code.len(),
                at: 0,
            }],
        });
    }

    edits.sort_by_key(|e| (e.start, e.end));
    let mut out = String::new();
    let mut pieces = Vec::new();
    if is_module {
        out.push_str("\"use strict\";\n");
        if has_exports {
            out.push_str("Object.defineProperty(exports, \"__esModule\", { value: true });\n");
        }
        let mut seen = Vec::new();
        let voids: Vec<&String> = void_exports
            .iter()
            .filter(|n| {
                let fresh = !seen.contains(n);
                seen.push(*n);
                fresh
            })
            .collect();
        if !voids.is_empty() {
            let chain: Vec<String> = voids.iter().map(|n| member("exports", n)).collect();
            out.push_str(&format!("{} = void 0;\n", chain.join(" = ")));
        }
        for line in &header {
            out.push_str(line);
            out.push('\n');
        }
        if uses_export_names {
            let names: Vec<String> = explicit_export_names(program)
                .iter()
                .map(|n| format!("{n:?}: true"))
                .collect();
            out.push_str(&format!("var _exportNames = {{ {} }};\n", names.join(", ")));
        }
    }
    if !hoisted.is_empty() {
        // The hoisted calls name `jest` or `vi` before the import that binds
        // it has run; require that module first, as jest-hoist does.
        for (_, access, _) in &hoist_roots {
            let module = access.rsplit_once('.').map_or(access.as_str(), |(m, _)| m);
            if let Some(index) = requires
                .iter()
                .position(|line| line.starts_with(&format!("var {module} = ")))
            {
                let line = requires.remove(index);
                out.push_str(&line);
                out.push('\n');
            }
        }
    }
    for &(start, end) in &hoisted {
        // The edit that removes the statement from the body doesn't apply to
        // its copy up here.
        let inside: Vec<Edit> = edits
            .iter()
            .filter(|e| !(e.start == start && e.end == end))
            .map(|e| Edit {
                start: e.start,
                end: e.end,
                text: e.text.clone(),
            })
            .collect();
        emit(code, start, end, &inside, &mut out, &mut pieces);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    for line in &requires {
        out.push_str(line);
        out.push('\n');
    }
    if uses_helpers {
        out.push_str(DECORATOR_HELPERS);
        out.push('\n');
    }
    if uses_default {
        out.push_str(INTEROP_DEFAULT);
        out.push('\n');
    }
    if uses_wildcard {
        out.push_str(INTEROP_WILDCARD);
        out.push('\n');
    }
    let body_edits: Vec<Edit> = edits
        .into_iter()
        .filter(|e| {
            !hoisted
                .iter()
                .any(|&(s, en)| e.start >= s && e.end <= en && !(e.start == s && e.end == en))
        })
        .collect();
    emit(code, 0, code.len(), &body_edits, &mut out, &mut pieces);
    Ok(Rewritten { code: out, pieces })
}

/// Every name the module exports by name: what `export *` must not override.
fn explicit_export_names(program: &oxc_ast::ast::Program) -> Vec<String> {
    let mut names = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::ExportDeclaration(decl) => match &decl.declaration {
                Declaration::VariableDeclaration(v) => names.extend(
                    v.declarations
                        .iter()
                        .flat_map(|d| d.id.get_binding_identifiers())
                        .map(|id| id.name.to_string()),
                ),
                Declaration::FunctionDeclaration(f) => {
                    names.extend(f.id.iter().map(|id| id.name.to_string()))
                }
                Declaration::ClassDeclaration(c) => {
                    names.extend(c.id.iter().map(|id| id.name.to_string()))
                }
                _ => {}
            },
            Statement::ExportNamedDeclaration(decl) => {
                names.extend(decl.specifiers.iter().map(|s| export_name(&s.exported)));
            }
            Statement::ExportFromDeclaration(decl) => {
                names.extend(decl.specifiers.iter().map(|s| export_name(&s.exported)));
            }
            Statement::ExportAllDeclaration(decl) => {
                if let Some(exported) = &decl.exported {
                    names.push(export_name(exported));
                }
            }
            Statement::ExportDefaultDeclaration(_) => names.push("default".to_string()),
            _ => {}
        }
    }
    names
}

// --- positions ------------------------------------------------------------------

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    fn offset(&self, line: u32, col: u32) -> Option<usize> {
        self.starts
            .get(line as usize)
            .map(|start| start + col as usize)
    }

    fn position(&self, offset: usize) -> (u32, u32) {
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        (line as u32, (offset - self.starts[line]) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, source: &str) -> String {
        transform(Path::new(name), source, &Options::default())
            .unwrap()
            .code
    }

    #[test]
    fn test_imports_become_requires_read_through_the_module() {
        let code = run(
            "a.mjs",
            "import x, {y as z} from './m';\nimport * as ns from 'n';\nz(x, ns);\n",
        );
        assert!(
            code.contains("var _m = _interopRequireWildcard(require(\"./m\"));"),
            "{code}"
        );
        assert!(
            code.contains("var ns = _interopRequireWildcard(require(\"n\"));"),
            "{code}"
        );
        assert!(code.contains("(0, _m.y)(_m.default, ns)"), "{code}");
    }

    #[test]
    fn test_exports_are_writable_properties() {
        let code = run(
            "a.mjs",
            "export const a = 1;\nexport function f() {}\nexport default a;\n",
        );
        assert!(
            code.contains("Object.defineProperty(exports, \"__esModule\", { value: true });"),
            "{code}"
        );
        assert!(code.contains("exports.f = f;"), "{code}");
        assert!(code.contains("exports.a = a;"), "{code}");
        assert!(
            code.contains("var _default = exports.default = a;"),
            "{code}"
        );
    }

    #[test]
    fn test_a_module_s_own_require_and_dirname_are_renamed() {
        let code = run(
            "a.mjs",
            "import {createRequire} from 'node:module';\nconst require = createRequire(import.meta.url);\nconst __dirname = '/x';\nexport const pkg = require('./p.json');\nexport const where = {__dirname};\n",
        );
        assert!(
            code.contains("const _require = (0, _node_module.createRequire)("),
            "{code}"
        );
        assert!(code.contains("const _dirname = \"/x\";"), "{code}");
        assert!(code.contains("= _require(\"./p.json\")"), "{code}");
        assert!(code.contains("{ __dirname: _dirname }"), "{code}");
    }

    #[test]
    fn test_module_variables_avoid_the_file_s_own_names() {
        let code = run(
            "a.test.mjs",
            "import {before} from './before';\nfunction t() { function _before() {} return before(_before); }\n",
        );
        assert!(
            code.contains("var _before2 = require(\"./before\");"),
            "{code}"
        );
        assert!(code.contains("(0, _before2.before)(_before)"), "{code}");
    }

    #[test]
    fn test_assignments_to_exports_update_them() {
        let code = run(
            "e.mjs",
            "export var E;\n(function (E) { E.A = 1; })(E || (E = {}));\nexport let n = 0;\nexport function inc() { n++; ++n; }\n",
        );
        assert!(code.contains("(exports.E = E = {})"), "{code}");
        assert!(code.contains("(n++, exports.n = n, n - 1)"), "{code}");
        assert!(code.contains("(exports.n = ++n)"), "{code}");
    }

    #[test]
    fn test_jest_mock_goes_above_the_require_it_governs() {
        let code = run(
            "a.test.js",
            "import {f} from './f';\njest.mock('./f');\ntest('t', () => f());\n",
        );
        assert_eq!(code.matches("jest.mock(").count(), 1, "{code}");
        let mock = code.find("jest.mock(").unwrap();
        let require = code.find("require(\"./f\")").unwrap();
        assert!(mock < require, "{code}");
    }

    #[test]
    fn test_typescript_is_removed_and_decorators_kept_with_metadata() {
        let options = Options {
            legacy_decorators: true,
            emit_decorator_metadata: true,
            use_define_for_class_fields: true,
            ..Options::default()
        };
        let source = "import {Injectable} from '@nestjs/common';\nimport {Dep} from './dep';\n@Injectable()\nexport class S {\n  constructor(private readonly dep: Dep) {}\n  get(): string { return 'x'; }\n}\n";
        let out = transform(Path::new("s.ts"), source, &options).unwrap();
        assert!(out.code.contains("design:paramtypes"), "{}", out.code);
        assert!(out.code.contains("this.dep = dep"), "{}", out.code);
        assert!(!out.code.contains(": string"), "{}", out.code);
    }

    #[test]
    fn test_mappings_point_back_to_the_line_written() {
        let source = "import {f} from './f';\n\ntype T = string;\n\nconst x: T = 'a';\nthrow new Error(x);\n";
        let out = transform(Path::new("a.ts"), source, &Options::default()).unwrap();
        let throw_line = out
            .code
            .lines()
            .position(|l| l.contains("throw new Error"))
            .unwrap() as u32;
        let mapped = out.mappings.iter().find(|m| m[0] == throw_line).unwrap();
        assert_eq!(mapped[2], 5, "{:?}\n{}", out.mappings, out.code);
    }
}

#[cfg(test)]
mod print {
    #[test]
    #[ignore = "prints a transform for reading"]
    fn print_transform() {
        let path = std::env::var("OPAL_TRANSFORM_FILE").unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        let options = super::Options {
            legacy_decorators: true,
            emit_decorator_metadata: true,
            use_define_for_class_fields: true,
            ..Default::default()
        };
        let out = super::transform(std::path::Path::new(&path), &source, &options).unwrap();
        println!("{}", out.code);
    }
}
