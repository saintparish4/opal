//! What `opal build` prints when it does not bundle: `--why` for a module
//! the bundle leaves out, and an out-dir it cannot write.

use std::path::Path;
use std::process::{Command, Output};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

/// `main.mjs` importing the package `dep`.
fn project(directory: &Path) -> std::path::PathBuf {
    let project = directory.join("project");
    let dep = project.join("node_modules").join("dep");
    std::fs::create_dir_all(&dep).expect("create dep");
    std::fs::write(
        project.join("main.mjs"),
        "import \"dep\";\nconsole.log(1);\n",
    )
    .expect("write main");
    std::fs::write(
        dep.join("package.json"),
        "{\"name\":\"dep\",\"main\":\"index.js\"}\n",
    )
    .expect("write manifest");
    std::fs::write(dep.join("index.js"), "module.exports = 1;\n").expect("write dep");
    project
}

fn opal(project: &Path, args: &[&str]) -> Output {
    Command::new(OPAL)
        .args(args)
        .arg("--cache-dir")
        .arg(project.join(".cache"))
        .current_dir(project)
        .output()
        .expect("run opal")
}

#[test]
fn test_why_follows_no_import_through_an_external() {
    let directory = tempfile::tempdir().expect("temp dir");
    let project = project(directory.path());
    let bundled = opal(
        &project,
        &["build", "main.mjs", "--why", "node_modules/dep/index.js"],
    );
    assert!(bundled.status.success());
    assert!(
        String::from_utf8_lossy(&bundled.stdout).contains("main.mjs imports \"dep\""),
        "{}",
        String::from_utf8_lossy(&bundled.stdout)
    );

    let external = opal(
        &project,
        &[
            "build",
            "main.mjs",
            "--external",
            "dep",
            "--why",
            "node_modules/dep/index.js",
        ],
    );
    let stdout = String::from_utf8_lossy(&external.stdout);
    assert_eq!(external.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("is not in the bundle"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn test_an_out_dir_it_cannot_write_is_an_error_not_a_refusal() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temp dir");
    let project = project(directory.path());
    let locked = project.join("locked");
    std::fs::create_dir(&locked).expect("create locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).expect("lock");
    let output = opal(&project, &["build", "main.mjs", "--outdir", "locked/out"]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).expect("unlock");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("E-IO"), "{stderr}");
    assert!(
        !stderr.contains("refused") && !stderr.contains("Refusals"),
        "an I/O error was reported as a refusal: {stderr}"
    );
}
