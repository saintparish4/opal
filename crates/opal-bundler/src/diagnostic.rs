//! Refusals: the codes `SEMANTICS.md` lists, and where in a module each one
//! was found.

use std::fmt;

/// A refusal code from `SEMANTICS.md`'s table. The `Display` form is the
/// code as written there, which is what the CLI prints and what the test262
/// runner and the fixtures match on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    Parse,
    Link,
    Tla,
    RequireEsm,
    Dynamic,
    ImportMeta,
    Eval,
    CjsApi,
    Builtin,
    BrowserField,
    Ts,
    Define,
    Unsupported,
    /// Not a refusal of the program: the build itself could not run (a file
    /// vanished, the out-dir is not writable).
    Io,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Parse => "E-PARSE",
            Code::Link => "E-LINK",
            Code::Tla => "E-TLA",
            Code::RequireEsm => "E-REQUIRE-ESM",
            Code::Dynamic => "E-DYNAMIC",
            Code::ImportMeta => "E-IMPORT-META",
            Code::Eval => "E-EVAL",
            Code::CjsApi => "E-CJS-API",
            Code::Builtin => "E-BUILTIN",
            Code::BrowserField => "E-BROWSER-FIELD",
            Code::Ts => "E-TS",
            Code::Define => "E-DEFINE",
            Code::Unsupported => "E-UNSUPPORTED",
            Code::Io => "E-IO",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One refusal, located in a module.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    pub code: Code,
    /// Root-relative path of the module, or empty for a build-wide problem.
    pub module: String,
    /// 1-based; 0 when the diagnostic is about the whole module.
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: Code, module: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            module: module.into(),
            line: 0,
            column: 0,
            message: message.into(),
        }
    }

    pub fn at(mut self, location: (u32, u32)) -> Self {
        self.line = location.0;
        self.column = location.1;
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        if !self.module.is_empty() {
            write!(f, ": {}", self.module)?;
            if self.line > 0 {
                write!(f, ":{}:{}", self.line, self.column)?;
            }
        }
        write!(f, ": {}", self.message)
    }
}

/// Byte offsets to 1-based line and column (in UTF-16 code units, which is
/// what editors and source maps count).
pub struct LineIndex {
    starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(offset as u32 + 1);
            }
        }
        Self { starts }
    }

    pub fn locate(&self, text: &str, offset: u32) -> (u32, u32) {
        let line = match self.starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index - 1,
        };
        let start = self.starts[line] as usize;
        let end = (offset as usize).min(text.len());
        let column = text
            .get(start..end)
            .map_or(0, |prefix| prefix.encode_utf16().count());
        (line as u32 + 1, column as u32 + 1)
    }
}
