//! TypeScript type stripping, as Node does it: types are replaced with
//! whitespace, so that every other byte of the file stays where it was and
//! the line and column of everything that runs are the source's. Nothing is
//! transformed. TypeScript that only a transformation could run (enums,
//! namespaces with values, parameter properties, `import x = require()`,
//! `export =`, `<T>expr`) is refused, as `node --strip-types` refuses it,
//! with the same messages.
//!
//! The reference is Node 24's own stripper (swc's strip-only mode, behind
//! `module.stripTypeScriptTypes`): for a program it accepts, the output here
//! is meant to be byte-for-byte its output, including where it puts a `;` so
//! that erasing a statement doesn't join the statements around it, and the
//! cross-check in opal-cli's tests compares the two on a corpus.
//!
//! The parser is oxc's. Its AST has no spans for modifiers (`private`,
//! `readonly`) or for the `?` and `!` markers, so those are found by
//! scanning the source between spans the AST does have, skipping whitespace
//! and comments.

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::scope::ScopeFlags;

/// Why a file couldn't be stripped, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripError {
    pub kind: StripErrorKind,
    pub message: String,
    /// Byte offsets of the construct the error is about.
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripErrorKind {
    /// Valid TypeScript that type stripping can't run:
    /// ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX in Node.
    Unsupported,
    /// Not valid TypeScript: ERR_INVALID_TYPESCRIPT_SYNTAX in Node.
    Invalid,
}

impl StripErrorKind {
    /// The error code Node gives the SyntaxError.
    pub fn code(self) -> &'static str {
        match self {
            StripErrorKind::Unsupported => "ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX",
            StripErrorKind::Invalid => "ERR_INVALID_TYPESCRIPT_SYNTAX",
        }
    }
}

/// Bumped whenever the output for some input changes, so that a cache keyed
/// on the source (the runtime's) never serves an older stripper's output.
pub const STRIPPER_VERSION: u32 = 1;

/// `source` with its types replaced by whitespace.
pub fn strip_types(source: &str) -> Result<String, StripError> {
    let allocator = Allocator::default();
    let source_type = SourceType::ts().with_unambiguous(true);
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if let Some(error) = parsed.diagnostics.first() {
        let span = error
            .labels
            .first()
            .map(|label| (label.offset(), label.offset() + label.len()))
            .unwrap_or((0, 0));
        return Err(StripError {
            kind: StripErrorKind::Invalid,
            message: error.message.to_string(),
            start: span.0,
            end: span.1,
        });
    }
    let comments: Vec<(u32, u32)> = parsed
        .program
        .comments
        .iter()
        .map(|c| (c.span.start, c.span.end))
        .collect();
    let mut stripper = Stripper {
        source,
        comments,
        erase: Vec::new(),
        replacements: Vec::new(),
        error: None,
    };
    stripper.visit_program(&parsed.program);
    if let Some(error) = stripper.error {
        return Err(error);
    }
    Ok(stripper.render())
}

struct Stripper<'s> {
    source: &'s str,
    /// Every comment's span, in order.
    comments: Vec<(u32, u32)>,
    /// Byte ranges to blank.
    erase: Vec<(u32, u32)>,
    /// Bytes written over erased ones: the `;` that keeps an erased
    /// statement from joining its neighbors, an arrow's `)` moved to the end
    /// of its return type.
    replacements: Vec<(u32, u8)>,
    /// The first unsupported construct, by position.
    error: Option<StripError>,
}

impl<'s> Stripper<'s> {
    fn render(mut self) -> String {
        let mut out = self.source.as_bytes().to_vec();
        self.erase.sort_unstable();
        for (start, end) in self.erase {
            blank(&mut out, start as usize, end as usize);
        }
        for (at, byte) in self.replacements {
            out[at as usize] = byte;
        }
        // Blanking whole characters keeps the bytes valid UTF-8: a multi-byte
        // character is either wholly inside an erased range or wholly out.
        String::from_utf8(out).expect("erased ranges fall on character boundaries")
    }

    fn erase(&mut self, span: Span) {
        if span.start < span.end {
            self.erase.push((span.start, span.end));
        }
    }

    fn erase_range(&mut self, start: u32, end: u32) {
        if start < end {
            self.erase.push((start, end));
        }
    }

    fn unsupported(&mut self, span: Span, message: &str) {
        self.fail(StripErrorKind::Unsupported, span, message);
    }

