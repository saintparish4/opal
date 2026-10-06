//! What `opal install` prints, as opposed to what it installs.
//!
//! A Next.js app skips 66 native builds for other platforms. Printed one per
//! line, they pushed the result and the warnings that matter off the screen,
//! so they collapse to one line. A dependency skipped for any other reason is
//! something the project asked for and did not get, and stays named.
//!
//! The summary is a headline and at most one detail line, which appears only
//! when it tells the reader something: that a re-run kept most of the tree,
//! or that the shared store supplied packages. Above it go the project's own
//! dependencies that the run added, so a run that added nothing lists none.
//!
//! `opal add` lists what was named on the command line, whether or not the
//! tree had to change for it, and `opal remove` lists what it took out.
//!
//! The skipped packages here target `aix` and `sunos`, which no CI runner is,
//! so the tests hold on every host they run on.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use opal_pm::fixtures::{FixtureRegistry, Package, write_project};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

fn install(project: &Path, cache: &Path, registry: &FixtureRegistry, extra: &[&str]) -> Output {
    opal("install", project, cache, registry, extra)
}

fn opal(
    command: &str,
    project: &Path,
    cache: &Path,
    registry: &FixtureRegistry,
    extra: &[&str],
) -> Output {
    Command::new(OPAL)
        .arg(command)
        .args(extra)
        .arg("--root")
        .arg(project)
        .arg("--cache-dir")
        .arg(cache)
        .arg("--registry")
        .arg(registry.url())
        .output()
        .expect("run opal")
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
fn test_a_fresh_install_lists_what_it_added_then_the_headline() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project) = two_package_project(directory.path());

    let lines = stdout_of(&install(
        &project,
        &directory.path().join("cache"),
        &registry,
        &[],
    ));

    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[..2], ["+ a@1.0.0", "+ b@1.0.0"]);
    assert!(lines[2].starts_with("2 packages installed ["), "{lines:?}");
    assert!(lines[2].contains("(resolve "), "{lines:?}");
}

#[test]
fn test_more_than_five_added_dependencies_are_counted_not_listed() {
    let directory = tempfile::tempdir().expect("temp dir");
    let mut registry = FixtureRegistry::new(directory.path().join("registry"));
    let mut dependencies = serde_json::Map::new();
    for name in ["a", "b", "c", "d", "e", "f", "g"] {
        registry.publish(Package::new(name, "1.0.0"));
        dependencies.insert(name.to_string(), serde_json::json!("^1.0.0"));
    }
    let project = directory.path().join("project");
    write_project(
        &project,
        serde_json::json!({ "dependencies": dependencies }),
    );

    let lines = stdout_of(&install(
        &project,
        &directory.path().join("cache"),
        &registry,
        &[],
    ));

    assert_eq!(
        lines[..5],
        [
            "+ a@1.0.0",
            "+ b@1.0.0",
            "+ c@1.0.0",
            "+ d@1.0.0",
            "+ e@1.0.0 (+ 2 more)"
        ],
        "{lines:?}"
    );
    assert!(lines[5].starts_with("7 packages installed ["), "{lines:?}");
}

#[test]
fn test_an_install_opens_by_naming_the_build() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project) = two_package_project(directory.path());

    let output = install(&project, &directory.path().join("cache"), &registry, &[]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    let header = stderr.lines().next().unwrap_or_default();
    assert!(
        header.starts_with(concat!("opal install v", env!("CARGO_PKG_VERSION"))),
        "unexpected first line: {stderr}"
    );
    // Stdout is the result, and a script that reads it should not have to
    // skip a banner.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("opal install"), "{stdout}");
}

#[test]
fn test_piped_progress_is_the_resolve_count_alone() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project) = two_package_project(directory.path());
    let cache = directory.path().join("cache");

    let first = install(&project, &cache, &registry, &[]);
    let stderr = String::from_utf8_lossy(&first.stderr);
    let progress: Vec<&str> = stderr.lines().skip(1).collect();
    assert_eq!(progress, ["Resolving [2/2]"]);

    // The lockfile answers a second run, so nothing was resolved to count.
    let second = install(&project, &cache, &registry, &[]);
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
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
        lines[0].starts_with("2 packages already installed ["),
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

    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(lines[3], "all 2 already in the store");
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

    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0], "+ b@1.0.0");
    assert!(lines[1].starts_with("2 packages installed ["), "{lines:?}");
    assert_eq!(lines[2], "1 added, 1 already in place");
}

/// `lib` depends on `util`; `extra` stands alone. The project starts with
/// `lib` installed.
fn installed_project(root: &Path) -> (FixtureRegistry, PathBuf, PathBuf) {
    let mut registry = FixtureRegistry::new(root.join("registry"));
    registry
        .publish(Package::new("util", "1.0.0"))
        .publish(Package::new("lib", "1.0.0").dependency("util", "^1.0.0"))
        .publish(Package::new("extra", "1.0.0"));
    let project = root.join("project");
    write_project(
        &project,
        serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }),
    );
    let cache = root.join("cache");
    stdout_of(&install(&project, &cache, &registry, &[]));
    (registry, project, cache)
}

fn dependencies_of(project: &Path, group: &str) -> serde_json::Value {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project.join("package.json")).expect("package.json"))
            .expect("package.json parses");
    manifest
        .get(group)
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

