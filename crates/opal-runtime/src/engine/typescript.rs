//! TypeScript files: their types stripped (opal_core::typescript, Node's
//! strip-only semantics), with the output kept in the shared cache's CAS so
//! that a file is parsed once per content, not once per run.
//!
//! The cache key is blake3 over the stripper's version and the source, so a
//! file whose content returns to an earlier version hits again, and a new
//! stripper never reads an old one's output. An index file under
//! `<cache>/strip/` names the output's CAS object. Any cache failure (no
//! cache directory, a read-only one, an object removed by `opal cache gc`)
//! falls back to stripping, which gives the same output.

use std::path::PathBuf;
use std::sync::OnceLock;

use opal_core::atomic::TempFile;
use opal_core::cache::CacheRoot;
use opal_core::cas::Cas;
use opal_core::hash::ContentHash;
use opal_core::typescript::{STRIPPER_VERSION, StripError, strip_types};

/// The result of stripping one file: its JavaScript, or why there is none.
pub enum Stripped {
    Code(String),
    Error(StripError),
}

struct Store {
    cas: Cas,
    index: PathBuf,
}

fn store() -> Option<&'static Store> {
    static STORE: OnceLock<Option<Store>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            let root = CacheRoot::discover().ok()?;
            let cas = root.open_cas().ok()?;
            let index = root.path().join("strip");
            Some(Store { cas, index })
        })
        .as_ref()
}

fn key(source: &str) -> ContentHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"opal.strip-types\0");
    hasher.update(&STRIPPER_VERSION.to_le_bytes());
    hasher.update(source.as_bytes());
    ContentHash::from_bytes(*hasher.finalize().as_bytes())
}

fn cached(store: &Store, key: &ContentHash, source_len: usize) -> Option<String> {
    let hex = key.to_hex();
    let record = std::fs::read_to_string(store.index.join(&hex[..2]).join(&hex)).ok()?;
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(record.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    let output = store.cas.read(&ContentHash::from_bytes(bytes)).ok()?;
    // Stripping keeps every byte offset, so anything else is not this
    // file's output.
    (output.len() == source_len).then_some(())?;
    String::from_utf8(output).ok()
}

fn remember(store: &Store, key: &ContentHash, output: &str) -> Option<()> {
    let object = store.cas.put(output.as_bytes()).ok()?;
    let hex = key.to_hex();
    let dir = store.index.join(&hex[..2]);
    std::fs::create_dir_all(&dir).ok()?;
    let mut temp = TempFile::create(&dir, ".strip-").ok()?;
    use std::io::Write;
    temp.file_mut().write_all(object.to_hex().as_bytes()).ok()?;
    temp.persist(&dir.join(&hex)).ok()
}

/// `source` with its types stripped, from the cache when it has it.
pub fn strip(source: &str) -> Stripped {
    let store = store();
    let key = key(source);
    if let Some(output) = store.and_then(|store| cached(store, &key, source.len())) {
        return Stripped::Code(output);
    }
    // oxc's parser and the stripper's walk recurse once per level of
    // nesting, with no stack check, so a file nested deeper than this
    // thread's stack holds would abort the process: it is stripped on a
    // stack sized to it.
    match opal_core::stack::with_room_for(source, || strip_types(source)) {
        Ok(output) => {
            if let Some(store) = store {
                let _ = remember(store, &key, &output);
            }
            Stripped::Code(output)
        }
        Err(error) => Stripped::Error(error),
    }
}

/// The source excerpt swc prints under a TypeScript error, which Node puts
/// at the top of the error's stack: the line before, the line, carets under
/// the error, and the line after; or, for an error over several lines, each
/// of those lines indented, with `>` at its first and last.
pub fn snippet(source: &str, start: u32, end: u32) -> (usize, String) {
    // A final line break ends the last line; it doesn't start an empty one.
    let lines: Vec<&str> = source
        .strip_suffix('\n')
        .unwrap_or(source)
        .split('\n')
        .collect();
    let line_of = |offset: u32| {
        source[..(offset as usize).min(source.len())]
            .matches('\n')
            .count()
    };
    let first = line_of(start);
    let last = line_of(end.max(start + 1) - 1).max(first);
    let show = |line: &str| line.trim_end_matches('\r').replace('\t', "    ");
    let mut out = String::new();
    let from = first.saturating_sub(1);
    let to = (last + 1).min(lines.len() - 1);
    if first == last {
        let line_start = source[..start as usize].rfind('\n').map_or(0, |i| i + 1);
        let column = source[line_start..start as usize]
            .replace('\t', "    ")
            .chars()
            .count();
        let line_end = line_start + lines[first].len();
        let width = source[start as usize..(end as usize).min(line_end)]
            .chars()
            .count()
            .max(1);
        for (i, line) in lines.iter().enumerate().take(to + 1).skip(from) {
            out.push_str(&show(line));
            out.push('\n');
            if i == first {
                out.push_str(&" ".repeat(column));
                out.push_str(&"^".repeat(width));
                out.push('\n');
            }
        }
    } else {
        for (i, line) in lines.iter().enumerate().take(to + 1).skip(from) {
            let marker = if i == first || i == last {
                "  > "
            } else {
                "    "
            };
            out.push_str(marker);
            out.push_str(&show(line));
            out.push('\n');
        }
    }
    (first + 1, out)
}