    fn fail(&mut self, kind: StripErrorKind, span: Span, message: &str) {
        if self.error.as_ref().is_some_and(|e| e.start <= span.start) {
            return;
        }
        self.error = Some(StripError {
            kind,
            message: message.to_string(),
            start: span.start,
            end: span.end,
        });
    }

    /// The offset of the next byte at or after `at` that is not whitespace
    /// or inside a comment.
    fn skip_trivia(&self, mut at: u32) -> u32 {
        let bytes = self.source.as_bytes();
        let mut i = at as usize;
        loop {
            while i < bytes.len() && is_space(self.source, i) {
                i += char_len(bytes[i]);
            }
            if bytes[i..].starts_with(b"//") {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i..].starts_with(b"/*") {
                match self.source[i + 2..].find("*/") {
                    Some(end) => i += 2 + end + 2,
                    None => i = bytes.len(),
                }
                continue;
            }
            break;
        }
        at = i as u32;
        at
    }

    /// The offset and value of the last byte of the significant token before
    /// `at`, skipping back over whitespace and comments, or None at the
    /// start of the file.
    fn previous_significant(&self, at: u32) -> Option<(u32, u8)> {
        let bytes = self.source.as_bytes();
        let mut i = at as usize;
        loop {
            while i > 0 && is_space_before(self.source, i) {
                i -= self.source[..i]
                    .chars()
                    .next_back()
                    .map_or(1, char::len_utf8);
            }
            if i == 0 {
                return None;
            }
            // Inside (at the end of) a comment: continue before it.
            let comment = self
                .comments
                .partition_point(|&(_, end)| (end as usize) < i);
            match self.comments.get(comment) {
                Some(&(start, end)) if (start as usize) < i && i <= end as usize => {
                    i = start as usize
                }
                _ => return Some((i as u32 - 1, bytes[i - 1])),
            }
        }
    }

