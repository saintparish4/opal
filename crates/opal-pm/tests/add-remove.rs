//! `opal add` and `opal remove` end to end, against a fixture registry.
//!
//! Three things are held here that the unit tests cannot hold separately.
//! What a change writes: `package.json` and `opal.lock` agree with each other
//! and with the tree. What it leaves alone: every package the change did not
//! name keeps the version it was locked at, even though the registry has
//! published since. And what a refused change leaves behind, which is
//! nothing: both files byte for byte as they were.
//!
//! "The registry has published since" is literal. Each test installs, then
//! publishes a newer version of something already locked, then makes its
//! change, because a registry that never moves cannot tell keeping a locked
//! version from resolving it again.

use std::path::PathBuf;

use opal_core::cache::CacheRoot;
use opal_pm::edit::{AddRequest, EditError, Group};
use opal_pm::fixtures::{FixtureRegistry, Package, write_project};
use opal_pm::install::{self, Change, InstallError, InstallOptions, InstallReport};
use opal_pm::lockfile;
use opal_pm::package::PackageStore;
use opal_pm::progress::Silent;
use opal_pm::projects::ProjectIndex;
use opal_pm::registry::{NpmRegistry, RegistryError};
use opal_pm::resolve::ResolveError;

struct Sandbox {
    _directory: tempfile::TempDir,
    project: PathBuf,
    registry: FixtureRegistry,
    store: PackageStore,
    projects: ProjectIndex,
}

impl Sandbox {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = CacheRoot::at(directory.path().join("cache"));
        let store = PackageStore::open(cache.open_cas().expect("cas"), cache.path())
            .expect("package store");

        Self {
            project: directory.path().join("project"),
            registry: FixtureRegistry::new(directory.path().join("registry")),
            store,
            projects: ProjectIndex::new(cache.path().join("projects")).expect("project index"),
            _directory: directory,
        }
    }

    fn project(&self, manifest: serde_json::Value) -> &Self {
        write_project(&self.project, manifest);
        self
    }

    fn install(&self) -> InstallReport {
        install::install(
            &self.project,
            &NpmRegistry::new(self.registry.url()),
            &self.store,
            &self.projects,
            &InstallOptions::default(),
            &Silent,
        )
        .expect("install")
    }

    fn change(&self, change: &Change) -> Result<InstallReport, InstallError> {
        self.change_with(change, &InstallOptions::default())
    }

    fn change_with(
        &self,
        change: &Change,
        options: &InstallOptions,
    ) -> Result<InstallReport, InstallError> {
        install::change(
            &self.project,
            change,
            &NpmRegistry::new(self.registry.url()),
            &self.store,
            &self.projects,
            options,
            &Silent,
        )
    }

    fn add(&self, arguments: &[&str]) -> Result<InstallReport, InstallError> {
        self.change(&adding(arguments, None, false))
    }

    fn remove(&self, names: &[&str]) -> Result<InstallReport, InstallError> {
        self.change(&Change::Remove {
            names: names.iter().map(|name| name.to_string()).collect(),
        })
    }

    fn manifest_path(&self) -> PathBuf {
        self.project.join("package.json")
    }

    fn manifest(&self) -> String {
        std::fs::read_to_string(self.manifest_path()).expect("package.json")
    }

    fn declared(&self, group: &str) -> serde_json::Value {
        let manifest: serde_json::Value =
            serde_json::from_str(&self.manifest()).expect("package.json parses");
        manifest
            .get(group)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    fn lockfile(&self) -> String {
        std::fs::read_to_string(lockfile::path_in(&self.project)).unwrap_or_default()
    }

    /// Every package the lockfile holds, as `name@version`.
    fn locked(&self) -> Vec<String> {
        self.lockfile()
            .lines()
            .filter_map(|line| line.strip_prefix("pkg "))
            .map(|line| {
                let mut fields = line.split(' ');
                format!(
                    "{}@{}",
                    fields.next().unwrap_or_default(),
                    fields.next().unwrap_or_default()
                )
            })
            .collect()
    }

    /// The version in `node_modules/<name>`, read from the package itself.
    fn installed(&self, name: &str) -> Option<String> {
        let path = self.project.join("node_modules").join(name);
        if !path.join(".opal-package").is_file() {
            return None;
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.join("package.json")).ok()?).ok()?;
        Some(format!(
            "{}@{}",
            manifest["name"].as_str()?,
            manifest["version"].as_str()?
        ))
    }
}

