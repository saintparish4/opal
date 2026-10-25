//! `typescript::strip_types` against Node's own stripper,
//! `module.stripTypeScriptTypes` (swc's strip-only mode), on the same
//! inputs: tests/strip/cases.json, one TypeScript program per entry, aimed at
//! the stripper's rules (where a `;` goes when a statement is erased, which
//! commas go with a `type` specifier, what is refused), and every `.ts` file
//! under the directories named by $OPAL_STRIP_CORPUS (colon-separated), for
//! real code. Where node accepts a program, the output must be node's byte
//! for byte; where node refuses one, it must be refused with the same error
//! code.
//!
//! Needs node 24.19.0 on PATH, so it is `#[ignore]`d in the ordinary suite:
//!
//! ```text
//! cargo test -p opal-core --test strip-cross-check -- --ignored --nocapture
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use opal_core::typescript::strip_types;
use serde_json::Value;

const NODE_STRIP: &str = r#"
const { stripTypeScriptTypes } = require("node:module");
const inputs = JSON.parse(require("fs").readFileSync(0, "utf8"));
const results = inputs.map((source) => {
  try {
    return { ok: true, output: stripTypeScriptTypes(source) };
  } catch (error) {
    return { ok: false, code: error.code, message: error.message };
  }
});
process.stdout.write(JSON.stringify(results));
"#;

fn ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            ts_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "ts" || e == "mts" || e == "cts")
            && !path.to_string_lossy().ends_with(".d.ts")
        {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "needs node on PATH"]
fn matches_node() {
    let cases_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/strip/cases.json");
    let cases: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(cases_path).unwrap()).unwrap();
    let mut inputs: Vec<(String, String)> = cases
        .into_iter()
        .enumerate()
        .map(|(i, c)| (format!("case {i}"), c))
        .collect();
    if let Some(dirs) = std::env::var_os("OPAL_STRIP_CORPUS") {
        let mut files = Vec::new();
        for dir in std::env::split_paths(&dirs) {
            ts_files(&dir, &mut files);
        }
        for file in files {
            if let Ok(source) = std::fs::read_to_string(&file) {
                inputs.push((file.display().to_string(), source));
            }
        }
    }

    let mut node = Command::new("node")
        .args(["--no-warnings", "-e", NODE_STRIP])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run node");
    let sources: Vec<&str> = inputs.iter().map(|(_, s)| s.as_str()).collect();
    let payload = serde_json::to_vec(&sources).unwrap();
    node.stdin.take().unwrap().write_all(&payload).unwrap();
    let output = node.wait_with_output().unwrap();
    let results: Vec<Value> = serde_json::from_slice(&output.stdout).expect("node's results");

    let mut failures = Vec::new();
    let (mut same_output, mut same_refusal) = (0, 0);
    for ((name, source), expected) in inputs.iter().zip(&results) {
        let ours = strip_types(source);
        match (expected["ok"].as_bool().unwrap(), &ours) {
            (true, Ok(output)) if output == expected["output"].as_str().unwrap() => {
                same_output += 1
            }
            (false, Err(error)) if expected["code"].as_str() == Some(error.kind.code()) => {
                same_refusal += 1
            }
            _ => failures.push(format!(
                "{name}: {source:?}\n  node: {}\n  opal: {:?}",
                if expected["ok"].as_bool().unwrap() {
                    format!("{:?}", expected["output"].as_str().unwrap())
                } else {
                    format!("{} {}", expected["code"], expected["message"])
                },
                ours
            )),
        }
    }
    println!(
        "{} inputs: {same_output} stripped as node strips them, {same_refusal} refused as node refuses them, {} differ",
        inputs.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