    /// The word that ends at the significant token before `at`, if that
    /// token is a word.
    fn previous_word(&self, at: u32) -> Option<&'s str> {
        let (end, _) = self.previous_significant(at)?;
        let end = end as usize + 1;
        let start = self.source[..end]
            .char_indices()
            .rev()
            .take_while(|&(_, c)| c.is_alphanumeric() || c == '_' || c == '$')
            .last()
            .map(|(i, _)| i)?;
        Some(&self.source[start..end])
    }

    /// Erases a whole statement, keeping the statements on either side of it
    /// from running together, as swc does. When the token after it starts a
    /// line with `(`, `[`, `` ` ``, `+`, `-` or `/`, it looks at the token
    /// before it: a `;` an earlier erasure removed is put back, any other
    /// token but `;` gets a `;` at the statement's start, and nothing (the
    /// start of the file) needs nothing.
    fn erase_statement(&mut self, span: Span) {
        self.erase(span);
        let next = self.skip_trivia(span.end) as usize;
        let continues = self
            .source
            .as_bytes()
            .get(next)
            .is_some_and(|b| matches!(b, b'(' | b'[' | b'`' | b'+' | b'-' | b'/'));
        if !continues || !has_line_break(&self.source[span.end as usize..next]) {
            return;
        }
        match self.previous_significant(span.start) {
            None => {}
            Some((at, b';')) => {
                if self
                    .erase
                    .iter()
                    .any(|&(start, end)| start <= at && at < end)
                {
                    self.replacements.push((at, b';'));
                }
            }
            Some(_) => self.replacements.push((span.start, b';')),
        }
    }

    /// Erases the TypeScript-only keywords among those from `start` up to
    /// `end` (a class member's modifiers, before its key).
    fn erase_modifiers(&mut self, start: u32, end: u32) {
        let mut at = start;
        loop {
            at = self.skip_trivia(at);
            if at >= end {
                break;
            }
            let word = word_at(self.source, at as usize);
            if word.is_empty() {
                break;
            }
            let after = at + word.len() as u32;
            if matches!(
                word,
                "public"
                    | "private"
                    | "protected"
                    | "readonly"
                    | "override"
                    | "declare"
                    | "abstract"
            ) {
                self.erase_range(at, after);
            }
            at = after;
        }
    }

    /// Erases a `?` or `!` marker at the next significant byte from `at`.
    fn erase_marker(&mut self, at: u32, marker: u8) -> u32 {
        let next = self.skip_trivia(at);
        if self.source.as_bytes().get(next as usize) == Some(&marker) {
            self.erase_range(next, next + 1);
            return next + 1;
        }
        at
    }

    /// The end of a class member's or property's key, past the `]` of a
    /// computed key.
    fn key_end(&self, key: &PropertyKey, computed: bool) -> u32 {
        let end = key.span().end;
        if !computed {
            return end;
        }
        let next = self.skip_trivia(end);
        if self.source.as_bytes().get(next as usize) == Some(&b']') {
            next + 1
        } else {
            end
        }
    }

    fn member_start(&self, span: Span, decorators: &[Decorator]) -> u32 {
        decorators.last().map_or(span.start, |d| d.span.end)
    }

    /// Whether a namespace holds anything that exists at run time.
    fn namespace_has_values(body: &TSNamespaceDeclarationBody) -> bool {
        match body {
            TSNamespaceDeclarationBody::TSNamespaceDeclaration(inner) => {
                !inner.declare && Self::namespace_has_values(&inner.body)
            }
            TSNamespaceDeclarationBody::TSModuleBlock(block) => block
                .body
                .iter()
                .any(|statement| !Self::is_type_only(statement)),
        }
    }

    fn is_type_only(statement: &Statement) -> bool {
        if let Some(declaration) = statement.as_declaration() {
            return Self::is_type_only_declaration(declaration);
        }
        match statement {
            Statement::ExportDeclaration(export) => {
                Self::is_type_only_declaration(&export.declaration)
            }
            Statement::ExportNamedDeclaration(export) => {
                export.export_kind == ImportOrExportKind::Type
            }
            Statement::ImportDeclaration(import) => import.import_kind == ImportOrExportKind::Type,
            _ => false,
        }
    }

    fn is_type_only_declaration(declaration: &Declaration) -> bool {
        match declaration {
            Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_) => true,
            Declaration::TSNamespaceDeclaration(ns) => {
                ns.declare || !Self::namespace_has_values(&ns.body)
            }
            Declaration::TSEnumDeclaration(e) => e.declare,
            Declaration::VariableDeclaration(v) => v.declare,
            Declaration::FunctionDeclaration(f) => {
                f.declare || f.r#type == FunctionType::TSDeclareFunction
            }
            Declaration::ClassDeclaration(c) => c.declare,
            Declaration::TSImportEqualsDeclaration(import) => {
                import.import_kind == ImportOrExportKind::Type
            }
            Declaration::TSExternalModuleDeclaration(_) | Declaration::TSGlobalDeclaration(_) => {
                true
            }
        }
    }

    /// Whether a whole statement holds only types and is erased; records
    /// an error for one that holds TypeScript that can't be stripped.
    fn erasable_statement(&mut self, statement: &Statement) -> bool {
        if let Some(declaration) = statement.as_declaration() {
            return self.erasable_declaration(declaration);
        }
        match statement {
            Statement::TSExportAssignment(assignment) => {
                self.unsupported(
                    assignment.span,
                    "TypeScript export assignment is not supported in strip-only mode",
                );
                false
            }
            Statement::ImportDeclaration(import) => import.import_kind == ImportOrExportKind::Type,
            Statement::ExportNamedDeclaration(export) => {
                export.export_kind == ImportOrExportKind::Type
            }
            Statement::ExportFromDeclaration(export) => {
                export.export_kind == ImportOrExportKind::Type
            }
            Statement::ExportAllDeclaration(export) => {
                export.export_kind == ImportOrExportKind::Type
            }
            Statement::ExportDeclaration(export) => self.erasable_declaration(&export.declaration),
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => true,
                ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                    f.r#type == FunctionType::TSDeclareFunction
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn erasable_declaration(&mut self, declaration: &Declaration) -> bool {
        match declaration {
            Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_) => true,
            Declaration::VariableDeclaration(v) => v.declare,
            Declaration::FunctionDeclaration(f) => {
                f.declare || f.r#type == FunctionType::TSDeclareFunction
            }
            Declaration::ClassDeclaration(c) => c.declare,
            Declaration::TSEnumDeclaration(e) => {
                if !e.declare {
                    self.unsupported(
                        e.span,
                        "TypeScript enum is not supported in strip-only mode",
                    );
                }
                e.declare
            }
            Declaration::TSNamespaceDeclaration(ns) => {
                if ns.kind == TSNamespaceDeclarationKind::Module {
                    self.unsupported(
                        ns.span,
                        "`module` keyword is not supported. Use `namespace` instead.",
                    );
                    return false;
                }
                if ns.declare || !Self::namespace_has_values(&ns.body) {
                    return true;
                }
                self.unsupported(
                    ns.span,
                    "TypeScript namespace declaration is not supported in strip-only mode",
                );
                false
            }
            Declaration::TSExternalModuleDeclaration(_) | Declaration::TSGlobalDeclaration(_) => {
                true
            }
            Declaration::TSImportEqualsDeclaration(import) => {
                if import.import_kind == ImportOrExportKind::Type {
                    return true;
                }
                self.unsupported(
                    import.span,
                    "TypeScript import equals declaration is not supported in strip-only mode",
                );
                false
            }
        }
    }

    /// Erases the specifiers marked `type`, each with the comma after it,
    /// if there is one (up to the next specifier, or a trailing comma).
    fn erase_specifiers(&mut self, spans: &[(Span, bool)]) {
        for (i, &(span, is_type)) in spans.iter().enumerate() {
            if !is_type {
                continue;
            }
            match spans.get(i + 1) {
                Some((next, _)) => self.erase_range(span.start, next.start),
                None => {
                    let after = self.skip_trivia(span.end);
                    if self.source.as_bytes().get(after as usize) == Some(&b',') {
                        self.erase_range(span.start, after + 1);
                    } else {
                        self.erase(span);
                    }
                }
            }
        }
    }

    /// Erases a class member's TypeScript modifiers, which come before its
    /// key, and, as swc does, starts the member with a `;` when it is an
    /// undecorated, non-static generator or computed member whose first
    /// token was such a modifier.
    fn strip_member_modifiers(&mut self, start: u32, key_start: u32, ends_like_statement: bool) {
        let erased_before = self.erase.len();
        self.erase_modifiers(start, key_start);
        if ends_like_statement
            && self.erase.len() > erased_before
            && self.erase[erased_before].0 == self.skip_trivia(start)
        {
            self.replacements.push((self.erase[erased_before].0, b';'));
        }
    }
}

