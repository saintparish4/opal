//! The `compilerOptions` that change what TypeScript and JSX compile to.
//!
//! `SEMANTICS.md` rule 13 lists them. Each file takes the `tsconfig.json`
//! nearest it, as `tsc` and every bundler do; `extends` is followed, and a
//! solution-style config (`"files": []` plus `references`) hands the file to
//! the referenced project whose `include` covers it, which is how the
//! TypeScript language service picks a project for a file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// How JSX compiles.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Jsx {
    /// `react-jsx`: calls into `<import_source>/jsx-runtime`.
    Automatic { import_source: String },
    /// `react`: calls `factory` and `fragment`, which must be in scope.
    Classic { factory: String, fragment: String },
    /// `preserve` and `react-native` leave JSX in the output, which no
    /// runtime runs.
    Preserve,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TsSettings {
    pub verbatim_module_syntax: bool,
    pub experimental_decorators: bool,
    pub emit_decorator_metadata: bool,
    pub use_define_for_class_fields: bool,
    pub strict_null_checks: bool,
    pub jsx: Jsx,
    /// The config file these came from, root-relative, for diagnostics.
    pub source: Option<String>,
}

impl TsSettings {
    pub fn defaults(jsx_import_source: &str) -> Self {
        Self {
            verbatim_module_syntax: false,
            experimental_decorators: false,
            emit_decorator_metadata: false,
            // tsc's default for a target of ES2022 or later, and with no
            // target its default target is ES5... but no runtime this
            // bundler targets runs ES5 semantics for fields, and every config
            // that matters names a target. With none, follow the language.
            use_define_for_class_fields: true,
            strict_null_checks: false,
            jsx: Jsx::Automatic {
                import_source: jsx_import_source.to_string(),
            },
            source: None,
        }
    }
}

/// Finds and caches the settings for each directory.
pub struct TsConfigs {
    root: PathBuf,
    jsx_import_source: String,
    by_dir: HashMap<PathBuf, Option<PathBuf>>,
    parsed: HashMap<PathBuf, Option<Value>>,
}

impl TsConfigs {
    pub fn new(root: &Path, jsx_import_source: &str) -> Self {
        Self {
            root: root.to_path_buf(),
            jsx_import_source: jsx_import_source.to_string(),
            by_dir: HashMap::new(),
            parsed: HashMap::new(),
        }
    }