fn first_line_of_stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn test_an_add_names_itself_and_lists_what_it_was_asked_for() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project, cache) = installed_project(directory.path());

    let output = opal("add", &project, &cache, &registry, &["extra"]);

    assert!(first_line_of_stderr(&output).starts_with("opal add v"));
    let lines = stdout_of(&output);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[0], "+ extra@1.0.0");
    assert!(lines[1].starts_with("3 packages installed ["), "{lines:?}");
    assert_eq!(lines[2], "1 added, 2 already in place");
    assert_eq!(
        dependencies_of(&project, "dependencies"),
        serde_json::json!({ "extra": "^1.0.0", "lib": "^1.0.0" })
    );
}

#[test]
fn test_adding_what_is_already_in_the_tree_still_lists_it() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project, cache) = installed_project(directory.path());

    // `util` is already in `node_modules` as `lib`'s dependency, so the
    // linker has nothing to add. It is still what the command did.
    let lines = stdout_of(&opal("add", &project, &cache, &registry, &["util"]));

    assert_eq!(lines[0], "+ util@1.0.0", "{lines:?}");
    assert_eq!(lines[2], "all 2 already in place", "{lines:?}");
}

#[test]
fn test_install_with_a_package_is_an_add_under_its_own_name() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project, cache) = installed_project(directory.path());

    let output = opal("install", &project, &cache, &registry, &["extra@1.0.0"]);

    assert!(first_line_of_stderr(&output).starts_with("opal install v"));
    assert_eq!(stdout_of(&output)[0], "+ extra@1.0.0");
    assert_eq!(
        dependencies_of(&project, "dependencies"),
        serde_json::json!({ "extra": "1.0.0", "lib": "^1.0.0" })
    );
}

#[test]
fn test_every_spelling_of_the_dev_flag_adds_a_dev_dependency() {
    for flag in ["-D", "-d", "--dev", "--save-dev"] {
        let directory = tempfile::tempdir().expect("temp dir");
        let (registry, project, cache) = installed_project(directory.path());

        stdout_of(&opal("add", &project, &cache, &registry, &[flag, "extra"]));

        assert_eq!(
            dependencies_of(&project, "devDependencies"),
            serde_json::json!({ "extra": "^1.0.0" }),
            "{flag}"
        );
    }
}

#[test]
fn test_exact_and_optional_are_written_as_asked() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project, cache) = installed_project(directory.path());

    stdout_of(&opal(
        "add",
        &project,
        &cache,
        &registry,
        &["-E", "-O", "extra"],
    ));

    assert_eq!(
        dependencies_of(&project, "optionalDependencies"),
        serde_json::json!({ "extra": "1.0.0" })
    );
}

#[test]
fn test_a_remove_lists_what_it_took_out_under_every_name_it_goes_by() {
    for command in ["remove", "rm", "uninstall"] {
        let directory = tempfile::tempdir().expect("temp dir");
        let (registry, project, cache) = installed_project(directory.path());

        let output = opal(command, &project, &cache, &registry, &["lib"]);

        assert!(
            first_line_of_stderr(&output).starts_with("opal remove v"),
            "{command}"
        );
        let lines = stdout_of(&output);
        assert_eq!(lines[0], "- lib", "{command}: {lines:?}");
        assert!(
            lines[1].starts_with("0 packages installed ["),
            "{command}: {lines:?}"
        );
        assert_eq!(lines[2], "2 removed", "{command}: {lines:?}");
        assert_eq!(
            dependencies_of(&project, "dependencies"),
            serde_json::Value::Null
        );
        assert!(!project.join("node_modules/lib").exists());
        assert!(!project.join("node_modules/util").exists());
    }
}

#[test]
fn test_removing_a_name_that_is_not_a_dependency_fails_and_says_which() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project, cache) = installed_project(directory.path());
    let before = std::fs::read(project.join("package.json")).expect("package.json");

    let output = opal("remove", &project, &cache, &registry, &["lbi"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr
            .trim_end()
            .ends_with("package.json: no dependency named lbi"),
        "{stderr}"
    );
    assert_eq!(
        std::fs::read(project.join("package.json")).expect("package.json"),
        before
    );
}

#[test]
fn test_what_is_not_a_registry_package_is_refused_before_anything_starts() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project, cache) = installed_project(directory.path());

    for argument in ["./local", "github:user/repo", "extra@file:../extra"] {
        let output = opal("add", &project, &cache, &registry, &[argument]);

        assert_eq!(output.status.code(), Some(1), "{argument}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("are not supported"), "{argument}: {stderr}");
        // Not even the header: nothing ran.
        assert!(!stderr.contains("opal add v"), "{argument}: {stderr}");
    }
}

#[test]
fn test_packages_and_a_frozen_lockfile_cannot_be_asked_for_together() {
    let directory = tempfile::tempdir().expect("temp dir");
    let (registry, project, cache) = installed_project(directory.path());
    let before = std::fs::read(project.join("package.json")).expect("package.json");

    let output = install(&project, &cache, &registry, &["extra", "--frozen-lockfile"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--frozen-lockfile"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(project.join("package.json")).expect("package.json"),
        before
    );
}