fn has_line_break(text: &str) -> bool {
    text.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

fn is_space(source: &str, i: usize) -> bool {
    let rest = &source[i..];
    let Some(c) = rest.chars().next() else {
        return false;
    };
    c.is_whitespace() || c == '\u{feff}'
}

fn char_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// The identifier-like word at `i`, or "" if there isn't one.
fn word_at(source: &str, i: usize) -> &str {
    let rest = &source[i..];
    let end = rest
        .char_indices()
        .find(|&(_, c)| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .map_or(rest.len(), |(at, _)| at);
    &rest[..end]
}

/// Replaces the characters in bytes `start..end` with whitespace of the
/// same UTF-8 length, as swc does: whitespace and line breaks stay as they
/// are, and anything else becomes a space (one byte), U+00A0 (two), U+2002
/// (three), or a space and U+FEFF (four).
fn blank(out: &mut [u8], start: usize, end: usize) {
    let text = std::str::from_utf8(&out[start..end])
        .expect("erased ranges are whole characters")
        .to_string();
    let mut at = start;
    for c in text.chars() {
        let len = c.len_utf8();
        if !is_js_whitespace(c) {
            let replacement: &[u8] = match len {
                1 => b" ",
                2 => "\u{a0}".as_bytes(),
                3 => "\u{2002}".as_bytes(),
                _ => " \u{feff}".as_bytes(),
            };
            out[at..at + len].copy_from_slice(replacement);
        }
        at += len;
    }
}

/// JavaScript's WhiteSpace and LineTerminator characters.
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\u{b}'
            | '\u{c}'
            | ' '
            | '\u{a0}'
            | '\u{feff}'
            | '\n'
            | '\r'
            | '\u{2028}'
            | '\u{2029}'
    ) || (c != '\u{85}' && c.is_whitespace() && !c.is_ascii())
}

fn is_space_before(source: &str, i: usize) -> bool {
    source[..i]
        .chars()
        .next_back()
        .is_some_and(is_js_whitespace)
}