    /// The settings for `file` (absolute).
    pub fn settings_for(&mut self, file: &Path) -> Result<TsSettings, String> {
        let dir = file.parent().unwrap_or(&self.root).to_path_buf();
        let Some(config) = self.nearest(&dir) else {
            return Ok(TsSettings::defaults(&self.jsx_import_source));
        };
        let config = self.pick_project(&config, file)?;
        let options = self.compiler_options(&config, &mut Vec::new())?;
        let mut settings = TsSettings::defaults(&self.jsx_import_source);
        settings.source = Some(
            config
                .strip_prefix(&self.root)
                .unwrap_or(&config)
                .to_string_lossy()
                .replace('\\', "/"),
        );
        let flag = |key: &str| options.get(key).and_then(Value::as_bool);
        settings.verbatim_module_syntax = flag("verbatimModuleSyntax").unwrap_or(false);
        settings.experimental_decorators = flag("experimentalDecorators").unwrap_or(false);
        settings.emit_decorator_metadata = flag("emitDecoratorMetadata").unwrap_or(false);
        let strict = flag("strict").unwrap_or(false);
        settings.strict_null_checks = flag("strictNullChecks").unwrap_or(strict);
        settings.use_define_for_class_fields = match flag("useDefineForClassFields") {
            Some(value) => value,
            // tsc: true when target is ES2022 or later (or ESNext).
            None => options
                .get("target")
                .and_then(Value::as_str)
                .is_none_or(target_defines_class_fields),
        };
        let string = |key: &str| options.get(key).and_then(Value::as_str).map(str::to_string);
        settings.jsx = match string("jsx")
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            None | Some("react-jsx") | Some("react-jsxdev") => Jsx::Automatic {
                import_source: string("jsxImportSource")
                    .unwrap_or_else(|| self.jsx_import_source.clone()),
            },
            Some("react") => Jsx::Classic {
                factory: string("jsxFactory").unwrap_or_else(|| "React.createElement".into()),
                fragment: string("jsxFragmentFactory").unwrap_or_else(|| "React.Fragment".into()),
            },
            Some(_) => Jsx::Preserve,
        };
        Ok(settings)
    }

    fn nearest(&mut self, dir: &Path) -> Option<PathBuf> {
        if let Some(found) = self.by_dir.get(dir) {
            return found.clone();
        }
        let candidate = dir.join("tsconfig.json");
        let found = if candidate.is_file() {
            Some(candidate)
        } else if dir == self.root || !dir.starts_with(&self.root) {
            None
        } else {
            dir.parent().and_then(|parent| self.nearest(parent))
        };
        self.by_dir.insert(dir.to_path_buf(), found.clone());
        found
    }

    /// For a solution-style config, the referenced project that includes
    /// `file`; otherwise the config itself.
    fn pick_project(&mut self, config: &Path, file: &Path) -> Result<PathBuf, String> {
        let value = self.read(config)?;
        let is_solution = value
            .get("files")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
            && value.get("include").is_none();
        let Some(references) = value.get("references").and_then(Value::as_array) else {
            return Ok(config.to_path_buf());
        };
        if !is_solution {
            return Ok(config.to_path_buf());
        }
        let base = config.parent().unwrap_or(&self.root).to_path_buf();
        for reference in references {
            let Some(path) = reference.get("path").and_then(Value::as_str) else {
                continue;
            };
            let mut referenced = base.join(path);
            if referenced.is_dir() {
                referenced = referenced.join("tsconfig.json");
            }
            if !referenced.is_file() {
                continue;
            }
            let project = self.read(&referenced)?;
            let project_dir = referenced.parent().unwrap_or(&base).to_path_buf();
            if includes(&project, &project_dir, file) {
                return Ok(referenced);
            }
        }
        Ok(config.to_path_buf())
    }

    fn compiler_options(
        &mut self,
        config: &Path,
        seen: &mut Vec<PathBuf>,
    ) -> Result<serde_json::Map<String, Value>, String> {
        if seen.iter().any(|path| path == config) {
            return Err(format!("{} extends itself", config.display()));
        }
        seen.push(config.to_path_buf());
        let value = self.read(config)?;
        let mut options = serde_json::Map::new();
        let bases: Vec<String> = match value.get("extends") {
            Some(Value::String(base)) => vec![base.clone()],
            Some(Value::Array(bases)) => bases
                .iter()
                .filter_map(|base| base.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        };
        let dir = config.parent().unwrap_or(&self.root).to_path_buf();
        for base in bases {
            let path = self.locate_extends(&dir, &base).ok_or_else(|| {
                format!("{}: cannot find extended config {base:?}", config.display())
            })?;
            for (key, value) in self.compiler_options(&path, seen)? {
                options.insert(key, value);
            }
        }
        if let Some(own) = value.get("compilerOptions").and_then(Value::as_object) {
            for (key, value) in own {
                options.insert(key.clone(), value.clone());
            }
        }
        Ok(options)
    }

    fn locate_extends(&self, dir: &Path, base: &str) -> Option<PathBuf> {
        let with_json = |path: PathBuf| {
            if path.is_file() {
                Some(path)
            } else {
                let mut with = path.into_os_string();
                with.push(".json");
                let with = PathBuf::from(with);
                with.is_file().then_some(with)
            }
        };
        if base.starts_with("./") || base.starts_with("../") || base.starts_with('/') {
            return with_json(dir.join(base));
        }
        // A package: node_modules/<name>[/file], walking up to the root.
        let mut current = Some(dir.to_path_buf());
        while let Some(at) = current {
            let candidate = at.join("node_modules").join(base);
            if let Some(found) = with_json(candidate.clone()) {
                return Some(found);
            }
            if candidate.join("tsconfig.json").is_file() {
                return Some(candidate.join("tsconfig.json"));
            }
            if at == self.root {
                break;
            }
            current = at.parent().map(Path::to_path_buf);
        }
        None
    }

    fn read(&mut self, path: &Path) -> Result<Value, String> {
        if let Some(cached) = self.parsed.get(path) {
            return cached
                .clone()
                .ok_or_else(|| format!("{} is not valid JSON", path.display()));
        }
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let value = serde_json::from_str(&strip_jsonc(&text)).ok();
        self.parsed.insert(path.to_path_buf(), value.clone());
        value.ok_or_else(|| format!("{} is not valid JSON", path.display()))
    }
}