fn adding(arguments: &[&str], group: Option<Group>, exact: bool) -> Change {
    Change::Add {
        requests: arguments
            .iter()
            .map(|argument| AddRequest::parse(argument).expect("a well-formed request"))
            .collect(),
        group,
        exact,
    }
}

fn requested(report: &InstallReport) -> Vec<String> {
    report
        .requested
        .iter()
        .map(|(name, id)| format!("{name} -> {id}"))
        .collect()
}

/// `app` depends on `lib`, which depends on `util`. Each has a 1.0.0, and a
/// 1.1.0 that [`publish_updates`] releases after the first install.
fn publish_originals(registry: &mut FixtureRegistry) {
    registry
        .publish(Package::new("util", "1.0.0"))
        .publish(Package::new("lib", "1.0.0").dependency("util", "^1.0.0"))
        .publish(Package::new("extra", "1.0.0"));
}

fn publish_updates(registry: &mut FixtureRegistry) {
    registry
        .publish(Package::new("util", "1.1.0"))
        .publish(Package::new("lib", "1.1.0").dependency("util", "^1.0.0"));
}

#[test]
fn test_adding_to_an_empty_project_writes_the_manifest_the_lockfile_and_the_tree() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "name": "app" }));

    let report = sandbox.add(&["lib"]).expect("add");

    assert_eq!(
        sandbox.manifest(),
        "{\n  \"name\": \"app\",\n  \"dependencies\": {\n    \"lib\": \"^1.0.0\"\n  }\n}"
    );
    assert_eq!(sandbox.locked(), ["lib@1.0.0", "util@1.0.0"]);
    assert!(
        sandbox
            .lockfile()
            .contains("require dependency lib - 1.0.0 ^1.0.0")
    );
    assert_eq!(sandbox.installed("lib").as_deref(), Some("lib@1.0.0"));
    assert_eq!(sandbox.installed("util").as_deref(), Some("util@1.0.0"));
    assert_eq!(requested(&report), ["lib -> lib@1.0.0"]);
}

#[test]
fn test_adding_a_package_moves_nothing_that_was_already_locked() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    publish_updates(&mut sandbox.registry);

    sandbox.add(&["extra"]).expect("add");

    assert_eq!(sandbox.locked(), ["extra@1.0.0", "lib@1.0.0", "util@1.0.0"]);
    assert_eq!(sandbox.installed("lib").as_deref(), Some("lib@1.0.0"));
    assert_eq!(sandbox.installed("util").as_deref(), Some("util@1.0.0"));
    assert_eq!(sandbox.installed("extra").as_deref(), Some("extra@1.0.0"));
}

#[test]
fn test_a_hand_edit_followed_by_install_moves_nothing_that_was_already_locked() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    publish_updates(&mut sandbox.registry);

    sandbox.project(serde_json::json!({
        "dependencies": { "extra": "^1.0.0", "lib": "^1.0.0" }
    }));
    let report = sandbox.install();

    assert!(report.resolved);
    assert_eq!(sandbox.locked(), ["extra@1.0.0", "lib@1.0.0", "util@1.0.0"]);
}

#[test]
fn test_a_new_dependency_shares_the_locked_version_of_something_it_also_needs() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    publish_updates(&mut sandbox.registry);
    sandbox
        .registry
        .publish(Package::new("other", "1.0.0").dependency("util", "^1.0.0"));

    sandbox.add(&["other"]).expect("add");

    // `other` is new and `util@1.1.0` is out, but `lib` already holds 1.0.0.
    assert_eq!(sandbox.locked(), ["lib@1.0.0", "other@1.0.0", "util@1.0.0"]);
}