impl<'a> Visit<'a> for Stripper<'_> {
    fn visit_statement(&mut self, statement: &Statement<'a>) {
        if self.erasable_statement(statement) {
            self.erase_statement(statement.span());
            return;
        }
        walk::walk_statement(self, statement);
    }

    fn visit_import_declaration(&mut self, import: &ImportDeclaration<'a>) {
        if let Some(specifiers) = &import.specifiers {
            let spans: Vec<(Span, bool)> = specifiers
                .iter()
                .map(|s| match s {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        (s.span, s.import_kind == ImportOrExportKind::Type)
                    }
                    other => (other.span(), false),
                })
                .collect();
            self.erase_specifiers(&spans);
        }
        walk::walk_import_declaration(self, import);
    }

    fn visit_export_named_declaration(&mut self, export: &ExportNamedDeclaration<'a>) {
        let spans: Vec<(Span, bool)> = export
            .specifiers
            .iter()
            .map(|s| (s.span, s.export_kind == ImportOrExportKind::Type))
            .collect();
        self.erase_specifiers(&spans);
        walk::walk_export_named_declaration(self, export);
    }

    fn visit_export_from_declaration(&mut self, export: &ExportFromDeclaration<'a>) {
        let spans: Vec<(Span, bool)> = export
            .specifiers
            .iter()
            .map(|s| (s.span, s.export_kind == ImportOrExportKind::Type))
            .collect();
        self.erase_specifiers(&spans);
        walk::walk_export_from_declaration(self, export);
    }

    fn visit_ts_type_annotation(&mut self, annotation: &TSTypeAnnotation<'a>) {
        self.erase(annotation.span);
    }

    fn visit_ts_type_parameter_declaration(&mut self, params: &TSTypeParameterDeclaration<'a>) {
        self.erase(params.span);
    }

    fn visit_ts_type_parameter_instantiation(&mut self, args: &TSTypeParameterInstantiation<'a>) {
        self.erase(args.span);
    }

    fn visit_ts_as_expression(&mut self, expression: &TSAsExpression<'a>) {
        self.erase_range(expression.expression.span().end, expression.span.end);
        self.visit_expression(&expression.expression);
    }

    fn visit_ts_satisfies_expression(&mut self, expression: &TSSatisfiesExpression<'a>) {
        self.erase_range(expression.expression.span().end, expression.span.end);
        self.visit_expression(&expression.expression);
    }

    fn visit_ts_non_null_expression(&mut self, expression: &TSNonNullExpression<'a>) {
        self.erase_range(expression.span.end - 1, expression.span.end);
        self.visit_expression(&expression.expression);
    }

    fn visit_ts_type_assertion(&mut self, assertion: &TSTypeAssertion<'a>) {
        self.unsupported(
            assertion.span,
            "The angle-bracket syntax for type assertions, `<T>expr`, is not supported in type strip mode. Instead, use the 'as' syntax: `expr as T`.",
        );
    }

    fn visit_ts_instantiation_expression(&mut self, expression: &TSInstantiationExpression<'a>) {
        self.erase(expression.type_arguments.span);
        self.visit_expression(&expression.expression);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if declarator.definite {
            self.erase_marker(declarator.id.span().end, b'!');
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_formal_parameter(&mut self, param: &FormalParameter<'a>) {
        if param.accessibility.is_some() || param.readonly || param.r#override {
            // swc's span for it is the parameter without its modifiers.
            let end = param
                .type_annotation
                .as_ref()
                .map_or(param.pattern.span().end, |t| t.span.end);
            let span = Span::new(param.pattern.span().start, end);
            self.unsupported(
                span,
                "TypeScript parameter property is not supported in strip-only mode",
            );
        }
        if param.optional {
            self.erase_marker(param.pattern.span().end, b'?');
        }
        walk::walk_formal_parameter(self, param);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        if let Some(this) = &function.this_param {
            // `this: T,` goes with its comma, when more parameters follow.
            let end = match function.params.items.first() {
                Some(next) => next.span.start,
                None => match &function.params.rest {
                    Some(rest) => rest.span.start,
                    None => this.span.end,
                },
            };
            self.erase_range(this.span.start, end);
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        let breaks = has_line_break;
        if let Some(params) = &arrow.type_parameters {
            // Type parameters over several lines leave a line break before
            // `(` once erased, which would make `async <T>(..) =>` a call
            // and `return <T>(..) =>` a bare return; in those two places
            // (and after `throw` and `yield`), swc moves the `(` to where
            // `<` was.
            let open = arrow.params.span.start;
            let restricted = arrow.r#async
                || self
                    .previous_word(arrow.span.start)
                    .is_some_and(|word| matches!(word, "return" | "throw" | "yield"));
            if restricted
                && self.source.as_bytes()[open as usize] == b'('
                && breaks(&self.source[params.span.start as usize..params.span.end as usize])
            {
                self.erase_range(open, open + 1);
                self.replacements.push((params.span.start, b'('));
            }
        }
        if let Some(return_type) = &arrow.return_type {
            let arrow_at = self.skip_trivia(return_type.span.end);
            if breaks(&self.source[return_type.span.end as usize..arrow_at as usize]) {
                self.fail(
                    StripErrorKind::Invalid,
                    Span::new(return_type.span.end, arrow_at),
                    "Unexpected line break between arrow head and arrow",
                );
            } else {
                // A return type over several lines would leave a line break
                // between `)` and `=>` once erased, which JavaScript forbids;
                // swc moves the `)` to the type's last byte.
                let close = arrow.params.span.end - 1;
                if self.source.as_bytes()[close as usize] == b')'
                    && breaks(&self.source[close as usize..return_type.span.end as usize])
                {
                    self.erase_range(close, close + 1);
                    self.replacements.push((return_type.span.end - 1, b')'));
                }
            }
        }
        walk::walk_arrow_function_expression(self, arrow);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        for element in &class.body.body {
            let (is_abstract, overrides, span) = match element {
                ClassElement::PropertyDefinition(p) => (
                    p.r#type == PropertyDefinitionType::TSAbstractPropertyDefinition,
                    p.r#override,
                    p.span,
                ),
                ClassElement::MethodDefinition(m) => (
                    m.r#type == MethodDefinitionType::TSAbstractMethodDefinition,
                    m.r#override,
                    m.span,
                ),
                ClassElement::AccessorProperty(a) => (
                    a.r#type == AccessorPropertyType::TSAbstractAccessorProperty,
                    a.r#override,
                    a.span,
                ),
                _ => (false, false, Span::empty(0)),
            };
            if is_abstract && !class.r#abstract {
                self.fail(
                    StripErrorKind::Invalid,
                    span,
                    "Abstract methods can only appear within an abstract class.",
                );
            }
            if overrides && class.heritage.is_none() {
                self.fail(
                    StripErrorKind::Invalid,
                    span,
                    "This member cannot have an 'override' modifier because its containing class does not extend another class.",
                );
            }
        }
        if class.r#abstract {
            let start = class
                .decorators
                .last()
                .map_or(class.span.start, |d| d.span.end);
            let at = self.skip_trivia(start);
            if word_at(self.source, at as usize) == "abstract" {
                self.erase_range(at, at + "abstract".len() as u32);
            }
        }
        if let (Some(first), Some(last)) = (class.implements.first(), class.implements.last()) {
            let keyword = self.source[..first.span.start as usize]
                .rfind("implements")
                .unwrap_or(first.span.start as usize) as u32;
            self.erase_range(keyword, last.span.end);
        }
        for decorator in &class.decorators {
            self.visit_decorator(decorator);
        }
        if let Some(id) = &class.id {
            self.visit_binding_identifier(id);
        }
        if let Some(params) = &class.type_parameters {
            self.visit_ts_type_parameter_declaration(params);
        }
        if let Some(heritage) = &class.heritage {
            self.visit_expression(&heritage.expression);
            if let Some(args) = &heritage.type_arguments {
                self.visit_ts_type_parameter_instantiation(args);
            }
        }
        for element in &class.body.body {
            self.visit_class_element(element);
        }
    }

    fn visit_class_element(&mut self, element: &ClassElement<'a>) {
        match element {
            ClassElement::TSIndexSignature(signature) => self.erase(signature.span),
            ClassElement::PropertyDefinition(property) => {
                if property.declare
                    || property.r#type == PropertyDefinitionType::TSAbstractPropertyDefinition
                {
                    self.erase(property.span);
                    return;
                }
                let start = self.member_start(property.span, &property.decorators);
                let hazard =
                    property.decorators.is_empty() && !property.r#static && property.computed;
                self.strip_member_modifiers(start, property.key.span().start, hazard);
                let key_end = self.key_end(&property.key, property.computed);
                if property.optional {
                    self.erase_marker(key_end, b'?');
                }
                if property.definite {
                    self.erase_marker(key_end, b'!');
                }
                walk::walk_property_definition(self, property);
            }
            ClassElement::MethodDefinition(method) => {
                if method.r#type == MethodDefinitionType::TSAbstractMethodDefinition
                    || method.value.r#type == FunctionType::TSEmptyBodyFunctionExpression
                {
                    self.erase(method.span);
                    return;
                }
                let start = self.member_start(method.span, &method.decorators);
                let hazard = method.decorators.is_empty()
                    && !method.r#static
                    && (method.computed || method.value.generator);
                self.strip_member_modifiers(start, method.key.span().start, hazard);
                if method.optional {
                    let key_end = self.key_end(&method.key, method.computed);
                    self.erase_marker(key_end, b'?');
                }
                walk::walk_method_definition(self, method);
            }
            ClassElement::AccessorProperty(accessor) => {
                if accessor.r#type == AccessorPropertyType::TSAbstractAccessorProperty {
                    self.erase(accessor.span);
                    return;
                }
                let start = self.member_start(accessor.span, &accessor.decorators);
                self.erase_modifiers(start, accessor.key.span().start);
                if accessor.definite {
                    let key_end = self.key_end(&accessor.key, accessor.computed);
                    self.erase_marker(key_end, b'!');
                }
                walk::walk_accessor_property(self, accessor);
            }
            ClassElement::StaticBlock(block) => walk::walk_static_block(self, block),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected outputs are node 24.19.0's `module.stripTypeScriptTypes` on
    // the same input; tests/strip-cross-check.rs compares against a live node.

    #[test]
    fn types_become_whitespace_and_nothing_moves() {
        assert_eq!(
            strip_types("function f<T>(a: T, b?: string): T { return a as T; }").unwrap(),
            "function f   (a   , b         )    { return a     ; }"
        );
    }

    #[test]
    fn an_erased_statement_keeps_its_neighbors_apart() {
        assert_eq!(
            strip_types("let x = 1\ntype A = string\n(foo)()").unwrap(),
            "let x = 1\n;              \n(foo)()"
        );
        assert_eq!(
            strip_types("type A = 1;\ntype B = 1;\n(b)").unwrap(),
            "          ;\n           \n(b)"
        );
        assert_eq!(strip_types("type A = 1\n(b)").unwrap(), "          \n(b)");
    }

    #[test]
    fn type_specifiers_go_with_their_commas() {
        assert_eq!(
            strip_types("import { type Y, Z } from 'y'\nimport { Z, type Y } from 'y'").unwrap(),
            "import {         Z } from 'y'\nimport { Z,        } from 'y'"
        );
    }

    #[test]
    fn multi_byte_characters_become_whitespace_of_their_length() {
        assert_eq!(
            strip_types("let a: \"é€😀\" = 1").unwrap(),
            "let a   \u{a0}\u{2002} \u{feff}  = 1"
        );
    }

    #[test]
    fn arrow_parens_move_to_keep_the_arrow_on_its_line() {
        assert_eq!(
            strip_types("const f = (a): {\n  x: 1\n} => a").unwrap(),
            "const f = (a    \n      \n) => a"
        );
        assert_eq!(
            strip_types("x = async <\nT>() => 1").unwrap(),
            "x = async (\n   ) => 1"
        );
    }

    #[test]
    fn what_needs_a_transform_is_refused_with_nodes_message() {
        for (source, message) in [
            (
                "enum E { A }",
                "TypeScript enum is not supported in strip-only mode",
            ),
            (
                "namespace N { export const x = 1 }",
                "TypeScript namespace declaration is not supported in strip-only mode",
            ),
            (
                "class P { constructor(public a: number) {} }",
                "TypeScript parameter property is not supported in strip-only mode",
            ),
            (
                "import x = require('x')",
                "TypeScript import equals declaration is not supported in strip-only mode",
            ),
            (
                "export = x",
                "TypeScript export assignment is not supported in strip-only mode",
            ),
            (
                "module M {}",
                "`module` keyword is not supported. Use `namespace` instead.",
            ),
        ] {
            let error = strip_types(source).unwrap_err();
            assert_eq!(error.kind, StripErrorKind::Unsupported, "{source}");
            assert_eq!(error.message, message, "{source}");
        }
        let error = strip_types("const i = (a): string\n  => a").unwrap_err();
        assert_eq!(error.kind, StripErrorKind::Invalid);
    }
}