fn target_defines_class_fields(target: &str) -> bool {
    let target = target.to_ascii_lowercase();
    if target == "esnext" {
        return true;
    }
    target
        .strip_prefix("es")
        .and_then(|year| year.parse::<u32>().ok())
        .is_some_and(|year| year >= 2022)
}

/// Whether a project's `include` (default: everything) covers `file`.
fn includes(project: &Value, dir: &Path, file: &Path) -> bool {
    let Ok(relative) = file.strip_prefix(dir) else {
        return false;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    if let Some(files) = project.get("files").and_then(Value::as_array)
        && files.iter().any(|f| {
            f.as_str()
                .is_some_and(|f| f.trim_start_matches("./") == relative)
        })
    {
        return true;
    }
    let patterns: Vec<String> = match project.get("include").and_then(Value::as_array) {
        Some(patterns) => patterns
            .iter()
            .filter_map(|p| p.as_str().map(|p| p.trim_start_matches("./").to_string()))
            .collect(),
        None if project.get("files").is_some() => return false,
        None => vec!["**/*".to_string()],
    };
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim_end_matches('/');
        // A pattern with no wildcard and no extension names a directory.
        let has_wildcard = pattern.contains('*') || pattern.contains('?');
        if !has_wildcard && Path::new(pattern).extension().is_none() {
            return relative == pattern || relative.starts_with(&format!("{pattern}/"));
        }
        crate::glob::matches(pattern, &relative)
    })
}

/// JSON with comments and trailing commas, as tsconfig files are written, to
/// plain JSON.
pub fn strip_jsonc(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            out.push(c as char);
            if c == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                in_string = true;
                out.push('"');
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b',' => {
                // Drop a comma whose next significant character closes.
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                if !matches!(bytes.get(j), Some(b'}') | Some(b']')) {
                    out.push(',');
                }
                i += 1;
            }
            _ => {
                // Copy the whole UTF-8 sequence.
                let width = utf8_width(c);
                out.push_str(&text[i..i + width]);
                i += width;
            }
        }
    }
    out
}

fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonc_comments_and_trailing_commas_are_removed() {
        let text =
            "{\n  // a comment\n  \"a\": \"// not a comment\", /* block */\n  \"b\": [1, 2,],\n}\n";
        let value: Value = serde_json::from_str(&strip_jsonc(text)).unwrap();
        assert_eq!(value["a"], "// not a comment");
        assert_eq!(value["b"], serde_json::json!([1, 2]));
    }

    #[test]
    fn test_include_patterns_match_like_tsc() {
        let project = serde_json::json!({ "include": ["src"] });
        assert!(includes(
            &project,
            Path::new("/p"),
            Path::new("/p/src/a/b.tsx")
        ));
        assert!(!includes(
            &project,
            Path::new("/p"),
            Path::new("/p/vite.config.ts")
        ));
        let project = serde_json::json!({ "include": ["**/*.ts"] });
        assert!(includes(&project, Path::new("/p"), Path::new("/p/x/y.ts")));
        assert!(!includes(
            &project,
            Path::new("/p"),
            Path::new("/p/x/y.tsx")
        ));
    }

    #[test]
    fn test_class_field_default_follows_target() {
        assert!(target_defines_class_fields("ES2022"));
        assert!(target_defines_class_fields("esnext"));
        assert!(!target_defines_class_fields("ES2017"));
    }
}