#[test]
fn test_adding_a_package_that_is_already_locked_moves_it_to_what_the_registry_has_now() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    publish_updates(&mut sandbox.registry);

    let report = sandbox.add(&["lib"]).expect("add");

    assert_eq!(sandbox.declared("dependencies")["lib"], "^1.1.0");
    assert_eq!(sandbox.installed("lib").as_deref(), Some("lib@1.1.0"));
    assert_eq!(requested(&report), ["lib -> lib@1.1.0"]);
    // Naming `lib` asked for nothing about `util`, which stays where it was.
    assert_eq!(sandbox.locked(), ["lib@1.1.0", "util@1.0.0"]);
}

#[test]
fn test_naming_a_transitive_dependency_moves_its_dependents_to_the_same_copy() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    publish_updates(&mut sandbox.registry);

    sandbox.add(&["util"]).expect("add");

    assert_eq!(sandbox.declared("dependencies")["util"], "^1.1.0");
    // One `util`, not 1.1.0 for the project and 1.0.0 nested under `lib`.
    assert_eq!(sandbox.locked(), ["lib@1.0.0", "util@1.1.0"]);
    assert_eq!(sandbox.installed("util").as_deref(), Some("util@1.1.0"));
    assert!(
        !sandbox
            .project
            .join("node_modules/lib/node_modules")
            .exists()
    );
}

#[test]
fn test_naming_a_version_a_dependent_cannot_use_leaves_the_dependent_on_its_locked_one() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    publish_updates(&mut sandbox.registry);
    sandbox.registry.publish(Package::new("util", "2.0.0"));

    sandbox.add(&["util"]).expect("add");

    assert_eq!(sandbox.declared("dependencies")["util"], "^2.0.0");
    // `lib` wants ^1.0.0, which 2.0.0 is not, so it keeps the 1.0.0 it had
    // and not the 1.1.0 the registry would give it today.
    assert_eq!(sandbox.locked(), ["lib@1.0.0", "util@1.0.0", "util@2.0.0"]);
    assert_eq!(sandbox.installed("util").as_deref(), Some("util@2.0.0"));
    assert_eq!(
        sandbox.installed("lib/node_modules/util").as_deref(),
        Some("util@1.0.0")
    );
}

#[test]
fn test_a_requirement_swapped_by_hand_takes_the_version_the_removed_one_had_locked() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox
        .registry
        .publish(Package::new("other", "1.0.0").dependency("util", "^1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    publish_updates(&mut sandbox.registry);

    sandbox.project(serde_json::json!({ "dependencies": { "other": "^1.0.0" } }));
    sandbox.install();

    // Only `lib`, now gone, led to `util@1.0.0`. It is still what `other`
    // gets, as it would from npm or bun.
    assert_eq!(sandbox.locked(), ["other@1.0.0", "util@1.0.0"]);
}

#[test]
fn test_what_was_typed_decides_what_is_saved() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("a", "1.2.0"))
        .publish(Package::new("b", "1.0.0"))
        .publish(Package::new("b", "1.2.0"))
        .publish(Package::new("c", "1.0.0"))
        .publish(Package::new("c", "1.2.0"))
        .publish(Package::new("d", "1.2.0"));
    sandbox.project(serde_json::json!({ "name": "app" }));

    sandbox
        .add(&["a@1.0.0", "b@^1.0.0", "c@latest", "d"])
        .expect("add");

    assert_eq!(
        sandbox.declared("dependencies"),
        serde_json::json!({ "a": "1.0.0", "b": "^1.0.0", "c": "^1.2.0", "d": "^1.2.0" })
    );
    assert_eq!(
        sandbox.locked(),
        ["a@1.0.0", "b@1.2.0", "c@1.2.0", "d@1.2.0"]
    );
}

#[test]
fn test_exact_saves_the_resolved_version_without_a_range() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("a", "1.2.0"));
    sandbox.project(serde_json::json!({ "name": "app" }));

    sandbox
        .change(&adding(&["a@^1.0.0"], None, true))
        .expect("add");

    assert_eq!(sandbox.declared("dependencies")["a"], "1.2.0");
}

