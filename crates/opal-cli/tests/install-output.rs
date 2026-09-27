//! What `opal install` prints, as opposed to what it installs.
//!
//! A Next.js app skips 66 native builds for other platforms. Printed one per
//! line, they pushed the result and the warnings that matter off the screen,
//! so they collapse to one line. A dependency skipped for any other reason is
//! something the project asked for and did not get, and stays named.
//!
//! The summary is a headline and at most one detail line, which appears only
//! when it tells the reader something: that a re-run kept most of the tree,
//! or that the shared store supplied packages.
//!
//! The skipped packages here target `aix` and `sunos`, which no CI runner is,
//! so the tests hold on every host they run on.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use opal_pm::fixtures::{FixtureRegistry, Package, write_project};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

fn install(project: &Path, cache: &Path, registry: &FixtureRegistry, extra: &[&str]) -> Output {
    Command::new(OPAL)
        .arg("install")
        .args(extra)
        .arg("--root")
        .arg(project)
        .arg("--cache-dir")
        .arg(cache)
        .arg("--registry")
        .arg(registry.url())
        .output()
        .expect("run opal install")
}

#[test]
fn test_platform_skips_print_as_one_line() {
    let directory = tempfile::tempdir().expect("temp dir");
    let mut registry = FixtureRegistry::new(directory.path().join("registry"));
    registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("native-aix", "1.0.0").platform(&["aix"], &[]))
        .publish(Package::new("native-sunos", "1.0.0").platform(&["sunos"], &[]));
    let project = directory.path().join("project");
    write_project(
        &project,
        serde_json::json!({
            "dependencies": { "a": "^1.0.0" },
            "optionalDependencies": {
                "native-aix": "^1.0.0",
                "native-sunos": "^1.0.0"
            }
        }),
    );

    let output = install(&project, &directory.path().join("cache"), &registry, &[]);
    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let skipped: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("skipped"))
        .collect();
    assert_eq!(
        skipped,
        ["skipped 2 optional packages built for other platforms"],
        "full output: {stdout}"
    );
}

#[test]
fn test_a_frozen_install_without_a_lockfile_says_how_to_create_one() {
    let directory = tempfile::tempdir().expect("temp dir");
    let mut registry = FixtureRegistry::new(directory.path().join("registry"));
    registry.publish(Package::new("a", "1.0.0"));
    let project = directory.path().join("project");
    write_project(
        &project,
        serde_json::json!({ "dependencies": { "a": "^1.0.0" } }),
    );

    let output = install(
        &project,
        &directory.path().join("cache"),
        &registry,
        &["--frozen-lockfile"],
    );

    assert!(
        !output.status.success(),
        "a frozen install with no lockfile succeeded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("there is no opal.lock to install from"),
        "unexpected error: {stderr}"
    );
    assert!(
        !project.join("opal.lock").exists(),
        "a frozen install wrote a lockfile"
    );
}

fn stdout_of(output: &Output) -> Vec<String> {
    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

fn two_package_project(root: &Path) -> (FixtureRegistry, PathBuf) {
    let mut registry = FixtureRegistry::new(root.join("registry"));
    registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("b", "1.0.0"));
    let project = root.join("project");
    write_project(
        &project,
        serde_json::json!({ "dependencies": { "a": "^1.0.0", "b": "^1.0.0" } }),
    );
    (registry, project)
}

#[test]
fn test_a_fresh_install_prints_only_the_headline() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project) = two_package_project(directory.path());

    let lines = stdout_of(&install(
        &project,
        &directory.path().join("cache"),
        &registry,
        &[],
    ));

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("2 packages installed in "),
        "{lines:?}"
    );
    assert!(lines[0].contains("(resolve "), "{lines:?}");
}

#[test]
fn test_an_unchanged_project_is_already_installed() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project) = two_package_project(directory.path());
    let cache = directory.path().join("cache");
    install(&project, &cache, &registry, &[]);

    let lines = stdout_of(&install(&project, &cache, &registry, &[]));

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("2 packages already installed ("),
        "{lines:?}"
    );
}

#[test]
fn test_a_second_project_is_served_from_the_store() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project) = two_package_project(directory.path());
    let cache = directory.path().join("cache");
    install(&project, &cache, &registry, &[]);
    let second = directory.path().join("second");
    write_project(
        &second,
        serde_json::json!({ "dependencies": { "a": "^1.0.0", "b": "^1.0.0" } }),
    );

    let lines = stdout_of(&install(&second, &cache, &registry, &[]));

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[1], "all 2 already in the store");
}

#[test]
fn test_a_repaired_tree_says_what_it_kept() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project) = two_package_project(directory.path());
    let cache = directory.path().join("cache");
    install(&project, &cache, &registry, &[]);
    // What a killed install leaves behind, near enough: one package complete,
    // one gone.
    let removed = project.join("node_modules").join("b");
    Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&removed)
        .status()
        .expect("chmod");
    std::fs::remove_dir_all(&removed).expect("remove b");

    let lines = stdout_of(&install(&project, &cache, &registry, &[]));

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("2 packages installed in "),
        "{lines:?}"
    );
    assert_eq!(lines[1], "1 added, 1 already in place");
}
