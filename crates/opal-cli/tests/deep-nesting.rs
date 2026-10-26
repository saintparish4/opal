//! `opal graph` and `opal build` on modules nested deeper than a fixed
//! stack holds.
//!
//! oxc's parser recurses once per level of nesting, and has no stack check.
//! `opal graph` parsed on the main thread's 8 MiB, which overflowed at about
//! 20,000 chained `?:` and aborted the process, on a program Node runs. Both
//! commands then parsed on 256 MiB, which 300,000 chained arrows overflow in
//! a debug build. Each module is now parsed on a stack sized to the file.

use std::path::Path;
use std::process::{Command, Output};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

/// A project whose `main.mjs` imports `deep.mjs`, which nests `chain` and
/// imports `leaf.mjs`.
fn project(directory: &Path, chain: &str) -> std::path::PathBuf {
    let project = directory.join("project");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::write(project.join("main.mjs"), "import \"./deep.mjs\";\n").expect("write main");
    std::fs::write(
        project.join("deep.mjs"),
        format!("import \"./leaf.mjs\";\nvar a;\nexport const v = {chain};\n"),
    )
    .expect("write deep");
    std::fs::write(project.join("leaf.mjs"), "export const leaf = 1;\n").expect("write leaf");
    project
}

fn opal(project: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(OPAL)
        .args(args)
        .arg("--cache-dir")
        .arg(cache)
        .current_dir(project)
        .output()
        .expect("run opal")
}

fn assert_graph_follows_the_deep_module(chain: &str) {
    let directory = tempfile::tempdir().expect("temp dir");
    let project = project(directory.path(), chain);
    let output = opal(
        &project,
        &directory.path().join("cache"),
        &["graph", "main.mjs", "--json"],
    );
    assert!(
        output.status.success(),
        "opal graph failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("leaf.mjs"),
        "the deep module's import was not followed: {stdout}"
    );
}

#[test]
fn test_a_deeply_nested_module_resolves_and_its_imports_are_followed() {
    assert_graph_follows_the_deep_module(&format!("{}0", "a?1:".repeat(50_000)));
}

#[test]
fn test_a_module_too_deep_for_a_fixed_stack_resolves() {
    assert_graph_follows_the_deep_module(&format!("{}0", "a=>".repeat(300_000)));
}

#[test]
fn test_opal_build_refuses_a_module_too_deep_for_a_fixed_stack() {
    let directory = tempfile::tempdir().expect("temp dir");
    let project = project(directory.path(), &format!("{}0", "a=>".repeat(300_000)));
    let output = opal(
        &project,
        &directory.path().join("cache"),
        &["build", "deep.mjs", "--root", ".", "--outdir", "out"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "opal build did not exit with a refusal ({}): {stderr}",
        output.status
    );
    assert!(
        stderr.contains("E-UNSUPPORTED") && stderr.contains("10000 syntax nodes deep"),
        "opal build did not refuse the module as too deep: {stderr}"
    );
}

#[test]
fn test_brackets_hidden_from_the_count_do_not_overflow() {
    // Each group opens 300 parentheses and closes them in a comment, so the
    // bracket count never passes 300 while the parser nests 150,000 deep;
    // the module never closes them, but the parser is deepest before it
    // finds that out. Each hidden level still takes two bytes of source.
    let directory = tempfile::tempdir().expect("temp dir");
    let group = format!("{}/*{}*/", "(".repeat(300), ")".repeat(300));
    let project = project(directory.path(), &format!("{}1", group.repeat(500)));
    let cache = directory.path().join("cache");

    let graph = opal(&project, &cache, &["graph", "main.mjs"]);
    assert!(
        graph.status.success(),
        "opal graph failed ({}): {}",
        graph.status,
        String::from_utf8_lossy(&graph.stderr)
    );
    let build = opal(
        &project,
        &cache,
        &["build", "deep.mjs", "--root", ".", "--outdir", "out"],
    );
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert_eq!(build.status.code(), Some(1), "({}): {stderr}", build.status);
    assert!(stderr.contains("E-PARSE"), "{stderr}");
}