#[test]
fn test_a_group_flag_moves_a_dependency_and_the_lockfile_follows() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();

    sandbox
        .change(&adding(&["lib"], Some(Group::Development), false))
        .expect("add");

    assert_eq!(sandbox.declared("dependencies"), serde_json::Value::Null);
    assert_eq!(sandbox.declared("devDependencies")["lib"], "^1.0.0");
    assert!(
        sandbox
            .lockfile()
            .contains("require devDependency lib - 1.0.0 ^1.0.0"),
        "{}",
        sandbox.lockfile()
    );
    assert!(!sandbox.lockfile().contains("require dependency lib "));
}

#[test]
fn test_an_alias_is_saved_as_written_and_installed_under_its_own_name() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "name": "app" }));

    let report = sandbox.add(&["old-util@npm:util@^1.0.0"]).expect("add");

    assert_eq!(
        sandbox.declared("dependencies")["old-util"],
        "npm:util@^1.0.0"
    );
    assert_eq!(sandbox.installed("old-util").as_deref(), Some("util@1.0.0"));
    assert_eq!(requested(&report), ["old-util -> util@1.0.0"]);
}

#[test]
fn test_removing_a_root_keeps_what_another_root_still_needs_at_its_locked_version() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({
        "dependencies": { "extra": "^1.0.0", "lib": "^1.0.0", "util": "^1.0.0" }
    }));
    sandbox.install();
    publish_updates(&mut sandbox.registry);

    let report = sandbox.remove(&["util", "extra"]).expect("remove");

    assert_eq!(
        sandbox.declared("dependencies"),
        serde_json::json!({ "lib": "^1.0.0" })
    );
    assert_eq!(sandbox.locked(), ["lib@1.0.0", "util@1.0.0"]);
    assert_eq!(sandbox.installed("util").as_deref(), Some("util@1.0.0"));
    assert_eq!(sandbox.installed("extra"), None);
    assert_eq!(report.removed_direct, ["util", "extra"]);
    assert_eq!(report.link.removed, 1);
}

#[test]
fn test_removing_the_last_dependency_leaves_an_empty_tree_and_no_group() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "name": "app", "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();

    let report = sandbox.remove(&["lib"]).expect("remove");

    assert_eq!(sandbox.manifest(), "{\n  \"name\": \"app\"\n}");
    assert!(sandbox.locked().is_empty(), "{}", sandbox.lockfile());
    assert_eq!(sandbox.installed("lib"), None);
    assert_eq!(sandbox.installed("util"), None);
    assert_eq!(report.packages, 0);
}

/// Both files, as bytes, so a test can hold that a refused change wrote
/// neither.
fn both_files(sandbox: &Sandbox) -> (String, String) {
    (sandbox.manifest(), sandbox.lockfile())
}

#[test]
fn test_removing_a_name_that_is_not_declared_is_an_error_that_writes_nothing() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    let before = both_files(&sandbox);

    let error = sandbox.remove(&["lib", "lbi"]).unwrap_err();

    assert!(
        matches!(
            &error,
            InstallError::Edit { source: EditError::NotDeclared { names }, .. }
                if names == &["lbi".to_string()]
        ),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .ends_with("package.json: no dependency named lbi")
    );
    assert_eq!(both_files(&sandbox), before);
    assert_eq!(sandbox.installed("lib").as_deref(), Some("lib@1.0.0"));
}

#[test]
fn test_adding_a_package_the_registry_does_not_have_writes_nothing() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    let before = both_files(&sandbox);

    let error = sandbox.add(&["extra", "no-such-package"]).unwrap_err();

    assert!(
        matches!(
            &error,
            InstallError::Resolve(ResolveError::Registry(RegistryError::NotFound(name)))
                if name == "no-such-package"
        ),
        "{error}"
    );
    assert_eq!(both_files(&sandbox), before);
    assert_eq!(sandbox.installed("extra"), None);
}

