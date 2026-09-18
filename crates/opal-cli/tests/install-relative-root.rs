//! `opal install --root .`, run from inside the project.
//!
//! A relative root once keyed every installed package as
//! `../node_modules/<name>`, so a re-install read the whole tree as stale and
//! removed those paths, which sit outside the project. A parent directory with
//! a `node_modules` of its own (a monorepo root, an enclosing app) lost every
//! package the project shared a name with.

use std::path::Path;
use std::process::Command;

use opal_pm::fixtures::{FixtureRegistry, Package, write_project};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

fn install_from_inside(project: &Path, cache: &Path, registry: &FixtureRegistry) -> String {
    let output = Command::new(OPAL)
        .current_dir(project)
        .args(["install", "--root", "."])
        .arg("--cache-dir")
        .arg(cache)
        .arg("--registry")
        .arg(registry.url())
        .output()
        .expect("run opal install");
    assert!(
        output.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn test_a_relative_root_reinstalls_nothing_and_reaches_nothing_outside() {
    let directory = tempfile::tempdir().expect("temp dir");
    let mut registry = FixtureRegistry::new(directory.path().join("registry"));
    registry.publish(Package::new("a", "1.0.0"));
    let cache = directory.path().join("cache");

    let parent = directory.path().join("parent");
    let project = parent.join("project");
    write_project(
        &project,
        serde_json::json!({ "dependencies": { "a": "^1.0.0" } }),
    );
    let outside = parent.join("node_modules").join("a").join("keep");
    std::fs::create_dir_all(outside.parent().expect("parent")).expect("create outside");
    std::fs::write(&outside, "not opal's").expect("write outside");

    install_from_inside(&project, &cache, &registry);
    let second = install_from_inside(&project, &cache, &registry);

    assert!(
        outside.is_file(),
        "an install removed a directory outside its project"
    );
    assert!(
        second.contains("0 added, 1 unchanged, 0 removed"),
        "an unchanged tree was rebuilt: {second}"
    );
}
