//! `opal graph` on a module nested deeper than the main thread's stack
//! holds.
//!
//! The parser recurses once per level of nesting. Resolving ran on the main
//! thread, whose 8 MiB overflowed at about 20,000 chained `?:` and aborted
//! the process, on a program Node runs.

use std::process::Command;

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

#[test]
fn test_a_deeply_nested_module_resolves_and_its_imports_are_followed() {
    let directory = tempfile::tempdir().expect("temp dir");
    let project = directory.path().join("project");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::write(project.join("main.mjs"), "import \"./deep.mjs\";\n").expect("write main");
    let chain = "a?1:".repeat(50_000);
    std::fs::write(
        project.join("deep.mjs"),
        format!("import \"./leaf.mjs\";\nvar a;\nexport const v = {chain}0;\n"),
    )
    .expect("write deep");
    std::fs::write(project.join("leaf.mjs"), "export const leaf = 1;\n").expect("write leaf");

    let output = Command::new(OPAL)
        .args(["graph", "main.mjs", "--json", "--cache-dir"])
        .arg(directory.path().join("cache"))
        .current_dir(&project)
        .output()
        .expect("run opal graph");
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