#[test]
fn test_adding_a_range_nothing_satisfies_writes_nothing() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    let before = both_files(&sandbox);

    let error = sandbox.add(&["extra@^9.0.0"]).unwrap_err();

    assert!(
        matches!(
            error,
            InstallError::Resolve(ResolveError::NoMatchingVersion { .. })
        ),
        "{error}"
    );
    assert_eq!(both_files(&sandbox), before);
}

#[test]
fn test_a_dependency_whose_own_dependency_cannot_be_resolved_writes_nothing() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox
        .registry
        .publish(Package::new("broken", "1.0.0").dependency("util", "^9.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    let before = both_files(&sandbox);

    // `broken` itself exists, so this fails in the resolve proper, after the
    // range to save has already been worked out.
    sandbox.add(&["broken"]).unwrap_err();

    assert_eq!(both_files(&sandbox), before);
}

#[test]
fn test_a_lockfile_that_cannot_be_rendered_leaves_the_manifest_alone() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox
        .registry
        .publish(Package::new("hostile", "1.0.0").dependency("util", ">=1.0.0\n<2.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    let before = both_files(&sandbox);

    let error = sandbox.add(&["hostile"]).unwrap_err();

    assert!(matches!(error, InstallError::Lockfile(_)), "{error}");
    assert_eq!(both_files(&sandbox), before);
}

#[test]
fn test_a_change_under_frozen_lockfile_is_refused_before_anything_exists() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "name": "app" }));
    let frozen = InstallOptions {
        frozen_lockfile: true,
        ..InstallOptions::default()
    };

    let error = sandbox
        .change_with(&adding(&["lib"], None, false), &frozen)
        .unwrap_err();

    assert!(matches!(error, InstallError::FrozenChange), "{error}");
    assert_eq!(sandbox.manifest(), "{\n  \"name\": \"app\"\n}");
    assert!(!lockfile::path_in(&sandbox.project).exists());
    assert!(!sandbox.project.join("node_modules").exists());
}

#[test]
fn test_a_directory_that_is_not_a_project_gains_nothing() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    std::fs::create_dir_all(&sandbox.project).expect("create directory");

    let error = sandbox.add(&["lib"]).unwrap_err();

    assert!(matches!(error, InstallError::Manifest(_)), "{error}");
    let left: Vec<_> = std::fs::read_dir(&sandbox.project)
        .expect("list")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn test_a_manifest_opal_cannot_read_is_refused_before_the_registry_is_asked() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    std::fs::create_dir_all(&sandbox.project).expect("create project");
    // Found by the `edit` fuzz target. The number is valid JSON text and
    // too large for any number opal reads it into, so the editor, which
    // carries values as written, accepts a manifest the reader rejects.
    for unreadable in ["{\n  \"engines\": 32E2220\n}", "{ \"name\": "] {
        std::fs::write(sandbox.manifest_path(), unreadable).expect("write package.json");

        // A package the registry does not have: asked first, that would be
        // the error reported, and it is not what is wrong.
        let error = sandbox.add(&["no-such-package"]).unwrap_err();

        assert!(matches!(error, InstallError::Manifest(_)), "{error}");
        assert_eq!(sandbox.manifest(), unreadable);
        assert!(!lockfile::path_in(&sandbox.project).exists());
    }
}

#[test]
fn test_the_manifest_keeps_its_formatting_through_a_real_write() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    std::fs::create_dir_all(&sandbox.project).expect("create project");
    std::fs::write(
        sandbox.manifest_path(),
        "\u{feff}{\r\n    \"name\": \"app\",\r\n    \"files\": [\"a.js\", \"b.js\"],\r\n    \"description\": \"caf\\u00e9\",\r\n    \"dependencies\": {\r\n        \"lib\": \"^1.0.0\"\r\n    }\r\n}\r\n",
    )
    .expect("write package.json");

    sandbox.add(&["extra"]).expect("add");

    assert_eq!(
        sandbox.manifest(),
        "\u{feff}{\r\n    \"name\": \"app\",\r\n    \"files\": [\"a.js\", \"b.js\"],\r\n    \"description\": \"caf\\u00e9\",\r\n    \"dependencies\": {\r\n        \"extra\": \"^1.0.0\",\r\n        \"lib\": \"^1.0.0\"\r\n    }\r\n}\r\n"
    );
    assert_eq!(sandbox.installed("extra").as_deref(), Some("extra@1.0.0"));
    // Nothing but the two files and the tree: no temp file left beside them.
    let mut left: Vec<String> = std::fs::read_dir(&sandbox.project)
        .expect("list")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    left.sort();
    assert_eq!(left, ["node_modules", "opal.lock", "package.json"]);
}

