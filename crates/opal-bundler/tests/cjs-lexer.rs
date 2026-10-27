//! The CommonJS export lexer against Node's own: every file in a corpus is
//! lexed by `opal_bundler::cjs_lexer` and by Node's internal `cjs_lexer`
//! binding (merve), and the two results must be identical. Needs `node`
//! (24 or newer, the version the port follows), so it is `#[ignore]`d:
//!
//! ```text
//! cargo test -p opal-bundler --test integration -- --ignored --nocapture cjs_lexer::
//! ```
//!
//! `OPAL_LEXER_CORPUS=<dir>` lexes every `.js` and `.cjs` file under that
//! directory. Without it the corpus is this crate's `tests/fixtures` plus
//! any `node_modules` under the repository's `compat/frameworks`. Files
//! over 2 MB are skipped. `OPAL_NODE` names the node binary.

use std::path::{Path, PathBuf};
use std::process::Command;

use opal_bundler::cjs_lexer;
use serde_json::Value;

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MISMATCHES_SHOWN: usize = 20;

/// Lexes every file named in the JSON array at argv[2] and writes one
/// `[exports, reexports]` pair per file, in the same order, to argv[3].
/// One process for the whole corpus keeps the run to seconds.
const DRIVER: &str = r#"
const { internalBinding } = require('internal/test/binding');
const { parse } = internalBinding('cjs_lexer');
const fs = require('fs');
const files = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const results = files.map((file) => {
  const [exports, reexports] = parse(fs.readFileSync(file, 'utf8'));
  return [[...exports], reexports];
});
fs.writeFileSync(process.argv[3], JSON.stringify(results));
"#;

fn is_lexed(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("js" | "cjs")
    )
}

/// Every lexed file under `dir`. Symlinks are not followed, so npm's
/// `.bin` links and linked workspaces neither repeat files nor loop.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            collect(&path, out);
        } else if meta.is_file() && is_lexed(&path) && meta.len() <= MAX_FILE_BYTES {
            out.push(path);
        }
    }
}

/// The `node_modules` directories under `dir`, without descending into
/// them (each is collected whole).
fn node_modules_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
        if !is_dir {
            continue;
        }
        if path.file_name().is_some_and(|n| n == "node_modules") {
            out.push(path);
        } else {
            node_modules_dirs(&path, out);
        }
    }
}

fn corpus() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    match std::env::var_os("OPAL_LEXER_CORPUS") {
        Some(dir) => roots.push(PathBuf::from(dir)),
        None => {
            let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
            roots.push(crate_dir.join("tests/fixtures"));
            node_modules_dirs(&crate_dir.join("../../compat/frameworks"), &mut roots);
        }
    }
    let mut files = Vec::new();
    for root in &roots {
        collect(root, &mut files);
    }
    files.sort();
    files.dedup();
    files
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("the driver writes arrays")
        .iter()
        .map(|s| s.as_str().expect("the driver writes strings").to_string())
        .collect()
}

#[test]
#[ignore = "needs node; run with --ignored"]
fn test_port_matches_node_on_every_corpus_file() {
    let files = corpus();
    assert!(!files.is_empty(), "the corpus has no .js or .cjs files");

    let temp = tempfile::tempdir().unwrap();
    let driver = temp.path().join("driver.js");
    let list = temp.path().join("files.json");
    let results = temp.path().join("results.json");
    std::fs::write(&driver, DRIVER).unwrap();
    let names: Vec<&str> = files
        .iter()
        .map(|f| f.to_str().expect("corpus paths are UTF-8"))
        .collect();
    std::fs::write(&list, serde_json::to_string(&names).unwrap()).unwrap();

    let node = std::env::var("OPAL_NODE").unwrap_or_else(|_| "node".to_string());
    let output = Command::new(&node)
        .arg("--expose-internals")
        .arg(&driver)
        .arg(&list)
        .arg(&results)
        .output()
        .unwrap_or_else(|e| panic!("could not run {node}: {e}"));
    assert!(
        output.status.success(),
        "the node driver failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node_results: Vec<Value> =
        serde_json::from_slice(&std::fs::read(&results).unwrap()).unwrap();
    assert_eq!(node_results.len(), files.len());

    let mut mismatches = Vec::new();
    let (mut with_exports, mut with_reexports) = (0, 0);
    for (file, expected) in files.iter().zip(&node_results) {
        // Node reads the file as UTF-8 with replacement, as this does.
        let source = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
        let node = (strings(&expected[0]), strings(&expected[1]));
        let port = cjs_lexer::exports(&source);
        with_exports += usize::from(!node.0.is_empty());
        with_reexports += usize::from(!node.1.is_empty());
        if node != port {
            mismatches.push((file, node, port));
        }
    }

    // The counts show the corpus exercises the lexer rather than failing
    // to parse everywhere.
    println!(
        "cjs-lexer: compared {} files ({with_exports} with exports, \
         {with_reexports} with reexports), {} mismatch(es)",
        files.len(),
        mismatches.len()
    );
    if !mismatches.is_empty() {
        let mut report = String::new();
        for (file, node, port) in mismatches.iter().take(MISMATCHES_SHOWN) {
            report.push_str(&format!(
                "\n{}\n  node: exports {:?} reexports {:?}\n  port: exports {:?} reexports {:?}\n",
                file.display(),
                node.0,
                node.1,
                port.0,
                port.1
            ));
        }
        panic!(
            "{} of {} files lex differently from Node:{report}",
            mismatches.len(),
            files.len()
        );
    }
}
