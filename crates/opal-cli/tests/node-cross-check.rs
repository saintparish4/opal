//! `opal run` against `node` on the same files.
//!
//! Every program in tests/node-diff/ prints what it observes, one value per
//! line, and must print byte-for-byte what node 24.19.0 prints and exit the
//! same way. The corpora aim at the parts of the runtime whose output has
//! rules a near miss breaks: util.inspect's layout (console.log of any
//! object), and Buffer's encodings and search offsets, which are ports of
//! Node's C++.
//!
//! Needs `node` on PATH, at the pinned version, which is why the tests are
//! `#[ignore]`d in the ordinary suite; CI runs them in their own job:
//!
//! ```text
//! cargo test -p opal-cli --test node-cross-check -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

const OPAL: &str = env!("CARGO_BIN_EXE_opal");
const PINNED_NODE: &str = "v24.19.0";

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node-diff")
}

fn check(file: &str) {
    let version = Command::new("node")
        .arg("--version")
        .output()
        .expect("run node");
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        PINNED_NODE,
        "the cross-check compares against node {PINNED_NODE}"
    );
    let dir = corpus();
    let node = Command::new("node")
        .current_dir(&dir)
        .arg(file)
        .output()
        .unwrap();
    let opal = Command::new(OPAL)
        .current_dir(&dir)
        .args(["run", file])
        .output()
        .unwrap();
    let node_out = String::from_utf8_lossy(&node.stdout);
    let opal_out = String::from_utf8_lossy(&opal.stdout);
    if node_out != opal_out {
        let first = node_out
            .lines()
            .zip(opal_out.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| node_out.lines().count().min(opal_out.lines().count()));
        panic!(
            "{file}: stdout differs from line {}\n  node: {:?}\n  opal: {:?}\nopal stderr:\n{}",
            first + 1,
            node_out.lines().nth(first),
            opal_out.lines().nth(first),
            String::from_utf8_lossy(&opal.stderr)
        );
    }
    assert_eq!(
        opal.status.code(),
        node.status.code(),
        "{file}: exit status"
    );
}

#[test]
#[ignore = "needs node on PATH"]
fn inspect() {
    check("inspect.js");
}

#[test]
#[ignore = "needs node on PATH"]
fn buffer() {
    check("buffer.js");
}

/// Every file in the corpus has a test above, so a new one can't be added
/// and never run.
#[test]
fn every_corpus_file_is_checked() {
    let source = include_str!("node-cross-check.rs");
    for entry in std::fs::read_dir(corpus()).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            source.contains(&format!("check(\"{name}\")")),
            "tests/node-diff/{name} has no test in {}",
            file!()
        );
    }
}
