//! Reading, classifying, parsing, and compiling each module.
//!
//! The graph says which files are in the program and what each import
//! resolved to. This module reads each file, checks it is still the file the
//! graph hashed, decides whether Node would load it as ESM or CommonJS
//! (`SEMANTICS.md` rule 1), parses it, and compiles TypeScript and JSX to
//! JavaScript (rule 13). Everything after this sees JavaScript only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::{ParseOptions, Parser};
use oxc_semantic::{Scoping, SemanticBuilder};
use oxc_span::SourceType;
use oxc_transformer::{
    DecoratorOptions, JsxOptions, JsxRuntime, TransformOptions, Transformer, TypeScriptOptions,
};
use serde_json::Value;

use opal_core::graph::{ModuleGraph, ModuleId, ModuleSystem, SourceKind};
use opal_core::hash::ContentHash;

use crate::diagnostic::{Code, Diagnostic, LineIndex};
use crate::tsconfig::{Jsx, TsConfigs, TsSettings};

/// What Node does with a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Esm,
    Cjs,
    Json,
    /// Not parsed: native addons, WebAssembly, assets.
    Opaque,
}

/// A module's text, before parsing.
pub struct Source {
    /// Root-relative.
    pub path: String,
    pub absolute: PathBuf,
    pub format: Format,
    /// Without a byte-order mark.
    pub text: String,
    pub language: Language,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    JavaScript,
    Jsx,
    TypeScript,
    Tsx,
    Other,
}

impl Language {
    fn of(kind: SourceKind) -> Self {
        match kind {
            SourceKind::JavaScript => Language::JavaScript,
            SourceKind::Jsx => Language::Jsx,
            SourceKind::TypeScript => Language::TypeScript,
            SourceKind::Tsx => Language::Tsx,
            SourceKind::Json | SourceKind::Opaque => Language::Other,
        }
    }

    pub fn is_typescript(self) -> bool {
        matches!(self, Language::TypeScript | Language::Tsx)
    }

    pub fn needs_compiling(self) -> bool {
        !matches!(self, Language::JavaScript | Language::Other)
    }
}

/// Reads every module's text. A file whose bytes no longer hash to what the
/// graph recorded changed after resolution, and building from it would mix
/// two versions of the program.
pub fn read(
    graph: &ModuleGraph,
    id: ModuleId,
    packages: &mut PackageTypes,
) -> Result<Source, Diagnostic> {
    let module = graph.module(id);
    let path = module.path.as_str().to_string();
    let absolute = graph.absolute_path(id).as_path().to_path_buf();
    let bytes = std::fs::read(&absolute)
        .map_err(|error| Diagnostic::new(Code::Io, &path, format!("cannot read: {error}")))?;
    if ContentHash::of(&bytes) != module.content_hash {
        return Err(Diagnostic::new(
            Code::Io,
            &path,
            "changed on disk after the module graph was resolved; build again",
        ));
    }
    let language = Language::of(module.source);
    let mut format = match module.source {
        SourceKind::Json => Format::Json,
        SourceKind::Opaque => Format::Opaque,
        _ => match module.system {
            ModuleSystem::Esm => Format::Esm,
            _ => Format::Cjs,
        },
    };
    if matches!(format, Format::Opaque) {
        return Ok(Source {
            path,
            absolute,
            format,
            text: String::new(),
            language,
        });
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Diagnostic::new(Code::Parse, &path, "not valid UTF-8"))?;
    let text = match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => text,
    };
    if format == Format::Cjs
        && may_detect(&absolute)
        && packages.type_of(&absolute).is_none()
        && has_module_syntax(&text, language)
    {
        format = Format::Esm;
    }
    Ok(Source {
        path,
        absolute,
        format,
        text,
        language,
    })
}

/// Node detects ESM syntax only in files whose extension does not decide
/// their format (`lib/internal/modules/cjs/loader.js:1792-1802`).
fn may_detect(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        None | Some("js") | Some("ts") | Some("jsx") | Some("tsx")
    )
}

/// `containsModuleSyntax`: the file fails to parse as a CommonJS function
/// body and parses as a module. Those failures are what ESM-only syntax
/// causes: `import`/`export` declarations, `import.meta`, top-level `await`.
fn has_module_syntax(text: &str, language: Language) -> bool {
    let allocator = Allocator::default();
    let base = source_type(language);
    let as_script = Parser::new(&allocator, text, base.with_script(true))
        .with_options(ParseOptions {
            allow_return_outside_function: true,
            ..ParseOptions::default()
        })
        .parse();
    if as_script.diagnostics.is_empty() {
        return false;
    }
    let allocator = Allocator::default();
    let as_module = Parser::new(&allocator, text, base.with_module(true)).parse();
    as_module.diagnostics.is_empty()
}