#[cfg(unix)]
#[test]
fn test_the_manifest_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt as _;

    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "name": "app" }));
    std::fs::set_permissions(
        sandbox.manifest_path(),
        std::fs::Permissions::from_mode(0o600),
    )
    .expect("chmod");

    sandbox.add(&["lib"]).expect("add");

    let mode = std::fs::metadata(sandbox.manifest_path())
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn test_a_read_only_manifest_is_refused_and_the_lockfile_is_not_written() {
    use std::os::unix::fs::PermissionsExt as _;

    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "name": "app" }));
    std::fs::set_permissions(
        sandbox.manifest_path(),
        std::fs::Permissions::from_mode(0o444),
    )
    .expect("chmod");
    // Root opens anything for writing, so there is nothing to refuse.
    if std::fs::File::options()
        .write(true)
        .open(sandbox.manifest_path())
        .is_ok()
    {
        return;
    }

    let error = sandbox.add(&["lib"]).unwrap_err();

    assert!(matches!(error, InstallError::Io { .. }), "{error}");
    assert_eq!(sandbox.manifest(), "{\n  \"name\": \"app\"\n}");
    assert!(!lockfile::path_in(&sandbox.project).exists());
}

#[cfg(unix)]
#[test]
fn test_an_add_that_changes_nothing_does_not_rewrite_the_manifest() {
    use std::os::unix::fs::MetadataExt as _;

    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    let before = both_files(&sandbox);
    // A rewrite goes through a rename, which always changes the inode.
    let inode = std::fs::metadata(sandbox.manifest_path())
        .expect("metadata")
        .ino();

    let report = sandbox.add(&["lib"]).expect("add");

    assert_eq!(both_files(&sandbox), before);
    assert_eq!(
        std::fs::metadata(sandbox.manifest_path())
            .expect("metadata")
            .ino(),
        inode
    );
    assert_eq!(requested(&report), ["lib -> lib@1.0.0"]);
}

#[test]
fn test_adding_then_removing_restores_both_files_byte_for_byte() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "name": "app", "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    let before = both_files(&sandbox);
    publish_updates(&mut sandbox.registry);

    sandbox.add(&["extra"]).expect("add");
    assert_ne!(both_files(&sandbox), before);
    sandbox.remove(&["extra"]).expect("remove");

    assert_eq!(both_files(&sandbox), before);
}

#[test]
fn test_an_install_clears_its_own_stale_temp_files_and_nothing_else() {
    let mut sandbox = Sandbox::new();
    publish_originals(&mut sandbox.registry);
    sandbox.project(serde_json::json!({ "dependencies": { "lib": "^1.0.0" } }));
    sandbox.install();
    let before = both_files(&sandbox);
    // The two a killed write leaves, and three that only look like them: a
    // temp file of the user's, one named the way opal names temp files
    // everywhere else, and one from an editor.
    let ours = ["opal.lock.tmp", "package.json.opal-tmp"];
    let theirs = [
        "notes.tmp",
        "write-4242-0-123456789.tmp",
        "package.json.tmp",
    ];
    for name in ours.iter().chain(&theirs) {
        std::fs::write(sandbox.project.join(name), "left behind").expect("write");
    }

    // Nothing to resolve and nothing to link, so nothing here is written.
    let report = sandbox.install();

    assert!(!report.resolved);
    assert_eq!(both_files(&sandbox), before);
    for name in ours {
        assert!(!sandbox.project.join(name).exists(), "{name} was left");
    }
    for name in theirs {
        assert!(sandbox.project.join(name).exists(), "{name} was removed");
    }
}
