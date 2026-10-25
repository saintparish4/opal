//! The CommonJS export lexer Node uses to decide a CommonJS module's named
//! exports when an ES module imports it.
//!
//! This is a port of merve 1.2.2 (`deps/merve` in Node v24.19.0), which
//! replaced cjs-module-lexer in Node 24. It keeps merve's structure: one
//! byte-oriented state machine whose functions carry merve's names in snake
//! case. `tests/cjs-lexer.rs` runs Node's own lexer over a corpus of real
//! packages and checks this port returns the same thing for every file.
//!
//! merve is MIT licensed:
//!
//! ```text
//! Copyright 2026 Yagiz Nizipli
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy of
//! this software and associated documentation files (the "Software"), to deal in
//! the Software without restriction, including without limitation the rights to
//! use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
//! the Software, and to permit persons to whom the Software is furnished to do so,
//! subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
//! FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
//! COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
//! IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
//! CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//! ```
//!
//! Differences from the C++ that cannot change a result:
//!
//! - merve counts lines and records error locations. Node's binding ignores
//!   both, so they are left out.
//! - merve reads through raw pointers, sometimes one or two bytes past the
//!   end of the source (Node hands it a NUL-terminated buffer) or just
//!   before its start. Here every read goes through `Lexer::byte`, which
//!   returns 0 outside the source, the byte the terminator would give.
//! - merve's nesting stacks are fixed arrays of 2048 entries and overflow
//!   silently past that. Here they grow, so absurdly deep nesting lexes
//!   instead of corrupting memory.

/// What Node's `internalBinding('cjs_lexer').parse(source)` returns, as
/// (export names, reexport specifiers).
///
/// The binding puts the export names in a `Set`, in the order merve found
/// them, and the reexport specifiers in an array, duplicates kept. Each name
/// becomes a JS string from merve's UTF-8 bytes, invalid sequences replaced
/// by U+FFFD; that replacement can make two of merve's distinct names equal,
/// and the `Set` then keeps only the first. When merve fails (a syntax error
/// it detects, or ESM syntax) the binding returns an empty `Set` and an
/// empty array, so this returns two empty vectors.
pub fn exports(source: &str) -> (Vec<String>, Vec<String>) {
    let Ok(analysis) = parse_commonjs(source.as_bytes()) else {
        return (Vec::new(), Vec::new());
    };
    let mut names: Vec<String> = Vec::with_capacity(analysis.exports.len());
    for bytes in &analysis.exports {
        let name = String::from_utf8_lossy(bytes).into_owned();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let reexports = analysis
        .re_exports
        .iter()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect();
    (names, reexports)
}

/// Why merve rejected a source. Node does not distinguish them, but they
/// make the unit tests easier to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexError {
    UnexpectedParen,
    UnexpectedBrace,
    UnterminatedParen,
    UnterminatedBrace,
    UnterminatedTemplateString,
    UnterminatedStringLiteral,
    UnterminatedRegexCharacterClass,
    UnterminatedRegex,
    UnexpectedEsmImportMeta,
    UnexpectedEsmImport,
    UnexpectedEsmExport,
    TemplateNestOverflow,
}

/// merve's raw result: names as the bytes it produced, which are not always
/// valid UTF-8 because `\xHH` escapes are copied as single bytes.
#[derive(Debug, Default)]
struct Analysis {
    exports: Vec<Vec<u8>>,
    re_exports: Vec<Vec<u8>>,
}

/// merve's `parse_commonjs`.
fn parse_commonjs(source: &[u8]) -> Result<Analysis, LexError> {
    let mut lexer = Lexer::new(source);
    lexer.parse()?;
    Ok(Analysis {
        exports: lexer.exports,
        re_exports: lexer.re_exports,
    })
}

fn hex_digit(c: u8) -> Option<u32> {
    (c as char).to_digit(16)
}

/// merve's `kSimpleEscapeTable`: the single-character escapes.
fn simple_escape(c: u8) -> Option<u8> {
    Some(match c {
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'b' => 0x08,
        b'f' => 0x0c,
        b'v' => 0x0b,
        b'0' => 0,
        b'\\' => b'\\',
        b'\'' => b'\'',
        b'"' => b'"',
        _ => return None,
    })
}