fn source_type(language: Language) -> SourceType {
    match language {
        Language::JavaScript | Language::Other => SourceType::mjs(),
        Language::Jsx => SourceType::jsx(),
        Language::TypeScript => SourceType::ts(),
        Language::Tsx => SourceType::tsx(),
    }
}

/// The nearest `package.json` of each directory, and what the bundler reads
/// from it: `type` (rule 1) and `sideEffects` (rule 15).
#[derive(Default)]
pub struct PackageTypes {
    nearest: HashMap<PathBuf, Option<(PathBuf, std::rc::Rc<Value>)>>,
}

impl PackageTypes {
    /// `Some("module")`, `Some("commonjs")`, or `None` when the nearest
    /// `package.json` has no `type` (or there is none).
    pub fn type_of(&mut self, file: &Path) -> Option<String> {
        let (_, manifest) = self.nearest(file.parent()?)?;
        manifest
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// Whether the nearest `package.json` says `file` has no side effects:
    /// `"sideEffects": false`, or an array of globs (relative to the
    /// package; one with no `/` matches at any depth, as webpack reads it)
    /// that `file` matches none of.
    pub fn side_effect_free(&mut self, file: &Path) -> bool {
        let Some((dir, manifest)) = file.parent().and_then(|d| self.nearest(d)) else {
            return false;
        };
        match manifest.get("sideEffects") {
            Some(Value::Bool(false)) => true,
            Some(Value::Array(patterns)) => {
                let Ok(relative) = file.strip_prefix(&dir) else {
                    return false;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                !patterns.iter().filter_map(Value::as_str).any(|pattern| {
                    let pattern = pattern.trim_start_matches("./");
                    if pattern.contains('/') {
                        crate::glob::matches(pattern, &relative)
                    } else {
                        crate::glob::matches(&format!("**/{pattern}"), &relative)
                    }
                })
            }
            _ => false,
        }
    }

    fn nearest(&mut self, dir: &Path) -> Option<(PathBuf, std::rc::Rc<Value>)> {
        if let Some(found) = self.nearest.get(dir) {
            return found.clone();
        }
        let manifest = dir.join("package.json");
        let found = match std::fs::read_to_string(&manifest) {
            Ok(text) => serde_json::from_str::<Value>(&text)
                .ok()
                .map(|value| (dir.to_path_buf(), std::rc::Rc::new(value))),
            Err(_) => dir.parent().and_then(|parent| self.nearest(parent)),
        };
        self.nearest.insert(dir.to_path_buf(), found.clone());
        found
    }
}

/// A parsed, compiled module: JavaScript only, with fresh scoping.
pub struct Parsed<'a> {
    pub program: Program<'a>,
    pub scoping: Scoping,
    pub text: &'a str,
    pub lines: LineIndex,
}

/// Parses `source` into `allocator`, reports any syntax error as E-PARSE,
/// and compiles TypeScript and JSX away.
pub fn parse<'a>(
    allocator: &'a Allocator,
    source: &Source,
    settings: &TsSettings,
) -> Result<Parsed<'a>, Diagnostic> {
    let text: &'a str = allocator.alloc_str(&source.text);
    let lines = LineIndex::new(text);
    let source_type = match source.format {
        Format::Esm => source_type(source.language).with_module(true),
        _ => source_type(source.language).with_script(true),
    };
    let parsed = Parser::new(allocator, text, source_type)
        .with_options(ParseOptions {
            parse_regular_expression: true,
            allow_return_outside_function: source.format == Format::Cjs,
            ..ParseOptions::default()
        })
        .parse();
    let error_at = |span: Option<oxc_span::Span>, message: String| {
        let diagnostic = Diagnostic::new(Code::Parse, &source.path, message);
        match span {
            Some(span) => diagnostic.at(lines.locate(text, span.start)),
            None => diagnostic,
        }
    };
    if let Some(error) = parsed.diagnostics.first() {
        let span = error
            .labels
            .first()
            .map(|label| oxc_span::Span::new(label.offset(), label.offset() + label.len()));
        return Err(error_at(span, error.message.to_string()));
    }
    let mut program = parsed.program;

    let semantic = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .with_enum_eval(source.language.is_typescript())
        .build(&program);
    if let Some(error) = semantic.diagnostics.first() {
        let span = error
            .labels
            .first()
            .map(|label| oxc_span::Span::new(label.offset(), label.offset() + label.len()));
        return Err(error_at(span, error.message.to_string()));
    }
    let mut scoping = semantic.semantic.into_scoping();

    if source.language.needs_compiling() {
        if source.language.is_typescript()
            && !settings.experimental_decorators
            && has_decorators(&program)
        {
            return Err(Diagnostic::new(
                Code::Ts,
                &source.path,
                format!(
                    "decorators need \"experimentalDecorators\": true in {}; opal does not compile TC39 decorators",
                    settings.source.as_deref().unwrap_or("a tsconfig.json")
                ),
            ));
        }
        if settings.jsx == Jsx::Preserve && matches!(source.language, Language::Jsx | Language::Tsx)
        {
            return Err(Diagnostic::new(
                Code::Ts,
                &source.path,
                "\"jsx\": \"preserve\" leaves JSX in the output, which no runtime runs",
            ));
        }
        let options = transform_options(source, settings);
        let returned = Transformer::new(allocator, &source.absolute, &options)
            .build_with_scoping(scoping, &mut program);
        if let Some(error) = returned.diagnostics.first() {
            return Err(Diagnostic::new(
                Code::Ts,
                &source.path,
                error.message.to_string(),
            ));
        }
        // The transform keeps scoping as it goes, but a fresh build is the
        // one every later step can rely on without knowing which transforms
        // ran.
        scoping = SemanticBuilder::new()
            .build(&program)
            .semantic
            .into_scoping();
    }

    Ok(Parsed {
        program,
        scoping,
        text,
        lines,
    })
}

