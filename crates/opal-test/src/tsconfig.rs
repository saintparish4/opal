//! The transform options a file gets from the nearest `tsconfig.json`.
//!
//! Only what changes the emitted JavaScript is read: `experimentalDecorators`,
//! `emitDecoratorMetadata`, `useDefineForClassFields` (defaulting from
//! `target` the way tsc does), `jsx` and `jsxImportSource`. `extends` is
//! followed, through relative paths and packages in node_modules. tsconfig
//! files are JSON with comments and trailing commas, so both are removed
//! before parsing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::transform::Options;

#[derive(Default)]
pub struct TsconfigCache {
    by_dir: Mutex<HashMap<PathBuf, Options>>,
}

impl TsconfigCache {
    pub fn options_for(&self, file: &Path) -> Options {
        let dir = file.parent().unwrap_or(Path::new("/")).to_path_buf();
        if let Some(options) = self.by_dir.lock().expect("lock").get(&dir) {
            return options.clone();
        }
        let options = find(&dir)
            .map(|path| options_from(&path))
            .unwrap_or_else(|| Options {
                use_define_for_class_fields: true,
                ..Options::default()
            });
        self.by_dir
            .lock()
            .expect("lock")
            .insert(dir, options.clone());
        options
    }
}

fn find(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let candidate = current.join("tsconfig.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        if current.join("package.json").is_file() && current.join("node_modules").is_dir() {
            // The project root; a tsconfig above it belongs to something else.
            return None;
        }
        dir = current.parent();
    }
    None
}

fn options_from(path: &Path) -> Options {
    let compiler = compiler_options(path, 0);
    let flag = |key: &str| compiler.get(key).and_then(Value::as_bool);
    let target = compiler
        .get("target")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "es5".to_string());
    // tsc: true when the target is ES2022 or later, or ESNext.
    let modern = target == "esnext"
        || target
            .strip_prefix("es")
            .and_then(|year| year.parse::<u32>().ok())
            .is_some_and(|year| year >= 2022);
    let jsx = compiler
        .get("jsx")
        .and_then(Value::as_str)
        .unwrap_or("react-jsx");
    Options {
        legacy_decorators: flag("experimentalDecorators").unwrap_or(false),
        emit_decorator_metadata: flag("emitDecoratorMetadata").unwrap_or(false),
        use_define_for_class_fields: flag("useDefineForClassFields").unwrap_or(modern),
        jsx_classic: jsx == "react",
        jsx_import_source: compiler
            .get("jsxImportSource")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// The merged `compilerOptions` of a tsconfig and everything it extends.
fn compiler_options(path: &Path, depth: usize) -> serde_json::Map<String, Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return serde_json::Map::new();
    };
    let Ok(config) = serde_json::from_str::<Value>(&strip_jsonc(&text)) else {
        return serde_json::Map::new();
    };
    let mut merged = serde_json::Map::new();
    if depth < 16 {
        let bases: Vec<&str> = match &config["extends"] {
            Value::String(base) => vec![base.as_str()],
            Value::Array(bases) => bases.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        let dir = path.parent().unwrap_or(Path::new("/"));
        for base in bases {
            if let Some(resolved) = resolve_extends(dir, base) {
                merged.extend(compiler_options(&resolved, depth + 1));
            }
        }
    }
    if let Some(own) = config["compilerOptions"].as_object() {
        merged.extend(own.clone());
    }
    merged
}

fn resolve_extends(dir: &Path, base: &str) -> Option<PathBuf> {
    let with_json = |p: PathBuf| {
        if p.is_file() {
            Some(p)
        } else {
            let json = p.with_extension("json");
            json.is_file().then_some(json)
        }
    };
    if base.starts_with('.') || Path::new(base).is_absolute() {
        return with_json(dir.join(base));
    }
    let mut current = Some(dir);
    while let Some(here) = current {
        let package = here.join("node_modules").join(base);
        if let Some(found) = with_json(package.clone()) {
            return Some(found);
        }
        let inside = package.join("tsconfig.json");
        if inside.is_file() {
            return Some(inside);
        }
        current = here.parent();
    }
    None
}

/// JSON with `//` and `/* */` comments and trailing commas, as plain JSON.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == ',' {
            let rest = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
            if matches!(rest, Some('}') | Some(']')) {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comments_and_trailing_commas_are_removed() {
        let text = "{\n  // a\n  \"a\": \"//not a comment\", /* b */\n  \"c\": [1, 2,],\n}";
        let value: Value = serde_json::from_str(&strip_jsonc(text)).unwrap();
        assert_eq!(value["a"], "//not a comment");
        assert_eq!(value["c"], serde_json::json!([1, 2]));
    }

    #[test]
    fn test_extends_and_target_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("base.json"),
            r#"{"compilerOptions": {"experimentalDecorators": true, "target": "ES2023"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{"extends": "./base", "compilerOptions": {"emitDecoratorMetadata": true}}"#,
        )
        .unwrap();
        let options = options_from(&dir.path().join("tsconfig.json"));
        assert!(options.legacy_decorators);
        assert!(options.emit_decorator_metadata);
        assert!(options.use_define_for_class_fields);
    }
}