/// merve's `encodeUtf8`. Callers never pass a surrogate or a value above
/// U+10FFFF, so `char::from_u32` always succeeds.
fn encode_utf8(out: &mut Vec<u8>, codepoint: u32) {
    if let Some(c) = char::from_u32(codepoint) {
        let mut buf = [0; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
}

/// Reads hex digits, wrapping on overflow like the C++ `uint32_t` it
/// ports. `None` if any byte is not a hex digit.
fn hex_value_wrapping(digits: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    for &d in digits {
        value = (value << 4) | hex_digit(d)?;
    }
    Some(value)
}

fn find_byte(str: &[u8], needle: u8, from: usize) -> Option<usize> {
    str.get(from..)?
        .iter()
        .position(|&b| b == needle)
        .map(|i| i + from)
}

/// merve's `unescapeJsString`. `None` for the escapes it rejects, such as a
/// lone surrogate, in which case the name is dropped.
///
/// It keeps merve's quirks: `\xHH` appends the raw byte rather than
/// encoding it, an unknown escape (a line continuation included) appends
/// the escaped character itself, and `\0` is NUL even before a digit.
fn unescape_js_string(str: &[u8]) -> Option<Vec<u8>> {
    let mut result = Vec::with_capacity(str.len());
    let mut i = 0;
    while i < str.len() {
        if str[i] != b'\\' {
            result.push(str[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= str.len() {
            return None;
        }
        if let Some(simple) = simple_escape(str[i]) {
            result.push(simple);
            i += 1;
            continue;
        }
        match str[i] {
            b'x' => {
                if i + 2 >= str.len() {
                    return None;
                }
                let h1 = hex_digit(str[i + 1])?;
                let h2 = hex_digit(str[i + 2])?;
                result.push(((h1 << 4) | h2) as u8);
                i += 2;
            }
            b'u' => {
                if i + 1 >= str.len() {
                    return None;
                }
                if str[i + 1] == b'{' {
                    let start = i + 2;
                    let mut end_brace = find_byte(str, b'}', start)?;
                    if end_brace == start {
                        return None;
                    }
                    let mut codepoint: u32 = 0;
                    for &d in &str[start..end_brace] {
                        codepoint = (codepoint << 4) | hex_digit(d)?;
                        if codepoint > 0x10FFFF {
                            return None;
                        }
                    }
                    if (0xD800..=0xDBFF).contains(&codepoint) {
                        // A high surrogate is accepted only when a braced low
                        // surrogate follows at once.
                        if !(end_brace + 3 < str.len()
                            && str[end_brace + 1] == b'\\'
                            && str[end_brace + 2] == b'u'
                            && str[end_brace + 3] == b'{')
                        {
                            return None;
                        }
                        let low_start = end_brace + 4;
                        let low_end = find_byte(str, b'}', low_start)?;
                        if low_end <= low_start {
                            return None;
                        }
                        match hex_value_wrapping(&str[low_start..low_end]) {
                            Some(low) if (0xDC00..=0xDFFF).contains(&low) => {
                                codepoint = 0x10000 + ((codepoint - 0xD800) << 10) + (low - 0xDC00);
                                end_brace = low_end;
                            }
                            _ => return None,
                        }
                    } else if (0xDC00..=0xDFFF).contains(&codepoint) {
                        return None;
                    }
                    encode_utf8(&mut result, codepoint);
                    i = end_brace;
                } else {
                    if i + 4 >= str.len() {
                        return None;
                    }
                    let mut codepoint = hex_value_wrapping(&str[i + 1..i + 5])?;
                    if (0xD800..=0xDBFF).contains(&codepoint) {
                        if !(i + 10 < str.len() && str[i + 5] == b'\\' && str[i + 6] == b'u') {
                            return None;
                        }
                        match hex_value_wrapping(&str[i + 7..i + 11]) {
                            Some(low) if (0xDC00..=0xDFFF).contains(&low) => {
                                codepoint = 0x10000 + ((codepoint - 0xD800) << 10) + (low - 0xDC00);
                                i += 6;
                            }
                            _ => return None,
                        }
                    } else if (0xDC00..=0xDFFF).contains(&codepoint) {
                        return None;
                    }
                    encode_utf8(&mut result, codepoint);
                    i += 4;
                }
            }
            other => result.push(other),
        }
        i += 1;
    }
    Some(result)
}

/// merve's `kPunctuatorTable`.
fn is_punctuator(ch: u8) -> bool {
    matches!(ch, b'!' | b'%' | b'&' | 40..=47 | 58..=63 | b'[' | b']' | b'^' | 123..=126)
}

/// merve's `kExpressionPunctuatorTable`: the punctuators after which a `/`
/// starts a regular expression. It leaves out `)`, `}`, `/` and `]`.
fn is_expression_punctuator(ch: u8) -> bool {
    matches!(ch, b'!' | b'%' | b'&' | 40 | 42..=46 | 58..=63 | b'[' | b'^' | 123 | 124 | 126)
}

/// merve's `kIdentifierStartTable`. Every byte of a non-ASCII character
/// counts, so names written in any script lex as identifiers.
fn is_identifier_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_' || ch == b'$' || ch >= 0x80
}

fn is_identifier_char(ch: u8) -> bool {
    is_identifier_start(ch) || ch.is_ascii_digit()
}

fn is_br(c: u8) -> bool {
    c == b'\r' || c == b'\n'
}

fn is_br_or_ws(c: u8) -> bool {
    (9..14).contains(&c) || c == b' '
}

fn is_br_or_ws_or_punctuator_not_dot(c: u8) -> bool {
    is_br_or_ws(c) || (is_punctuator(c) && c != b'.')
}

/// merve allows 256 star export bindings but uses only the first 255.
const MAX_STAR_EXPORTS: usize = 256;
/// merve's `STACK_DEPTH`, which bounds only the template stack here.
const STACK_DEPTH: u16 = 2048;
/// The template depth meaning "not inside a template", merve's
/// `numeric_limits<uint16_t>::max()`.
const NO_TEMPLATE: u16 = u16::MAX;
/// A never-written slot of the open token position stack. merve's array
/// starts zeroed and a null pointer compares below the source, so this must
/// sort below every position, `-1` included.
const NULL_POS: isize = isize::MIN / 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequireType {
    Import,
    ExportAssign,
    ExportStar,
}

/// A `var id = require('specifier')` binding, both as byte ranges of the
/// source, kept so a later Babel `Object.keys(id).forEach(...)` loop can be
/// recognized as a reexport of the specifier.
#[derive(Clone, Copy, Default)]
struct StarExportBinding {
    specifier: (isize, isize),
    id: (isize, isize),
}

/// Reads slot `i` of a stack that merve keeps as a zeroed fixed array.
fn stack_get<T: Copy>(stack: &[T], i: u16, zero: T) -> T {
    stack.get(usize::from(i)).copied().unwrap_or(zero)
}

/// Writes slot `i`, growing the stack instead of overflowing it. Slots keep
/// their old values when the depth drops, as merve's arrays do, and the `/`
/// heuristic reads them.
fn stack_set<T: Copy>(stack: &mut Vec<T>, i: u16, value: T, zero: T) {
    let i = usize::from(i);
    if stack.len() <= i {
        stack.resize(i + 1, zero);
    }
    stack[i] = value;
}

/// merve's `CJSLexer`. Positions are signed offsets into the source because
/// merve's pointers start one before it and run past its end.
struct Lexer<'a> {
    source: &'a [u8],
    pos: isize,
    end: isize,
    last_token_pos: isize,

    template_stack_depth: u16,
    open_token_depth: u16,
    template_depth: u16,

    last_slash_was_division: bool,
    next_brace_is_class: bool,

    template_stack: Vec<u16>,
    open_token_pos_stack: Vec<isize>,
    open_token_type_stack: Vec<u8>,
    open_class_pos_stack: Vec<bool>,
    star_export_stack: [StarExportBinding; MAX_STAR_EXPORTS],
    /// Index of merve's `starExportStack` pointer: the next free binding.
    star_export_top: usize,

    last_error: Option<LexError>,

    exports: Vec<Vec<u8>>,
    re_exports: Vec<Vec<u8>>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a [u8]) -> Self {
        Lexer {
            source,
            pos: -1,
            end: source.len() as isize,
            last_token_pos: -1,
            template_stack_depth: 0,
            open_token_depth: 0,
            template_depth: NO_TEMPLATE,
            last_slash_was_division: false,
            next_brace_is_class: false,
            template_stack: Vec::new(),
            open_token_pos_stack: Vec::new(),
            open_token_type_stack: Vec::new(),
            open_class_pos_stack: Vec::new(),
            star_export_stack: [StarExportBinding::default(); MAX_STAR_EXPORTS],
            star_export_top: 0,
            last_error: None,
            exports: Vec::new(),
            re_exports: Vec::new(),
        }
    }

    /// The byte at `p`, or 0 outside the source (see the module doc).
    fn byte(&self, p: isize) -> u8 {
        if p < 0 {
            return 0;
        }
        self.source.get(p as usize).copied().unwrap_or(0)
    }

    fn cur(&self) -> u8 {
        self.byte(self.pos)
    }

    /// The bytes from `start` to `end`, clamped to the source. They fall
    /// outside it only after a syntax error, when the result is discarded.
    fn slice(&self, start: isize, end: isize) -> &'a [u8] {
        let len = self.source.len() as isize;
        let start = start.clamp(0, len) as usize;
        let end = end.clamp(0, len) as usize;
        self.source.get(start..end).unwrap_or(&[])
    }

    /// merve's `while (pos++ < end)`: compares, then advances either way.
    fn advance_while_before_end(&mut self) -> bool {
        let before = self.pos < self.end;
        self.pos += 1;
        before
    }

    fn matches_at(&self, p: isize, expected: &[u8]) -> bool {
        // merve's length check, done on a size_t, only rejects when `p` is
        // at or before the end; past the end it compares (zero) bytes.
        if p <= self.end && ((self.end - p) as usize) < expected.len() {
            return false;
        }
        expected
            .iter()
            .enumerate()
            .all(|(i, &e)| self.byte(p + i as isize) == e)
    }

    fn push_open_token(&mut self, ty: u8, at: isize) {
        let depth = self.open_token_depth;
        stack_set(&mut self.open_token_type_stack, depth, ty, 0);
        stack_set(&mut self.open_token_pos_stack, depth, at, NULL_POS);
        self.open_token_depth = depth.wrapping_add(1);
    }

    fn keyword_start(&self, p: isize) -> bool {
        p == 0 || is_br_or_ws_or_punctuator_not_dot(self.byte(p - 1))
    }

    /// Whether `keyword` ends at `p` and starts a token.
    fn read_preceding_keyword(&self, p: isize, keyword: &[u8]) -> bool {
        let start = p - keyword.len() as isize + 1;
        if start < 0 {
            return false;
        }
        self.matches_at(start, keyword)
            && (start == 0 || is_br_or_ws_or_punctuator_not_dot(self.byte(start - 1)))
    }

    /// Whether the token ending at `p` is a keyword after which a `/`
    /// starts a regular expression.
    fn is_expression_keyword(&self, p: isize) -> bool {
        let b = |back: isize| self.byte(p - back);
        match b(0) {
            b'd' => match b(1) {
                b'i' => self.read_preceding_keyword(p - 2, b"vo"),
                b'l' => self.read_preceding_keyword(p - 2, b"yie"),
                _ => false,
            },
            b'e' => match b(1) {
                b's' => match b(2) {
                    b'l' => p - 3 >= 0 && b(3) == b'e' && self.keyword_start(p - 3),
                    b'a' => p - 3 >= 0 && b(3) == b'c' && self.keyword_start(p - 3),
                    _ => false,
                },
                b't' => self.read_preceding_keyword(p - 2, b"dele"),
                _ => false,
            },
            b'f' => {
                if b(1) != b'o' || b(2) != b'e' {
                    return false;
                }
                match b(3) {
                    b'c' => self.read_preceding_keyword(p - 4, b"instan"),
                    b'p' => self.read_preceding_keyword(p - 4, b"ty"),
                    _ => false,
                }
            }
            b'n' => {
                (p > 0 && b(1) == b'i' && self.keyword_start(p - 1))
                    || self.read_preceding_keyword(p - 1, b"retur")
            }
            b'o' => p > 0 && b(1) == b'd' && self.keyword_start(p - 1),
            b'r' => self.read_preceding_keyword(p - 1, b"debugge"),
            b't' => self.read_preceding_keyword(p - 1, b"awai"),
            b'w' => match b(1) {
                b'e' => p - 2 >= 0 && b(2) == b'n' && self.keyword_start(p - 2),
                b'o' => self.read_preceding_keyword(p - 2, b"thr"),
                _ => false,
            },
            _ => false,
        }
    }

    fn is_paren_keyword(&self, cur_pos: isize) -> bool {
        self.read_preceding_keyword(cur_pos, b"while")
            || self.read_preceding_keyword(cur_pos, b"for")
            || self.read_preceding_keyword(cur_pos, b"if")
    }

    fn is_expression_terminator(&self, cur_pos: isize) -> bool {
        match self.byte(cur_pos) {
            b'>' => self.byte(cur_pos - 1) == b'=',
            b';' | b')' => true,
            b'h' => self.read_preceding_keyword(cur_pos - 1, b"catc"),
            b'y' => self.read_preceding_keyword(cur_pos - 1, b"finall"),
            b'e' => self.read_preceding_keyword(cur_pos - 1, b"els"),
            _ => false,
        }
    }

    /// Records the first error and jumps past the end, which stops every
    /// loop still running. Lexing can resume when a caller reverts `pos`,
    /// but the parse fails all the same.
    fn syntax_error(&mut self, code: LexError) {
        if self.last_error.is_none() {
            self.last_error = Some(code);
        }
        self.pos = self.end + 1;
    }

    /// Skips whitespace and comments and returns the byte it stops on.
    fn comment_whitespace(&mut self) -> u8 {
        let mut ch;
        loop {
            if self.pos >= self.end {
                return 0;
            }
            ch = self.cur();
            if ch == b'/' {
                let next_ch = if self.pos + 1 < self.end {
                    self.byte(self.pos + 1)
                } else {
                    0
                };
                if next_ch == b'/' {
                    self.line_comment();
                } else if next_ch == b'*' {
                    self.block_comment();
                } else {
                    return ch;
                }
            } else if !is_br_or_ws(ch) {
                return ch;
            }
            if !self.advance_while_before_end() {
                break;
            }
        }
        // Reached when a comment runs to the end of the source: merve then
        // returns the `/` that opened it.
        ch
    }

    fn line_comment(&mut self) {
        while self.advance_while_before_end() {
            if is_br(self.cur()) {
                return;
            }
        }
    }

    fn block_comment(&mut self) {
        self.pos += 1;
        while self.advance_while_before_end() {
            if self.cur() == b'*' && self.byte(self.pos + 1) == b'/' {
                self.pos += 1;
                return;
            }
        }
    }

    fn string_literal(&mut self, quote: u8) {
        while self.advance_while_before_end() {
            let ch = self.cur();
            if ch == quote {
                return;
            }
            if ch == b'\\' {
                if self.pos + 1 >= self.end {
                    break;
                }
                self.pos += 1;
                if self.cur() == b'\r' && self.byte(self.pos + 1) == b'\n' {
                    self.pos += 1;
                }
            } else if is_br(ch) {
                break;
            }
        }
        self.syntax_error(LexError::UnterminatedStringLiteral);
    }

    fn regular_expression(&mut self) {
        while self.advance_while_before_end() {
            let ch = self.cur();
            if ch == b'/' {
                return;
            }
            if ch == b'[' {
                self.regex_character_class();
            } else if ch == b'\\' {
                if self.pos + 1 < self.end {
                    self.pos += 1;
                }
            } else if is_br(ch) {
                break;
            }
        }
        self.syntax_error(LexError::UnterminatedRegex);
    }

    fn regex_character_class(&mut self) {
        while self.advance_while_before_end() {
            let ch = self.cur();
            if ch == b']' {
                return;
            }
            if ch == b'\\' {
                if self.pos + 1 < self.end {
                    self.pos += 1;
                }
            } else if is_br(ch) {
                break;
            }
        }
        self.syntax_error(LexError::UnterminatedRegexCharacterClass);
    }

    fn template_string(&mut self) {
        while self.advance_while_before_end() {
            let ch = self.cur();
            if ch == b'$' && self.byte(self.pos + 1) == b'{' {
                self.pos += 1;
                if self.template_stack_depth >= STACK_DEPTH {
                    self.syntax_error(LexError::TemplateNestOverflow);
                    return;
                }
                let depth = self.template_stack_depth;
                stack_set(&mut self.template_stack, depth, self.template_depth, 0);
                self.template_stack_depth += 1;
                self.open_token_depth = self.open_token_depth.wrapping_add(1);
                self.template_depth = self.open_token_depth;
                return;
            }
            if ch == b'`' {
                return;
            }
            if ch == b'\\' && self.pos + 1 < self.end {
                self.pos += 1;
            }
        }
        self.syntax_error(LexError::UnterminatedTemplateString);
    }

    /// Consumes an identifier starting at `pos`, whose byte the caller
    /// passes as `start_ch`.
    fn identifier(&mut self, start_ch: u8) -> bool {
        if !is_identifier_start(start_ch) {
            return false;
        }
        self.pos += 1;
        while self.pos < self.end && is_identifier_char(self.cur()) {
            self.pos += 1;
        }
        true
    }

    /// Strips the quotes from a string literal's range; identifiers have
    /// none.
    fn unquote(name: &[u8]) -> &[u8] {
        match name.first() {
            Some(b'\'' | b'"') if name.len() >= 2 => &name[1..name.len() - 1],
            Some(b'\'' | b'"') => &[],
            _ => name,
        }
    }

    /// The name merve records for the range: unquoted, and unescaped when
    /// it holds a backslash. `None` when unescaping rejects it.
    fn export_name(&self, start: isize, end: isize) -> Option<Vec<u8>> {
        let name = Self::unquote(self.slice(start, end));
        if name.contains(&b'\\') {
            unescape_js_string(name)
        } else {
            Some(name.to_vec())
        }
    }

    fn add_export(&mut self, start: isize, end: isize) {
        if let Some(name) = self.export_name(start, end)
            && !self.exports.contains(&name)
        {
            self.exports.push(name);
        }
    }

    fn add_reexport(&mut self, start: isize, end: isize) {
        if let Some(name) = self.export_name(start, end) {
            self.re_exports.push(name);
        }
    }

    /// Consumes `exports` or `module.exports` starting at `pos`.
    fn read_exports_or_module_dot_exports(&mut self, mut ch: u8) -> bool {
        let revert_pos = self.pos;
        if ch == b'm' && self.matches_at(self.pos + 1, b"odule") {
            self.pos += 6;
            ch = self.comment_whitespace();
            if ch != b'.' {
                self.pos = revert_pos;
                return false;
            }
            self.pos += 1;
            ch = self.comment_whitespace();
        }
        if ch == b'e' && self.matches_at(self.pos + 1, b"xports") {
            self.pos += 7;
            return true;
        }
        self.pos = revert_pos;
        false
    }

    /// Parses `require('x')` with `pos` on the `r`, leaving `pos` on the
    /// closing paren when it succeeds.
    fn try_parse_require(&mut self, require_type: RequireType) -> bool {
        let revert_pos = self.pos;
        if !self.matches_at(self.pos + 1, b"equire") {
            return false;
        }
        self.pos += 7;
        let mut ch = self.comment_whitespace();
        if ch == b'(' {
            self.pos += 1;
            ch = self.comment_whitespace();
            let reexport_start = self.pos;
            if ch == b'\'' || ch == b'"' {
                self.string_literal(ch);
                self.pos += 1;
                let reexport_end = self.pos;
                ch = self.comment_whitespace();
                if ch == b')' {
                    match require_type {
                        RequireType::ExportStar | RequireType::ExportAssign => {
                            self.add_reexport(reexport_start, reexport_end);
                        }
                        RequireType::Import => {
                            if self.star_export_top < MAX_STAR_EXPORTS - 1 {
                                self.star_export_stack[self.star_export_top].specifier =
                                    (reexport_start, reexport_end);
                            }
                        }
                    }
                    return true;
                }
            }
        }
        self.pos = revert_pos;
        false
    }

    /// An object literal property's value: an identifier or `require(...)`.
    /// Leaves `ch` on the byte right after it, without skipping whitespace.
    fn try_parse_property_value(&mut self, ch: &mut u8) -> bool {
        if *ch == b'r' && self.try_parse_require(RequireType::ExportAssign) {
            *ch = self.cur();
            return true;
        }
        if self.identifier(*ch) {
            *ch = self.cur();
            return true;
        }
        false
    }

    /// `module.exports = { ... }` with `pos` on the `{`.
    fn try_parse_literal_exports(&mut self) {
        let revert_pos = self.pos - 1;
        while self.advance_while_before_end() {
            let mut ch = self.comment_whitespace();
            let start_pos = self.pos;
            if self.identifier(ch) {
                let end_pos = self.pos;
                ch = self.comment_whitespace();

                // A getter, `get name() {...}`, ends the literal.
                if ch != b':'
                    && end_pos - start_pos == 3
                    && self.matches_at(start_pos, b"get")
                    && self.identifier(ch)
                {
                    ch = self.comment_whitespace();
                    if ch == b'(' {
                        self.pos = revert_pos;
                        return;
                    }
                }

                if ch == b':' {
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if !self.try_parse_property_value(&mut ch) {
                        self.pos = revert_pos;
                        return;
                    }
                }
                self.add_export(start_pos, end_pos);
            } else if ch == b'\'' || ch == b'"' {
                let start = self.pos;
                self.string_literal(ch);
                self.pos += 1;
                let end_pos = self.pos;
                ch = self.comment_whitespace();
                if ch == b':' {
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if !self.try_parse_property_value(&mut ch) {
                        self.pos = revert_pos;
                        return;
                    }
                    self.add_export(start, end_pos);
                }
            } else if ch == b'.' && self.matches_at(self.pos + 1, b"..") {
                self.pos += 3;
                if self.pos < self.end
                    && self.cur() == b'r'
                    && self.try_parse_require(RequireType::ExportAssign)
                {
                    self.pos += 1;
                } else if self.pos < self.end && !self.identifier(self.cur()) {
                    self.pos = revert_pos;
                    return;
                }
                ch = self.comment_whitespace();
            } else {
                self.pos = revert_pos;
                return;
            }

            if ch == b'}' {
                return;
            }
            if ch != b',' {
                self.pos = revert_pos;
                return;
            }
        }
    }

    /// `exports.x =`, `exports['x'] =`, and with `assign`, `exports =`
    /// (reached from `module.exports =`). `pos` is on the `e`.
    fn try_parse_exports_dot_assign(&mut self, assign: bool) {
        self.pos += 7;
        let revert_pos = self.pos - 1;
        let mut ch = self.comment_whitespace();
        match ch {
            b'.' => {
                self.pos += 1;
                ch = self.comment_whitespace();
                let start_pos = self.pos;
                if self.identifier(ch) {
                    let end_pos = self.pos;
                    ch = self.comment_whitespace();
                    if ch == b'=' {
                        self.add_export(start_pos, end_pos);
                        return;
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                ch = self.comment_whitespace();
                if ch == b'\'' || ch == b'"' {
                    let start_pos = self.pos;
                    self.string_literal(ch);
                    self.pos += 1;
                    let end_pos = self.pos;
                    ch = self.comment_whitespace();
                    if ch == b']' {
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch == b'=' {
                            self.add_export(start_pos, end_pos);
                        }
                    }
                }
            }
            b'=' if assign => {
                // Assigning module.exports replaces whatever was reexported
                // before it.
                self.re_exports.clear();
                self.pos += 1;
                ch = self.comment_whitespace();
                if ch == b'{' {
                    self.try_parse_literal_exports();
                    return;
                }
                if ch == b'r' {
                    self.try_parse_require(RequireType::ExportAssign);
                }
            }
            _ => {}
        }
        self.pos = revert_pos;
    }

    /// `module.exports...` with `pos` on the `m`.
    fn try_parse_module_exports_dot_assign(&mut self) {
        self.pos += 6;
        let revert_pos = self.pos - 1;
        let mut ch = self.comment_whitespace();
        if ch == b'.' {
            self.pos += 1;
            ch = self.comment_whitespace();
            if ch == b'e' && self.matches_at(self.pos + 1, b"xports") {
                self.try_parse_exports_dot_assign(true);
                return;
            }
        }
        self.pos = revert_pos;
    }

    /// `Object.prototype.hasOwnProperty.call(x, it_id)`, or the same without
    /// `.prototype`.
    fn try_parse_object_has_own_property(&mut self, it_id: &[u8]) -> bool {
        let mut ch = self.comment_whitespace();
        if ch != b'O' || !self.matches_at(self.pos + 1, b"bject") {
            return false;
        }
        self.pos += 6;
        ch = self.comment_whitespace();
        if ch != b'.' {
            return false;
        }
        self.pos += 1;
        ch = self.comment_whitespace();
        if ch == b'p' {
            if !self.matches_at(self.pos + 1, b"rototype") {
                return false;
            }
            self.pos += 9;
            ch = self.comment_whitespace();
            if ch != b'.' {
                return false;
            }
            self.pos += 1;
            ch = self.comment_whitespace();
        }
        if ch != b'h' || !self.matches_at(self.pos + 1, b"asOwnProperty") {
            return false;
        }
        self.pos += 14;
        ch = self.comment_whitespace();
        if ch != b'.' {
            return false;
        }
        self.pos += 1;
        ch = self.comment_whitespace();
        if ch != b'c' || !self.matches_at(self.pos + 1, b"all") {
            return false;
        }
        self.pos += 4;
        ch = self.comment_whitespace();
        if ch != b'(' {
            return false;
        }
        self.pos += 1;
        ch = self.comment_whitespace();
        if !self.identifier(ch) {
            return false;
        }
        ch = self.comment_whitespace();
        if ch != b',' {
            return false;
        }
        self.pos += 1;
        self.comment_whitespace();
        if !self.eat_id(it_id) {
            return false;
        }
        ch = self.comment_whitespace();
        if ch != b')' {
            return false;
        }
        self.pos += 1;
        true
    }

    /// Consumes `id` exactly at `pos`, for the places the Babel loop
    /// repeats one of its identifiers.
    fn eat_id(&mut self, id: &[u8]) -> bool {
        if !self.matches_at(self.pos, id) {
            return false;
        }
        self.pos += id.len() as isize;
        true
    }

    /// Consumes a getter, `get: function name() { return` or the method
    /// form `get() { return`, through the `return`. `pos` is on the `g`.
    /// merve repeats this sequence in both places that use it.
    fn getter_to_return(&mut self) -> bool {
        if !self.matches_at(self.pos + 1, b"et") {
            return false;
        }
        self.pos += 3;
        let mut ch = self.comment_whitespace();
        if ch == b':' {
            self.pos += 1;
            ch = self.comment_whitespace();
            if ch != b'f' || !self.matches_at(self.pos + 1, b"unction") {
                return false;
            }
            self.pos += 8;
            let last_pos = self.pos;
            ch = self.comment_whitespace();
            if ch != b'(' && (last_pos == self.pos || !self.identifier(ch)) {
                return false;
            }
            ch = self.comment_whitespace();
        }
        if ch != b'(' {
            return false;
        }
        self.pos += 1;
        ch = self.comment_whitespace();
        if ch != b')' {
            return false;
        }
        self.pos += 1;
        ch = self.comment_whitespace();
        if ch != b'{' {
            return false;
        }
        self.pos += 1;
        ch = self.comment_whitespace();
        if ch != b'r' || !self.matches_at(self.pos + 1, b"eturn") {
            return false;
        }
        self.pos += 6;
        true
    }

    /// `Object.defineProperty(exports, 'x', ...)` and, when `keys`, Babel's
    /// `Object.keys(_x).forEach(function (key) { ... })` reexport loop.
    /// `pos` is on the `O`.
    fn try_parse_object_define_or_keys(&mut self, keys: bool) {
        self.pos += 6;
        let mut revert_pos = self.pos - 1;
        let mut ch = self.comment_whitespace();
        if ch == b'.' {
            self.pos += 1;
            ch = self.comment_whitespace();
            if ch == b'd' && self.matches_at(self.pos + 1, b"efineProperty") {
                // merve writes this as a `while (true)` that runs once and
                // leaves with `break` on the first mismatch.
                #[allow(clippy::never_loop)]
                loop {
                    self.pos += 14;
                    revert_pos = self.pos - 1;
                    ch = self.comment_whitespace();
                    if ch != b'(' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if !self.read_exports_or_module_dot_exports(ch) {
                        break;
                    }
                    ch = self.comment_whitespace();
                    if ch != b',' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch != b'\'' && ch != b'"' {
                        break;
                    }
                    let export_start = self.pos;
                    self.string_literal(ch);
                    self.pos += 1;
                    let export_end = self.pos;
                    ch = self.comment_whitespace();
                    if ch != b',' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch != b'{' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch == b'e' {
                        if !self.matches_at(self.pos + 1, b"numerable") {
                            break;
                        }
                        self.pos += 10;
                        ch = self.comment_whitespace();
                        if ch != b':' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b't' || !self.matches_at(self.pos + 1, b"rue") {
                            break;
                        }
                        self.pos += 4;
                        ch = self.comment_whitespace();
                        if ch != b',' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                    }
                    if ch == b'v' {
                        if !self.matches_at(self.pos + 1, b"alue") {
                            break;
                        }
                        self.pos += 5;
                        ch = self.comment_whitespace();
                        if ch != b':' {
                            break;
                        }
                        self.add_export(export_start, export_end);
                        self.pos = revert_pos;
                        return;
                    } else if ch == b'g' {
                        if !self.getter_to_return() {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if !self.identifier(ch) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch == b'.' {
                            self.pos += 1;
                            ch = self.comment_whitespace();
                            if !self.identifier(ch) {
                                break;
                            }
                            ch = self.comment_whitespace();
                        } else if ch == b'[' {
                            self.pos += 1;
                            ch = self.comment_whitespace();
                            if ch == b'\'' || ch == b'"' {
                                self.string_literal(ch);
                            } else {
                                break;
                            }
                            self.pos += 1;
                            ch = self.comment_whitespace();
                            if ch != b']' {
                                break;
                            }
                            self.pos += 1;
                            ch = self.comment_whitespace();
                        }
                        if ch == b';' {
                            self.pos += 1;
                            ch = self.comment_whitespace();
                        }
                        if ch != b'}' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch == b',' {
                            self.pos += 1;
                            ch = self.comment_whitespace();
                        }
                        if ch != b'}' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b')' {
                            break;
                        }
                        self.add_export(export_start, export_end);
                        return;
                    }
                    break;
                }
            } else if keys && ch == b'k' && self.matches_at(self.pos + 1, b"eys") {
                #[allow(clippy::never_loop)]
                loop {
                    self.pos += 4;
                    revert_pos = self.pos - 1;
                    ch = self.comment_whitespace();
                    if ch != b'(' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    let id_pos = self.pos;
                    if !self.identifier(ch) {
                        break;
                    }
                    let id = self.slice(id_pos, self.pos);
                    ch = self.comment_whitespace();
                    if ch != b')' {
                        break;
                    }

                    revert_pos = self.pos;
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch != b'.' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch != b'f' || !self.matches_at(self.pos + 1, b"orEach") {
                        break;
                    }
                    self.pos += 7;
                    ch = self.comment_whitespace();
                    revert_pos = self.pos - 1;
                    if ch != b'(' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch != b'f' || !self.matches_at(self.pos + 1, b"unction") {
                        break;
                    }
                    self.pos += 8;
                    ch = self.comment_whitespace();
                    if ch != b'(' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    let it_id_pos = self.pos;
                    if !self.identifier(ch) {
                        break;
                    }
                    let it_id = self.slice(it_id_pos, self.pos);
                    ch = self.comment_whitespace();
                    if ch != b')' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch != b'{' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch != b'i' || self.byte(self.pos + 1) != b'f' {
                        break;
                    }
                    self.pos += 2;
                    ch = self.comment_whitespace();
                    if ch != b'(' {
                        break;
                    }
                    self.pos += 1;
                    self.comment_whitespace();
                    if !self.eat_id(it_id) {
                        break;
                    }
                    ch = self.comment_whitespace();

                    if ch == b'=' {
                        // if (k === "default" || k === "__esModule") return;
                        if !self.matches_at(self.pos + 1, b"==") {
                            break;
                        }
                        self.pos += 3;
                        ch = self.comment_whitespace();
                        if ch != b'"' && ch != b'\'' {
                            break;
                        }
                        let mut quot = ch;
                        if !self.matches_at(self.pos + 1, b"default") {
                            break;
                        }
                        self.pos += 8;
                        ch = self.comment_whitespace();
                        if ch != quot {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b'|' || self.byte(self.pos + 1) != b'|' {
                            break;
                        }
                        self.pos += 2;
                        self.comment_whitespace();
                        if !self.eat_id(it_id) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch != b'=' || !self.matches_at(self.pos + 1, b"==") {
                            break;
                        }
                        self.pos += 3;
                        ch = self.comment_whitespace();
                        if ch != b'"' && ch != b'\'' {
                            break;
                        }
                        quot = ch;
                        if !self.matches_at(self.pos + 1, b"__esModule") {
                            break;
                        }
                        self.pos += 11;
                        ch = self.comment_whitespace();
                        if ch != quot {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b')' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b'r' || !self.matches_at(self.pos + 1, b"eturn") {
                            break;
                        }
                        self.pos += 6;
                        ch = self.comment_whitespace();
                        if ch == b';' {
                            self.pos += 1;
                        }
                        ch = self.comment_whitespace();

                        if ch == b'i' && self.byte(self.pos + 1) == b'f' {
                            let mut in_if = true;
                            self.pos += 2;
                            ch = self.comment_whitespace();
                            if ch != b'(' {
                                break;
                            }
                            self.pos += 1;
                            let if_inner_pos = self.pos;

                            if self.try_parse_object_has_own_property(it_id) {
                                ch = self.comment_whitespace();
                                if ch != b')' {
                                    break;
                                }
                                self.pos += 1;
                                ch = self.comment_whitespace();
                                if ch != b'r' || !self.matches_at(self.pos + 1, b"eturn") {
                                    break;
                                }
                                self.pos += 6;
                                ch = self.comment_whitespace();
                                if ch == b';' {
                                    self.pos += 1;
                                }
                                ch = self.comment_whitespace();
                                if ch == b'i' && self.byte(self.pos + 1) == b'f' {
                                    self.pos += 2;
                                    ch = self.comment_whitespace();
                                    if ch != b'(' {
                                        break;
                                    }
                                    self.pos += 1;
                                } else {
                                    in_if = false;
                                }
                            } else {
                                self.pos = if_inner_pos;
                            }

                            if in_if {
                                // if (k in exports && exports[k] === _x[k]) return;
                                if !self.eat_id(it_id) {
                                    break;
                                }
                                ch = self.comment_whitespace();
                                if ch != b'i' || !self.matches_at(self.pos + 1, b"n ") {
                                    break;
                                }
                                self.pos += 3;
                                ch = self.comment_whitespace();
                                if !self.read_exports_or_module_dot_exports(ch) {
                                    break;
                                }
                                ch = self.comment_whitespace();
                                if ch != b'&' || self.byte(self.pos + 1) != b'&' {
                                    break;
                                }
                                self.pos += 2;
                                ch = self.comment_whitespace();
                                if !self.read_exports_or_module_dot_exports(ch) {
                                    break;
                                }
                                ch = self.comment_whitespace();
                                if ch != b'[' {
                                    break;
                                }
                                self.pos += 1;
                                self.comment_whitespace();
                                if !self.eat_id(it_id) {
                                    break;
                                }
                                ch = self.comment_whitespace();
                                if ch != b']' {
                                    break;
                                }
                                self.pos += 1;
                                ch = self.comment_whitespace();
                                if ch != b'=' || !self.matches_at(self.pos + 1, b"==") {
                                    break;
                                }
                                self.pos += 3;
                                self.comment_whitespace();
                                if !self.eat_id(id) {
                                    break;
                                }
                                ch = self.comment_whitespace();
                                if ch != b'[' {
                                    break;
                                }
                                self.pos += 1;
                                self.comment_whitespace();
                                if !self.eat_id(it_id) {
                                    break;
                                }
                                ch = self.comment_whitespace();
                                if ch != b']' {
                                    break;
                                }
                                self.pos += 1;
                                ch = self.comment_whitespace();
                                if ch != b')' {
                                    break;
                                }
                                self.pos += 1;
                                ch = self.comment_whitespace();
                                if ch != b'r' || !self.matches_at(self.pos + 1, b"eturn") {
                                    break;
                                }
                                self.pos += 6;
                                ch = self.comment_whitespace();
                                if ch == b';' {
                                    self.pos += 1;
                                }
                                ch = self.comment_whitespace();
                            }
                        }
                    } else if ch == b'!' {
                        // if (k !== "default" && !Object.prototype.hasOwnProperty.call(...))
                        if !self.matches_at(self.pos + 1, b"==") {
                            break;
                        }
                        self.pos += 3;
                        ch = self.comment_whitespace();
                        if ch != b'"' && ch != b'\'' {
                            break;
                        }
                        let quot = ch;
                        if !self.matches_at(self.pos + 1, b"default") {
                            break;
                        }
                        self.pos += 8;
                        ch = self.comment_whitespace();
                        if ch != quot {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch == b'&' {
                            if self.byte(self.pos + 1) != b'&' {
                                break;
                            }
                            self.pos += 2;
                            ch = self.comment_whitespace();
                            if ch != b'!' {
                                break;
                            }
                            self.pos += 1;
                            ch = self.comment_whitespace();
                            if ch == b'O' && self.matches_at(self.pos + 1, b"bject.") {
                                if !self.try_parse_object_has_own_property(it_id) {
                                    break;
                                }
                            } else if self.identifier(ch) {
                                ch = self.comment_whitespace();
                                if ch != b'.' {
                                    break;
                                }
                                self.pos += 1;
                                ch = self.comment_whitespace();
                                if ch != b'h' || !self.matches_at(self.pos + 1, b"asOwnProperty") {
                                    break;
                                }
                                self.pos += 14;
                                ch = self.comment_whitespace();
                                if ch != b'(' {
                                    break;
                                }
                                self.pos += 1;
                                self.comment_whitespace();
                                if !self.eat_id(it_id) {
                                    break;
                                }
                                ch = self.comment_whitespace();
                                if ch != b')' {
                                    break;
                                }
                                self.pos += 1;
                            }
                            ch = self.comment_whitespace();
                        }
                        if ch != b')' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                    } else {
                        break;
                    }

                    if self.read_exports_or_module_dot_exports(ch) {
                        // exports[k] = _x[k];
                        ch = self.comment_whitespace();
                        if ch != b'[' {
                            break;
                        }
                        self.pos += 1;
                        self.comment_whitespace();
                        if !self.eat_id(it_id) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch != b']' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b'=' {
                            break;
                        }
                        self.pos += 1;
                        self.comment_whitespace();
                        if !self.eat_id(id) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch != b'[' {
                            break;
                        }
                        self.pos += 1;
                        self.comment_whitespace();
                        if !self.eat_id(it_id) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch != b']' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch == b';' {
                            self.pos += 1;
                            ch = self.comment_whitespace();
                        }
                    } else if ch == b'O' {
                        // Object.defineProperty(exports, k, { enumerable: true, get: ... });
                        if !self.matches_at(self.pos + 1, b"bject") {
                            break;
                        }
                        self.pos += 6;
                        ch = self.comment_whitespace();
                        if ch != b'.' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b'd' || !self.matches_at(self.pos + 1, b"efineProperty") {
                            break;
                        }
                        self.pos += 14;
                        ch = self.comment_whitespace();
                        if ch != b'(' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if !self.read_exports_or_module_dot_exports(ch) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch != b',' {
                            break;
                        }
                        self.pos += 1;
                        self.comment_whitespace();
                        if !self.eat_id(it_id) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch != b',' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b'{' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b'e' || !self.matches_at(self.pos + 1, b"numerable") {
                            break;
                        }
                        self.pos += 10;
                        ch = self.comment_whitespace();
                        if ch != b':' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b't' || !self.matches_at(self.pos + 1, b"rue") {
                            break;
                        }
                        self.pos += 4;
                        ch = self.comment_whitespace();
                        if ch != b',' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b'g' || !self.getter_to_return() {
                            break;
                        }
                        self.comment_whitespace();
                        if !self.eat_id(id) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch != b'[' {
                            break;
                        }
                        self.pos += 1;
                        self.comment_whitespace();
                        if !self.eat_id(it_id) {
                            break;
                        }
                        ch = self.comment_whitespace();
                        if ch != b']' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch == b';' {
                            self.pos += 1;
                            ch = self.comment_whitespace();
                        }
                        if ch != b'}' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch == b',' {
                            self.pos += 1;
                            ch = self.comment_whitespace();
                        }
                        if ch != b'}' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch != b')' {
                            break;
                        }
                        self.pos += 1;
                        ch = self.comment_whitespace();
                        if ch == b';' {
                            self.pos += 1;
                            ch = self.comment_whitespace();
                        }
                    } else {
                        break;
                    }

                    if ch != b'}' {
                        break;
                    }
                    self.pos += 1;
                    ch = self.comment_whitespace();
                    if ch != b')' {
                        break;
                    }

                    // The loop is a reexport only when its object came from
                    // an earlier `var id = require('x')`.
                    for i in 0..self.star_export_top {
                        let binding = self.star_export_stack[i];
                        if self.slice(binding.id.0, binding.id.1) == id {
                            self.add_reexport(binding.specifier.0, binding.specifier.1);
                            self.pos = revert_pos;
                            return;
                        }
                    }
                    return;
                }
            }
        }
        self.pos = revert_pos;
    }

    /// After a `require(...)` that starts a token, looks back from `b_pos`
    /// for `var|let|const id =` and records `id` as bound to the specifier
    /// `try_parse_require` just stored.
    fn try_backtrack_add_star_export_binding(&mut self, mut b_pos: isize) {
        if b_pos < 0 {
            return;
        }
        while b_pos > 0 && self.byte(b_pos) == b' ' {
            b_pos -= 1;
        }
        if self.byte(b_pos) != b'=' {
            return;
        }
        if b_pos <= 0 {
            return;
        }
        b_pos -= 1;
        while b_pos > 0 && self.byte(b_pos) == b' ' {
            b_pos -= 1;
        }
        let id_end = b_pos;
        let mut identifier_start = false;
        while b_pos > 0 {
            let ch = self.byte(b_pos);
            if !is_identifier_char(ch) {
                break;
            }
            identifier_start = is_identifier_start(ch);
            b_pos -= 1;
        }
        if identifier_start && self.byte(b_pos) == b' ' {
            if self.star_export_top == MAX_STAR_EXPORTS - 1 {
                return;
            }
            self.star_export_stack[self.star_export_top].id = (b_pos + 1, id_end + 1);
            while b_pos > 0 && self.byte(b_pos) == b' ' {
                b_pos -= 1;
            }
            match self.byte(b_pos) {
                b'r' => {
                    if !self.read_preceding_keyword(b_pos - 1, b"va") {
                        return;
                    }
                }
                b't' => {
                    if !self.read_preceding_keyword(b_pos - 1, b"le")
                        && !self.read_preceding_keyword(b_pos - 1, b"cons")
                    {
                        return;
                    }
                }
                _ => return,
            }
            self.star_export_top += 1;
        }
    }

    /// `import` at the top level: dynamic `import(...)` is fine, while
    /// `import.meta` and import declarations mean the file is ESM.
    fn throw_if_import_statement(&mut self) {
        let start_pos = self.pos;
        self.pos += 6;
        let mut ch = self.comment_whitespace();
        match ch {
            b'(' => {
                self.push_open_token(b'(', start_pos);
            }
            b'.' => {
                self.pos += 1;
                ch = self.comment_whitespace();
                if ch == b'm' && self.pos + 4 <= self.end && self.matches_at(self.pos + 1, b"eta") {
                    // `import.metaData` is some other property.
                    if self.pos + 4 < self.end && is_identifier_char(self.byte(self.pos + 4)) {
                        return;
                    }
                    self.syntax_error(LexError::UnexpectedEsmImportMeta);
                }
            }
            _ => {
                let declaration_start = matches!(ch, b'"' | b'\'' | b'{' | b'*');
                // `import` glued to the next token, as in `importFoo`, is an
                // identifier.
                if !declaration_start && self.pos == start_pos + 6 {
                    return;
                }
                if self.open_token_depth != 0 {
                    self.pos -= 1;
                    return;
                }
                self.syntax_error(LexError::UnexpectedEsmImport);
            }
        }
    }

    fn throw_if_export_statement(&mut self) {
        self.pos += 6;
        let cur_pos = self.pos;
        let ch = self.comment_whitespace();
        if self.pos == cur_pos && !is_punctuator(ch) {
            return;
        }
        self.syntax_error(LexError::UnexpectedEsmExport);
    }

    fn parse(&mut self) -> Result<(), LexError> {
        let len = self.source.len();

        if len >= 2 && self.source[0] == b'#' && self.source[1] == b'!' {
            if len == 2 {
                return Ok(());
            }
            self.pos += 2;
            while self.pos < self.end && !is_br(self.cur()) {
                self.pos += 1;
            }
            self.last_token_pos = self.pos;
        }

        while self.advance_while_before_end() {
            let ch = self.cur();

            if ch == b' ' || (9..14).contains(&ch) {
                continue;
            }

            if self.open_token_depth == 0 {
                match ch {
                    b'i' => {
                        if self.pos + 6 < self.end
                            && self.matches_at(self.pos + 1, b"mport")
                            && self.keyword_start(self.pos)
                        {
                            self.throw_if_import_statement();
                        }
                        self.last_token_pos = self.pos;
                        continue;
                    }
                    b'r' => {
                        let start_pos = self.pos;
                        if self.try_parse_require(RequireType::Import)
                            && self.keyword_start(start_pos)
                        {
                            self.try_backtrack_add_star_export_binding(start_pos - 1);
                        }
                        self.last_token_pos = self.pos;
                        continue;
                    }
                    b'_' => {
                        let dot_before = self.byte(self.pos - 1) == b'.';
                        if self.pos + 23 < self.end
                            && self.matches_at(self.pos + 1, b"interopRequireWildcard")
                            && (self.keyword_start(self.pos) || dot_before)
                        {
                            let start_pos = self.pos;
                            self.pos += 23;
                            if self.cur() == b'(' {
                                self.pos += 1;
                                self.push_open_token(b'(', self.last_token_pos);
                                if self.try_parse_require(RequireType::Import)
                                    && self.keyword_start(start_pos)
                                {
                                    self.try_backtrack_add_star_export_binding(start_pos - 1);
                                }
                            }
                        } else if self.pos + 8 < self.end
                            && self.matches_at(self.pos + 1, b"_export")
                            && (self.keyword_start(self.pos) || dot_before)
                        {
                            self.pos += 8;
                            if self.pos + 4 < self.end && self.matches_at(self.pos, b"Star") {
                                self.pos += 4;
                            }
                            if self.cur() == b'(' {
                                self.push_open_token(b'(', self.last_token_pos);
                                if self.byte(self.pos + 1) == b'r' {
                                    self.pos += 1;
                                    self.try_parse_require(RequireType::ExportStar);
                                }
                            }
                        }
                        self.last_token_pos = self.pos;
                        continue;
                    }
                    _ => {}
                }
            }

            match ch {
                b'e' => {
                    if self.pos + 6 < self.end
                        && self.matches_at(self.pos + 1, b"xport")
                        && self.keyword_start(self.pos)
                    {
                        if self.pos + 7 < self.end && self.byte(self.pos + 6) == b's' {
                            self.try_parse_exports_dot_assign(false);
                        } else if self.open_token_depth == 0 {
                            self.throw_if_export_statement();
                        }
                    }
                }
                b'c' => {
                    if self.keyword_start(self.pos)
                        && self.matches_at(self.pos + 1, b"lass")
                        && is_br_or_ws(self.byte(self.pos + 5))
                    {
                        self.next_brace_is_class = true;
                    }
                }
                b'm' => {
                    if self.pos + 6 < self.end
                        && self.matches_at(self.pos + 1, b"odule")
                        && self.keyword_start(self.pos)
                    {
                        self.try_parse_module_exports_dot_assign();
                    }
                }
                b'O' => {
                    if self.pos + 6 < self.end
                        && self.matches_at(self.pos + 1, b"bject")
                        && self.keyword_start(self.pos)
                    {
                        self.try_parse_object_define_or_keys(self.open_token_depth == 0);
                    }
                }
                b'(' => {
                    self.push_open_token(b'(', self.last_token_pos);
                }
                b')' => {
                    if self.open_token_depth == 0 {
                        self.syntax_error(LexError::UnexpectedParen);
                        return Err(LexError::UnexpectedParen);
                    }
                    self.open_token_depth -= 1;
                }
                b'{' => {
                    let depth = self.open_token_depth;
                    stack_set(
                        &mut self.open_class_pos_stack,
                        depth,
                        self.next_brace_is_class,
                        false,
                    );
                    self.next_brace_is_class = false;
                    self.push_open_token(b'{', self.last_token_pos);
                }
                b'}' => {
                    if self.open_token_depth == 0 {
                        self.syntax_error(LexError::UnexpectedBrace);
                        return Err(LexError::UnexpectedBrace);
                    }
                    let closed = self.open_token_depth;
                    self.open_token_depth -= 1;
                    if closed == self.template_depth {
                        // This brace closes a `${`, so the template resumes.
                        self.template_stack_depth = self.template_stack_depth.wrapping_sub(1);
                        self.template_depth =
                            stack_get(&self.template_stack, self.template_stack_depth, 0);
                        self.template_string();
                    } else if self.template_depth != NO_TEMPLATE
                        && self.open_token_depth < self.template_depth
                    {
                        self.syntax_error(LexError::UnterminatedTemplateString);
                        return Err(LexError::UnterminatedTemplateString);
                    }
                }
                b'\'' | b'"' => self.string_literal(ch),
                b'/' => {
                    let next_ch = if self.pos + 1 < self.end {
                        self.byte(self.pos + 1)
                    } else {
                        0
                    };
                    if next_ch == b'/' {
                        self.line_comment();
                        continue;
                    } else if next_ch == b'*' {
                        self.block_comment();
                        continue;
                    }
                    if self.slash_starts_regex() {
                        self.regular_expression();
                        self.last_slash_was_division = false;
                    } else {
                        self.last_slash_was_division = true;
                    }
                }
                b'`' => {
                    if self.template_depth == NO_TEMPLATE - 1 {
                        self.syntax_error(LexError::TemplateNestOverflow);
                        return Err(LexError::TemplateNestOverflow);
                    }
                    self.template_string();
                }
                _ => {}
            }
            self.last_token_pos = self.pos;
        }

        if self.last_error.is_none() {
            if self.template_depth != NO_TEMPLATE {
                self.syntax_error(LexError::UnterminatedTemplateString);
            } else if self.open_token_depth != 0 {
                let open = stack_get(&self.open_token_type_stack, self.open_token_depth - 1, 0);
                self.syntax_error(if open == b'{' {
                    LexError::UnterminatedBrace
                } else {
                    LexError::UnterminatedParen
                });
            }
        }
        match self.last_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Whether a `/` that does not start a comment starts a regular
    /// expression, judged from the token before it.
    fn slash_starts_regex(&self) -> bool {
        let is_start_of_input = self.last_token_pos < 0;
        let last_token = if is_start_of_input {
            0
        } else {
            self.byte(self.last_token_pos)
        };
        let has_before = self.last_token_pos > 0;
        let before_last = self.byte(self.last_token_pos - 1);
        let open_pos = stack_get(&self.open_token_pos_stack, self.open_token_depth, NULL_POS);

        (is_expression_punctuator(last_token)
            && !(last_token == b'.' && has_before && before_last.is_ascii_digit())
            && !(last_token == b'+' && has_before && before_last == b'+')
            && !(last_token == b'-' && has_before && before_last == b'-'))
            || (last_token == b')' && self.is_paren_keyword(open_pos))
            || (last_token == b'}'
                && (open_pos < 0
                    || self.is_expression_terminator(open_pos)
                    || stack_get(&self.open_class_pos_stack, self.open_token_depth, false)))
            || (last_token == b'/' && self.last_slash_was_division)
            || (!is_start_of_input && self.is_expression_keyword(self.last_token_pos))
            || last_token == 0
            || is_start_of_input
    }
}

// Every expectation below is what Node v24.19.0's
// `internalBinding('cjs_lexer').parse` returned for the same source.
#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn check(source: &str, names: &[&str], reexports: &[&str]) {
        let (got_names, got_reexports) = exports(source);
        assert_eq!(got_names, names, "exports of {source:?}");
        assert_eq!(got_reexports, reexports, "reexports of {source:?}");
    }

    #[test]
    fn test_dot_assignments_are_exports() {
        check(
            "exports.a = 1; exports.b = 2; module.exports.c = 3;",
            &["a", "b", "c"],
            &[],
        );
        check(
            "exports.ü = 1; exports.$x = 2; exports._y = 3;",
            &["ü", "$x", "_y"],
            &[],
        );
        check(
            "exports.default = 1; exports.__esModule = true;",
            &["default", "__esModule"],
            &[],
        );
    }

    #[test]
    fn test_repeated_names_keep_their_first_position() {
        check(
            "exports.a = 1; exports.b = 2; exports.a = 3;",
            &["a", "b"],
            &[],
        );
        check(
            r"exports.a = 1; exports.a = 2; exports['a'] = 3; exports['\x61'] = 4;",
            &["a"],
            &[],
        );
    }

    #[test]
    fn test_string_keys_are_exports() {
        check(
            r#"exports['a-b'] = 1; exports["c d"] = 2; exports['e'] = 3;"#,
            &["a-b", "c d", "e"],
            &[],
        );
    }

    #[test]
    fn test_escapes_in_string_keys_follow_merve() {
        // Lone surrogates, braced or not, drop the name; a line continuation
        // keeps its newline and an unknown escape keeps its character.
        check(
            "exports['\\u0061b'] = 1; exports['\\u{1F600}'] = 2; exports['\\ud83d\\ude00x'] = 3; \
             exports['\\ud800'] = 4; exports['\\udc00'] = 5; exports['\\u{d83d}\\u{de00}'] = 6; \
             exports['\\x41'] = 7; exports['a\\\nb'] = 8; exports['\\q'] = 9",
            &["ab", "😀", "😀x", "A", "a\nb", "q"],
            &[],
        );
    }

    #[test]
    fn test_raw_bytes_from_hex_escapes_merge_once_replaced() {
        // `\xe9` and `\xe8` are distinct bytes to merve but both become
        // U+FFFD in the JS string, and the Set keeps one.
        check(
            r"exports['\xe9'] = 1; exports['\xe8'] = 2; exports.c = 1",
            &["\u{FFFD}", "c"],
            &[],
        );
        check(r"exports['a\ud800'] = 1; exports['😀'] = 2", &["😀"], &[]);
    }

    #[test]
    fn test_object_literal_assignment() {
        check(
            "module.exports = { a, b: c, 'd-e': f, ...require('x'), g };",
            &["a", "b", "d-e", "g"],
            &["x"],
        );
        check(
            "module.exports = {\n  // comment\n  a,\n  /* c */ b,\n}",
            &["a", "b"],
            &[],
        );
        check("module.exports = { ...a, b }", &["b"], &[]);
    }

    #[test]
    fn test_object_literal_stops_where_merve_stops() {
        // merve reads the byte right after a property value without
        // skipping whitespace, and after `require(...)` that byte is `)`,
        // so the literal ends early in both cases. A getter ends it too.
        check("module.exports = { a: require('x'), b };", &["a"], &["x"]);
        check("module.exports = { a: b , c };", &["a"], &[]);
        check(
            "module.exports = { a, get b() { return 1 }, c };",
            &["a"],
            &[],
        );
    }

    #[test]
    fn test_define_property_forms() {
        check(
            "Object.defineProperty(exports, 'a', { value: 1 });",
            &["a"],
            &[],
        );
        check(
            "Object.defineProperty(exports, '__esModule', { value: true });",
            &["__esModule"],
            &[],
        );
        check(
            "Object.defineProperty(exports, 'b', { enumerable: true, get: function () { return m.b; } });",
            &["b"],
            &[],
        );
        check(
            "Object.defineProperty(exports, 'c', { enumerable: true, get() { return m['c']; } });",
            &["c"],
            &[],
        );
        check(
            "Object.defineProperty(module.exports, 'd', { get: function get() { return d; } });",
            &["d"],
            &[],
        );
        // A getter that computes its value is not a static export.
        check(
            "Object.defineProperty(exports, 'e', { get: function () { return compute(); } });",
            &[],
            &[],
        );
    }

    #[test]
    fn test_typescript_reexport_helpers() {
        check(
            "\"use strict\";\nvar __exportStar = (this && this.__exportStar) || function(m, exports) {};\n\
             Object.defineProperty(exports, \"__esModule\", { value: true });\n\
             __exportStar(require(\"./a\"), exports);\ntslib_1.__exportStar(require(\"./b\"), exports);",
            &["__esModule"],
            &["./a", "./b"],
        );
        check(
            "function __export(m) { for (var p in m) if (!exports.hasOwnProperty(p)) exports[p] = m[p]; }\n\
             __export(require('./c'));",
            &[],
            &["./c"],
        );
        // Reexports are not deduplicated.
        check(
            "__exportStar(require('x'), exports); __exportStar(require('x'), exports);",
            &[],
            &["x", "x"],
        );
    }

    #[test]
    fn test_babel_object_keys_reexport() {
        check(
            "\"use strict\";\nvar _x = require(\"./x\");\nObject.keys(_x).forEach(function (key) {\n  \
             if (key === \"default\" || key === \"__esModule\") return;\n  \
             if (key in exports && exports[key] === _x[key]) return;\n  \
             Object.defineProperty(exports, key, {\n    enumerable: true,\n    get: function () {\n      \
             return _x[key];\n    }\n  });\n});",
            &[],
            &["./x"],
        );
        check(
            "var _y = require('./y');\nObject.keys(_y).forEach(function (k) {\n  \
             if (k !== 'default' && !Object.prototype.hasOwnProperty.call(exports, k)) exports[k] = _y[k];\n});",
            &[],
            &["./y"],
        );
        check(
            "var e = _interopRequireWildcard(require('./w'));\nObject.keys(e).forEach(function (key) { \
             if (key === 'default' || key === '__esModule') return; exports[key] = e[key]; });",
            &[],
            &["./w"],
        );
        // The loop's object must be bound to a require.
        check(
            "var _z = require('./z');\nObject.keys(_q).forEach(function (k) { if (k !== 'default') exports[k] = _q[k]; });",
            &[],
            &[],
        );
    }

    #[test]
    fn test_module_exports_require_replaces_earlier_reexports() {
        check("module.exports = require('./main');", &[], &["./main"]);
        check(
            "module.exports = require('x'); module.exports = require('x');",
            &[],
            &["x"],
        );
        check(
            "__exportStar(require('a'), exports); module.exports = require('b');",
            &[],
            &["b"],
        );
        check(
            "module.exports = require('b'); __exportStar(require('a'), exports);",
            &[],
            &["b", "a"],
        );
    }

    #[test]
    fn test_templates_regexes_and_comments() {
        check(
            "exports.a = 1;\nconst x = `${exports.b = 1}`;",
            &["a", "b"],
            &[],
        );
        check(
            "exports.a = 1; var r = /exports.c = 1/; var d = a / exports.b; ",
            &["a"],
            &[],
        );
        check("if (x) /foo/.test(y); exports.ok = 1", &["ok"], &[]);
        check(
            "class A { m() {} } /re/.test(); exports.after = 1",
            &["after"],
            &[],
        );
        check("exports.x = 1 // trailing comment", &["x"], &[]);
        // merve does not reject an unterminated block comment.
        check("exports.x = 1 /* unterminated block", &["x"], &[]);
    }

    #[test]
    fn test_dynamic_import_and_shebang_are_fine() {
        check("exports.a = 1; import('x');", &["a"], &[]);
        check("#!/usr/bin/env node\nexports.a=1", &["a"], &[]);
        check("#!", &[], &[]);
        check("", &[], &[]);
    }

    #[test]
    fn test_failure_returns_nothing() {
        // Node's binding returns an empty Set and array when merve fails,
        // throwing away whatever it found before the failure.
        check("export const x = 1", &[], &[]);
        check("exports.a = 1; export default 1;", &[], &[]);
        check("exports.a = 1; import x from 'y';", &[], &[]);
        check("exports.a = 1; import.meta", &[], &[]);
        check("exports.a = 'unterminated", &[], &[]);
        check("exports.a = 1; foo(", &[], &[]);
        check("exports.a = 1; }", &[], &[]);
        check("exports.a = 1; )", &[], &[]);
        check("exports.a=1; `${", &[], &[]);
        // merve skips the byte after `_interopRequireWildcard(` when no
        // require follows, here the `)`, so the paren never closes.
        check("exports.a = 1; _interopRequireWildcard()", &[], &[]);
    }

    // Node does not expose why merve failed, only that it did (each of
    // these sources after `exports.a=1;` gives Node an empty result), so the
    // kinds here come from reading merve.
    #[test]
    fn test_failure_kinds() {
        let error = |source: &str| parse_commonjs(source.as_bytes()).err();
        assert_eq!(
            error("export const x = 1"),
            Some(LexError::UnexpectedEsmExport)
        );
        assert_eq!(
            error("import x from 'y'"),
            Some(LexError::UnexpectedEsmImport)
        );
        assert_eq!(
            error("import.meta.url"),
            Some(LexError::UnexpectedEsmImportMeta)
        );
        assert_eq!(error("'abc"), Some(LexError::UnterminatedStringLiteral));
        assert_eq!(error("f("), Some(LexError::UnterminatedParen));
        assert_eq!(error("{"), Some(LexError::UnterminatedBrace));
        assert_eq!(error("`abc"), Some(LexError::UnterminatedTemplateString));
        assert_eq!(error("x = /abc"), Some(LexError::UnterminatedRegex));
        assert_eq!(
            error("x = /[abc/"),
            Some(LexError::UnterminatedRegexCharacterClass)
        );
        assert_eq!(error(")"), Some(LexError::UnexpectedParen));
        assert_eq!(error("}"), Some(LexError::UnexpectedBrace));
        assert_eq!(error("exports.a = 1"), None);
    }
}