fn transform_options(source: &Source, settings: &TsSettings) -> TransformOptions {
    let mut options = TransformOptions::default();
    let (jsx_runtime, import_source, pragma, pragma_frag) = match &settings.jsx {
        Jsx::Automatic { import_source } => (
            JsxRuntime::Automatic,
            Some(import_source.clone()),
            None,
            None,
        ),
        Jsx::Classic { factory, fragment } => (
            JsxRuntime::Classic,
            None,
            Some(factory.clone()),
            Some(fragment.clone()),
        ),
        Jsx::Preserve => (JsxRuntime::Automatic, None, None, None),
    };
    options.typescript = TypeScriptOptions {
        only_remove_type_imports: settings.verbatim_module_syntax,
        remove_class_fields_without_initializer: !settings.use_define_for_class_fields,
        ..TypeScriptOptions::default()
    };
    if let Some(pragma) = &pragma {
        options.typescript.jsx_pragma = pragma.clone().into();
    }
    if let Some(pragma_frag) = &pragma_frag {
        options.typescript.jsx_pragma_frag = pragma_frag.clone().into();
    }
    options.assumptions.set_public_class_fields = !settings.use_define_for_class_fields;
    options.decorator = DecoratorOptions {
        legacy: settings.experimental_decorators,
        emit_decorator_metadata: settings.emit_decorator_metadata,
        strict_null_checks: settings.strict_null_checks,
    };
    if matches!(source.language, Language::Jsx | Language::Tsx) {
        options.jsx = JsxOptions {
            runtime: jsx_runtime,
            import_source,
            pragma,
            pragma_frag,
            development: false,
            ..JsxOptions::enable()
        };
    } else {
        options.jsx = JsxOptions::disable();
    }
    options
}

fn has_decorators(program: &Program<'_>) -> bool {
    use oxc_ast_visit::Visit;
    struct Finder(bool);
    impl<'a> Visit<'a> for Finder {
        fn visit_decorator(&mut self, _: &oxc_ast::ast::Decorator<'a>) {
            self.0 = true;
        }
    }
    let mut finder = Finder(false);
    finder.visit_program(program);
    finder.0
}

/// Settings per file, from the nearest tsconfig.json.
pub fn settings_for(
    configs: &mut TsConfigs,
    source: &Source,
    jsx_import_source: &str,
) -> Result<TsSettings, Diagnostic> {
    if !source.language.needs_compiling() {
        return Ok(TsSettings::defaults(jsx_import_source));
    }
    configs
        .settings_for(&source.absolute)
        .map_err(|message| Diagnostic::new(Code::Ts, &source.path, message))
}
