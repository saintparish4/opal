//! `opal install` end to end, against a fixture registry.
//!
//! Everything here runs offline. The registry is a directory of packuments and
//! real gzipped tarballs served over `file://`, so the client, the integrity
//! check, the tarball reader, the CAS, and the linker are all the production
//! ones.

use std::path::{Path, PathBuf};

use opal_core::cache::CacheRoot;
use opal_core::graph::{ResolverOptions, resolver};
use opal_core::path::NormalizedPath;
use opal_pm::diagnose::{self, Severity};
use opal_pm::fixtures::{FixtureRegistry, Package, write_project};
use opal_pm::install::{self, InstallError, InstallOptions, InstallReport};
use opal_pm::lockfile;
use opal_pm::package::PackageStore;
use opal_pm::platform::Platform;
use opal_pm::progress::{Silent, Stage};
use opal_pm::projects::ProjectIndex;
use opal_pm::registry::NpmRegistry;

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

    fn install(&self) -> Result<InstallReport, InstallError> {
        self.install_with(&InstallOptions::default())
    }

    fn install_with(&self, options: &InstallOptions) -> Result<InstallReport, InstallError> {
        self.install_at(&self.project.clone(), options)
    }

    fn install_reporting(
        &self,
        progress: &dyn opal_pm::progress::Progress,
    ) -> Result<InstallReport, InstallError> {
        install::install(
            &self.project,
            &NpmRegistry::new(self.registry.url()),
            &self.store,
            &self.projects,
            &InstallOptions::default(),
            progress,
        )
    }

    fn install_at(
        &self,
        project: &Path,
        options: &InstallOptions,
    ) -> Result<InstallReport, InstallError> {
        let registry = NpmRegistry::new(self.registry.url());
        install::install(
            project,
            &registry,
            &self.store,
            &self.projects,
            options,
            &Silent,
        )
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.project.join(relative)
    }

    fn installed(&self, relative: &str) -> bool {
        self.path(relative).join(".opal-package").is_file()
    }

    fn lockfile(&self) -> String {
        std::fs::read_to_string(lockfile::path_in(&self.project)).expect("lockfile")
    }
}

#[test]
fn test_installs_a_flat_tree() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("b", "1.0.0"))
        .publish(Package::new("a", "1.0.0").dependency("b", "^1.0.0"));
    sandbox.project(serde_json::json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "a": "^1.0.0" }
    }));

    let report = sandbox.install().expect("install");

    assert_eq!(report.packages, 2);
    assert_eq!(report.fetched, 2);
    assert!(sandbox.installed("node_modules/a"));
    assert!(sandbox.installed("node_modules/b"));
    assert!(sandbox.path("node_modules/a/package.json").is_file());
    assert!(sandbox.path("node_modules/a/index.js").is_file());
}

#[test]
fn test_conflicting_versions_nest_under_the_dependent() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("shared", "1.0.0"))
        .publish(Package::new("shared", "2.0.0"))
        .publish(Package::new("a", "1.0.0").dependency("shared", "^1.0.0"))
        .publish(Package::new("b", "1.0.0").dependency("shared", "^2.0.0"));
    sandbox.project(serde_json::json!({
        "dependencies": { "a": "^1.0.0", "b": "^1.0.0" }
    }));

    sandbox.install().expect("install");

    // One version wins the hoisted slot, the other nests. Both are present, and
    // each dependent resolves to the one it asked for.
    assert!(sandbox.installed("node_modules/shared"));
    let nested = sandbox.installed("node_modules/a/node_modules/shared")
        || sandbox.installed("node_modules/b/node_modules/shared");
    assert!(nested, "the conflicting version should have nested");
}

#[test]
fn test_second_run_changes_nothing() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));

    let first = sandbox.install().expect("first install");
    let lockfile = sandbox.lockfile();
    let second = sandbox.install().expect("second install");

    assert!(first.resolved, "the first run has no lockfile to reuse");
    assert!(!second.resolved, "the second run reuses opal.lock");
    assert_eq!(second.fetched, 0, "contents are already in the store");
    assert_eq!(second.already_stored, 1);
    assert_eq!(second.link.added, 0);
    assert_eq!(second.link.unchanged, 1);
    assert_eq!(sandbox.lockfile(), lockfile, "lockfile must be stable");
}

#[test]
fn test_editing_package_json_re_resolves() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("a", "2.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    sandbox.install().expect("install");
    assert!(sandbox.lockfile().contains("pkg a 1.0.0 "));

    sandbox.project(serde_json::json!({ "dependencies": { "a": "^2.0.0" } }));
    let report = sandbox.install().expect("reinstall");

    assert!(report.resolved);
    assert!(sandbox.lockfile().contains("pkg a 2.0.0 "));
    assert!(!sandbox.lockfile().contains("pkg a 1.0.0 "));
    // The old version is gone from the tree, not merely shadowed.
    let marker =
        std::fs::read_to_string(sandbox.path("node_modules/a/.opal-package")).expect("marker");
    assert!(marker.contains("2.0.0"));
}

#[test]
fn test_frozen_lockfile_refuses_to_re_resolve() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("a", "2.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    sandbox.install().expect("install");

    sandbox.project(serde_json::json!({ "dependencies": { "a": "^2.0.0" } }));
    let options = InstallOptions {
        frozen_lockfile: true,
        ..InstallOptions::default()
    };
    assert!(matches!(
        sandbox.install_with(&options),
        Err(InstallError::LockfileOutdated)
    ));
}

#[test]
fn test_removing_a_dependency_removes_it_from_the_tree() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("b", "1.0.0"));
    sandbox.project(serde_json::json!({
        "dependencies": { "a": "^1.0.0", "b": "^1.0.0" }
    }));
    sandbox.install().expect("install");
    assert!(sandbox.installed("node_modules/b"));

    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    let report = sandbox.install().expect("reinstall");

    assert_eq!(report.link.removed, 1);
    assert!(!sandbox.path("node_modules/b").exists());
    assert!(sandbox.installed("node_modules/a"));
}

#[test]
fn test_a_package_without_its_marker_is_rebuilt() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    sandbox.install().expect("install");

    // Exactly the state a kill mid-materialization leaves: files present,
    // marker absent.
    std::fs::remove_file(sandbox.path("node_modules/a/.opal-package")).expect("remove marker");
    std::fs::remove_file(sandbox.path("node_modules/a/index.js")).expect("remove file");

    let report = sandbox.install().expect("reinstall");
    assert_eq!(report.link.added, 1);
    assert!(sandbox.path("node_modules/a/index.js").is_file());
    assert!(sandbox.installed("node_modules/a"));
}

#[test]
fn test_files_are_hardlinked_from_the_store() {
    use std::os::unix::fs::MetadataExt as _;

    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    let report = sandbox.install().expect("install");

    assert!(report.link.files_linked >= 2, "package.json and index.js");
    let installed = std::fs::metadata(sandbox.path("node_modules/a/index.js")).expect("metadata");
    assert!(
        installed.nlink() >= 2,
        "an installed file shares its inode with the CAS object"
    );
}

#[test]
fn test_executable_bins_are_symlinked_and_runnable() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(
        Package::new("tool", "1.0.0")
            .executable("cli.js", "#!/usr/bin/env node\nconsole.log('hi');\n")
            .bin("tool", "./cli.js"),
    );
    sandbox.project(serde_json::json!({ "dependencies": { "tool": "^1.0.0" } }));
    let report = sandbox.install().expect("install");

    assert_eq!(report.link.bins, 1);
    let link = sandbox.path("node_modules/.bin/tool");
    let target = std::fs::read_link(&link).expect("bin entry is a symlink");
    assert_eq!(target, Path::new("../tool/cli.js"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&link)
            .expect("metadata")
            .permissions()
            .mode();
        assert!(mode & 0o111 != 0, "bin target must be executable");
    }
}

#[test]
fn test_a_tampered_tarball_is_refused() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));

    // Rewrite the tarball after publishing, leaving the packument's integrity
    // pointing at the original bytes — a corrupted mirror, or an attack.
    let tarball = sandbox
        .registry
        .url()
        .trim_start_matches("file://")
        .to_string();
    std::fs::write(
        Path::new(&tarball).join("tarballs").join("a-1.0.0.tgz"),
        b"not a tarball",
    )
    .expect("tamper");

    let error = sandbox.install().expect_err("install must fail");
    assert!(
        matches!(error, InstallError::Package(_)),
        "unexpected error: {error}"
    );
    assert!(!sandbox.path("node_modules/a").exists());
}

#[test]
fn test_missing_optional_dependency_is_skipped_not_fatal() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0").optional_dependency("never-published", "^1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));

    let report = sandbox.install().expect("install");
    assert!(sandbox.installed("node_modules/a"));
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0, "never-published");
    assert!(sandbox.lockfile().contains("skip never-published"));
}

#[test]
fn test_dist_tags_resolve() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("a", "1.4.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "latest" } }));

    sandbox.install().expect("install");
    assert!(sandbox.lockfile().contains("pkg a 1.4.0 "));
}

#[test]
fn test_production_install_skips_dev_dependencies() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("tool", "1.0.0"));
    sandbox.project(serde_json::json!({
        "dependencies": { "a": "^1.0.0" },
        "devDependencies": { "tool": "^1.0.0" }
    }));

    let options = InstallOptions {
        include_development: false,
        ..InstallOptions::default()
    };
    sandbox.install_with(&options).expect("install");

    assert!(sandbox.installed("node_modules/a"));
    assert!(!sandbox.path("node_modules/tool").exists());
}

/// Records what the pipeline reported, so the seam can be asserted without a
/// terminal anywhere near it.
#[derive(Default)]
struct Recorder {
    stages: std::cell::RefCell<Vec<Stage>>,
    fetched: std::cell::RefCell<Vec<(String, bool)>>,
    finished: std::cell::Cell<bool>,
}

impl opal_pm::progress::Progress for Recorder {
    fn stage(&self, stage: Stage) {
        self.stages.borrow_mut().push(stage);
    }

    fn fetched(&self, id: &opal_pm::resolve::PackageId, from_store: bool) {
        self.fetched
            .borrow_mut()
            .push((id.name.clone(), from_store));
    }

    fn finished(&self) {
        self.finished.set(true);
    }
}

#[test]
fn test_the_pipeline_reports_each_stage_once_and_every_package() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("b", "1.0.0"))
        .publish(Package::new("a", "1.0.0").dependency("b", "^1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));

    let first = Recorder::default();
    sandbox.install_reporting(&first).expect("install");

    assert_eq!(
        first.stages.borrow().as_slice(),
        [
            Stage::Resolving,
            Stage::Fetching { packages: 2 },
            Stage::Linking { packages: 2 },
        ]
    );
    assert_eq!(
        first.fetched.borrow().as_slice(),
        [("a".to_string(), false), ("b".to_string(), false)],
        "a cold store reports every package as a download"
    );
    assert!(first.finished.get());

    // A second run answers from the lockfile, so there is no resolve stage to
    // report and every package is already in the store.
    let second = Recorder::default();
    sandbox.install_reporting(&second).expect("second install");
    assert_eq!(
        second.stages.borrow().as_slice(),
        [
            Stage::Fetching { packages: 2 },
            Stage::Linking { packages: 2 }
        ]
    );
    assert!(
        second
            .fetched
            .borrow()
            .iter()
            .all(|(_, from_store)| *from_store)
    );
}

#[test]
fn test_an_alias_installs_two_majors_side_by_side() {
    // The shape `@isaacs/cliui@8.0.2` has, and the reason aliases exist: one
    // package depending on two majors of another at the same time.
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("width", "4.2.3").file("index.js", "module.exports = 4;\n"))
        .publish(Package::new("width", "5.1.2").file("index.js", "module.exports = 5;\n"))
        .publish(
            Package::new("cliui", "1.0.0")
                .dependency("width", "^5.0.0")
                .alias("width-cjs", "width", "^4.2.0")
                .file(
                    "index.js",
                    "module.exports = require('width') + require('width-cjs');\n",
                ),
        );
    sandbox.project(serde_json::json!({ "dependencies": { "cliui": "^1.0.0" } }));

    let report = sandbox.install().expect("install");

    assert!(sandbox.installed("node_modules/width"));
    assert!(sandbox.installed("node_modules/width-cjs"));
    let aliased = std::fs::read_to_string(sandbox.path("node_modules/width-cjs/package.json"))
        .expect("the alias directory holds the aliased package");
    assert!(
        aliased.contains("\"name\": \"width\"") && aliased.contains("\"version\": \"4.2.3\""),
        "the directory is a name, the contents are the package: {aliased}"
    );
    assert_eq!(report.packages, 3);

    // And the lockfile says which package the name resolves to.
    assert!(
        sandbox
            .lockfile()
            .contains("dep cliui 1.0.0 width-cjs width 4.2.3 - npm:width@^4.2.0\n"),
        "{}",
        sandbox.lockfile()
    );

    // Reused from the lockfile, the alias still lands in the same place.
    std::fs::remove_dir_all(sandbox.path("node_modules")).expect("clear");
    let second = sandbox.install().expect("second install");
    assert!(!second.resolved);
    assert!(sandbox.installed("node_modules/width-cjs"));
}

#[test]
fn test_an_alias_at_the_project_root_installs_under_its_own_name() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("real", "2.0.0").file("index.js", "module.exports = 'real';\n"));
    sandbox.project(serde_json::json!({
        "dependencies": { "renamed": "npm:real@^2.0.0" }
    }));

    sandbox.install().expect("install");

    assert!(sandbox.installed("node_modules/renamed"));
    assert!(!sandbox.path("node_modules/real").exists());
    assert!(
        sandbox
            .lockfile()
            .contains("require dependency renamed real 2.0.0 npm:real@^2.0.0\n"),
        "{}",
        sandbox.lockfile()
    );
}

#[test]
fn test_a_required_package_for_another_platform_fails_the_install() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("mac-only", "1.0.0").platform(&["darwin"], &[]));
    sandbox.project(serde_json::json!({
        "dependencies": { "mac-only": "^1.0.0" }
    }));

    let error = sandbox
        .install_with(&InstallOptions {
            platform: Platform::new("linux", "x64"),
            ..InstallOptions::default()
        })
        .expect_err("a required dependency this host cannot run is EBADPLATFORM");

    assert!(
        matches!(error, InstallError::UnsupportedPlatform { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("requires os darwin"));
}

#[test]
fn test_a_root_pin_survives_a_higher_transitive_version() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("shared", "1.0.0"))
        .publish(Package::new("shared", "2.0.0"))
        .publish(Package::new("b", "1.0.0").dependency("shared", "^2.0.0"));
    sandbox.project(serde_json::json!({
        "dependencies": { "shared": "^1.0.0", "b": "^1.0.0" }
    }));

    sandbox.install().expect("install");

    // Deriving the root from version order hoists b's 2.0.0 into the project's
    // own slot and never places 1.0.0 at all — a silently wrong major.
    let installed = std::fs::read_to_string(sandbox.path("node_modules/shared/package.json"))
        .expect("shared is installed");
    assert!(
        installed.contains("\"version\": \"1.0.0\""),
        "the project asked for shared@^1.0.0: {installed}"
    );
    assert!(sandbox.installed("node_modules/b/node_modules/shared"));
}

#[test]
fn test_a_dist_tag_root_survives_a_higher_transitive_version() {
    let mut sandbox = Sandbox::new();
    // The fixture registry points `latest` at whatever was published last, so
    // this leaves 2.0.0 published but untagged.
    sandbox
        .registry
        .publish(Package::new("shared", "2.0.0"))
        .publish(Package::new("shared", "1.0.0"))
        .publish(Package::new("b", "1.0.0").dependency("shared", "^2.0.0"));
    sandbox.project(serde_json::json!({
        "dependencies": { "shared": "latest", "b": "^1.0.0" }
    }));

    sandbox.install().expect("install");

    // A tag maps to a version through the packument, so this is the case the
    // spec alone cannot recover — only the version recorded in the lockfile.
    let installed = std::fs::read_to_string(sandbox.path("node_modules/shared/package.json"))
        .expect("shared is installed");
    assert!(installed.contains("\"version\": \"1.0.0\""), "{installed}");
    assert!(
        sandbox
            .lockfile()
            .contains("require dependency shared - 1.0.0 latest\n")
    );

    // And again from the lockfile, without the registry to ask.
    let report = sandbox.install().expect("second install");
    assert!(!report.resolved);
    let installed = std::fs::read_to_string(sandbox.path("node_modules/shared/package.json"))
        .expect("shared is installed");
    assert!(installed.contains("\"version\": \"1.0.0\""), "{installed}");
}

#[test]
fn test_production_leaves_the_lockfile_alone() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("tool", "1.0.0"));
    sandbox.project(serde_json::json!({
        "dependencies": { "a": "^1.0.0" },
        "devDependencies": { "tool": "^1.0.0" }
    }));

    sandbox.install().expect("dev install");
    let dev_lockfile = sandbox.lockfile();

    let report = sandbox
        .install_with(&InstallOptions {
            include_development: false,
            ..InstallOptions::default()
        })
        .expect("production install");

    assert!(!report.resolved, "a dev lockfile already answers this");
    assert_eq!(
        sandbox.lockfile(),
        dev_lockfile,
        "opal.lock must not change"
    );
    assert!(sandbox.installed("node_modules/a"));
    assert!(!sandbox.path("node_modules/tool").exists());
    assert_eq!(report.fetched, 0, "tool is neither linked nor fetched");
}

#[test]
fn test_production_and_frozen_lockfile_work_together() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("tool", "1.0.0"));
    sandbox.project(serde_json::json!({
        "dependencies": { "a": "^1.0.0" },
        "devDependencies": { "tool": "^1.0.0" }
    }));

    sandbox.install().expect("dev install");
    std::fs::remove_dir_all(sandbox.path("node_modules")).expect("clear the tree");

    // The CI invocation: a committed lockfile, no dev dependencies, and no
    // permission to re-resolve.
    let report = sandbox
        .install_with(&InstallOptions {
            include_development: false,
            frozen_lockfile: true,
            ..InstallOptions::default()
        })
        .expect("production frozen install");

    assert!(!report.resolved);
    assert!(sandbox.installed("node_modules/a"));
    assert!(!sandbox.path("node_modules/tool").exists());
}

#[test]
fn test_a_dev_only_package_is_still_linked_when_a_runtime_dependency_needs_it() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("shared", "1.0.0"))
        .publish(Package::new("a", "1.0.0").dependency("shared", "^1.0.0"));
    sandbox.project(serde_json::json!({
        "dependencies": { "a": "^1.0.0" },
        "devDependencies": { "shared": "^1.0.0" }
    }));

    sandbox
        .install_with(&InstallOptions {
            include_development: false,
            ..InstallOptions::default()
        })
        .expect("production install");

    assert!(
        sandbox.installed("node_modules/shared"),
        "a reaches shared, so dropping the dev root must not drop the package"
    );
}

#[test]
fn test_a_package_for_another_platform_is_recorded_but_never_installed() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("a", "1.0.0"))
        .publish(Package::new("native-darwin", "1.0.0").platform(&["darwin"], &[]))
        .publish(Package::new("native-linux", "1.0.0").platform(&["linux"], &[]));
    sandbox.project(serde_json::json!({
        "dependencies": { "a": "^1.0.0" },
        "optionalDependencies": {
            "native-darwin": "^1.0.0",
            "native-linux": "^1.0.0"
        }
    }));

    let report = sandbox
        .install_with(&InstallOptions {
            platform: Platform::new("linux", "x64"),
            ..InstallOptions::default()
        })
        .expect("install");

    assert!(sandbox.installed("node_modules/native-linux"));
    assert!(!sandbox.path("node_modules/native-darwin").exists());
    assert_eq!(report.platform_skipped.len(), 1);
    assert_eq!(
        report.platform_skipped[0].0.to_string(),
        "native-darwin@1.0.0"
    );
    // Never downloaded either: a, plus the one native binary this host runs.
    assert_eq!(report.fetched, 2);

    // The lockfile stays portable — it records the package it did not install,
    // constraints and all, so the same file drives a macOS install.
    let lockfile = sandbox.lockfile();
    assert!(lockfile.contains("pkg native-darwin 1.0.0 "));
    assert!(lockfile.contains(" darwin - "));
}

#[test]
fn test_the_same_lockfile_installs_the_other_platforms_binary() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("native-darwin", "1.0.0").platform(&["darwin"], &[]))
        .publish(Package::new("native-linux", "1.0.0").platform(&["linux"], &[]));
    sandbox.project(serde_json::json!({
        "optionalDependencies": {
            "native-darwin": "^1.0.0",
            "native-linux": "^1.0.0"
        }
    }));

    sandbox
        .install_with(&InstallOptions {
            platform: Platform::new("linux", "x64"),
            ..InstallOptions::default()
        })
        .expect("linux install");
    let linux_lockfile = sandbox.lockfile();
    std::fs::remove_dir_all(sandbox.path("node_modules")).expect("clear the tree");

    let report = sandbox
        .install_with(&InstallOptions {
            platform: Platform::new("darwin", "arm64"),
            frozen_lockfile: true,
            ..InstallOptions::default()
        })
        .expect("darwin install from the same lockfile");

    assert!(!report.resolved, "the lockfile is platform-independent");
    assert_eq!(sandbox.lockfile(), linux_lockfile);
    assert!(sandbox.installed("node_modules/native-darwin"));
    assert!(!sandbox.path("node_modules/native-linux").exists());
}

#[test]
fn test_a_cpu_constraint_is_checked_independently_of_os() {
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("arm-only", "1.0.0").platform(&["linux"], &["arm64"]));
    sandbox.project(serde_json::json!({
        "optionalDependencies": { "arm-only": "^1.0.0" }
    }));

    let report = sandbox
        .install_with(&InstallOptions {
            platform: Platform::new("linux", "x64"),
            ..InstallOptions::default()
        })
        .expect("install");

    assert!(!sandbox.path("node_modules/arm-only").exists());
    assert_eq!(report.platform_skipped.len(), 1);
    assert!(report.platform_skipped[0].1.contains("cpu arm64"));
}

/// The header this build writes. Spelled from the constant so a version bump
/// cannot quietly turn a downgrade test into a no-op — which is exactly what
/// happened when v3 landed.
fn current_header() -> String {
    format!("opal-lock {}", lockfile::LOCKFILE_VERSION)
}

#[test]
fn test_an_older_lockfile_is_re_resolved() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));

    sandbox.install().expect("install");
    let v1 = sandbox.lockfile().replace(&current_header(), "opal-lock 1");
    std::fs::write(sandbox.path("opal.lock"), &v1).expect("downgrade the lockfile");

    let report = sandbox.install().expect("install over a v1 lockfile");

    assert!(report.lockfile_upgraded);
    assert!(report.resolved);
    assert!(
        sandbox
            .lockfile()
            .starts_with(&format!("{}\n", current_header()))
    );
    assert!(sandbox.installed("node_modules/a"));
}

#[test]
fn test_an_older_lockfile_is_not_rewritten_under_frozen_lockfile() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));

    sandbox.install().expect("install");
    let v1 = sandbox.lockfile().replace(&current_header(), "opal-lock 1");
    std::fs::write(sandbox.path("opal.lock"), &v1).expect("downgrade the lockfile");

    let error = sandbox
        .install_with(&InstallOptions {
            frozen_lockfile: true,
            ..InstallOptions::default()
        })
        .expect_err("CI must not silently upgrade a committed lockfile");

    assert!(matches!(error, InstallError::Lockfile(_)), "{error}");
    assert_eq!(sandbox.lockfile(), v1, "the lockfile is untouched");
}

#[test]
fn test_a_newer_lockfile_is_never_overwritten() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));

    sandbox.install().expect("install");
    let future = sandbox
        .lockfile()
        .replace(&current_header(), "opal-lock 99");
    std::fs::write(sandbox.path("opal.lock"), &future).expect("write a future lockfile");

    let error = sandbox.install().expect_err("a future lockfile is fatal");

    assert!(matches!(error, InstallError::Lockfile(_)), "{error}");
    assert_eq!(sandbox.lockfile(), future);
}

#[test]
fn test_the_module_graph_resolves_against_the_installed_tree() {
    // The cross-phase contract: Phase 1 produces a tree Phase 0's resolver walks
    // without a single unresolved runtime import.
    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("b", "1.0.0"))
        .publish(
            Package::new("a", "1.0.0")
                .dependency("b", "^1.0.0")
                .file("index.js", "module.exports = require('b');\n"),
        );
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    sandbox.install().expect("install");
    std::fs::write(sandbox.path("index.js"), "module.exports = require('a');\n").expect("entry");

    let root = NormalizedPath::from_native(&sandbox.project).expect("utf-8");
    let resolution = resolver::resolve(
        &root,
        &NormalizedPath::new("index.js"),
        &ResolverOptions::default(),
    )
    .expect("resolve");

    assert_eq!(resolution.graph.unresolved().count(), 0);
    assert!(
        resolution
            .graph
            .id_of(&NormalizedPath::new("node_modules/b/index.js"))
            .is_some(),
        "the transitive dependency is part of the graph"
    );
}

#[test]
fn test_an_absent_optional_peer_reads_as_informational() {
    // build_guide.md Phase 1.7: `debug` declares `supports-color` as an optional
    // peer. Absent is correct, and must not read like a broken tree.
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(
        Package::new("debug", "1.0.0")
            .optional_peer("supports-color", "*")
            .file("index.js", "module.exports = require('supports-color');\n"),
    );
    sandbox.project(serde_json::json!({ "dependencies": { "debug": "^1.0.0" } }));
    sandbox.install().expect("install");
    std::fs::write(
        sandbox.path("index.js"),
        "module.exports = require('debug');\n",
    )
    .expect("entry");

    let root = NormalizedPath::from_native(&sandbox.project).expect("utf-8");
    let resolution = resolver::resolve(
        &root,
        &NormalizedPath::new("index.js"),
        &ResolverOptions::default(),
    )
    .expect("resolve");

    let findings = diagnose::classify(&resolution.graph, &sandbox.project);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].package, "supports-color");
    assert_eq!(findings[0].severity, Severity::Informational);
    assert!(findings[0].explain().contains("optional peerDependency"));
}

#[test]
fn test_a_dependencys_own_dev_dependency_is_not_an_error() {
    // `sharp` declares `@img/sharp-libvips-dev` as its own devDependency.
    // Nothing installs a dependency's dev tooling, so telling the reader to
    // run `opal install` points at a command that cannot change the outcome.
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(
        Package::new("sharp", "1.0.0")
            .dev_dependency("libvips-dev", "^1.0.0")
            .file("index.js", "module.exports = require('libvips-dev');\n"),
    );
    sandbox.project(serde_json::json!({ "dependencies": { "sharp": "^1.0.0" } }));
    sandbox.install().expect("install");
    std::fs::write(
        sandbox.path("index.js"),
        "module.exports = require('sharp');\n",
    )
    .expect("entry");

    let root = NormalizedPath::from_native(&sandbox.project).expect("utf-8");
    let resolution = resolver::resolve(
        &root,
        &NormalizedPath::new("index.js"),
        &ResolverOptions::default(),
    )
    .expect("resolve");

    let findings = diagnose::classify(&resolution.graph, &sandbox.project);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].package, "libvips-dev");
    assert_eq!(findings[0].severity, Severity::Informational);
    assert_eq!(findings[0].declared_by.as_deref(), Some("sharp"));
    assert_eq!(
        findings[0].explain(),
        "libvips-dev is sharp's own devDependency, which is never installed"
    );
}

#[test]
fn test_the_projects_own_missing_dev_dependency_stays_actionable() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("tool", "1.0.0"));
    sandbox.project(serde_json::json!({
        "devDependencies": { "tool": "^1.0.0" }
    }));
    // Resolved and linked, then removed from disk: declared, not installed.
    sandbox.install().expect("install");
    std::fs::remove_dir_all(sandbox.path("node_modules/tool")).expect("remove");
    std::fs::write(
        sandbox.path("index.js"),
        "module.exports = require('tool');\n",
    )
    .expect("entry");

    let root = NormalizedPath::from_native(&sandbox.project).expect("utf-8");
    let resolution = resolver::resolve(
        &root,
        &NormalizedPath::new("index.js"),
        &ResolverOptions::default(),
    )
    .expect("resolve");

    let findings = diagnose::classify(&resolution.graph, &sandbox.project);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[0].declared_by, None);
    assert!(findings[0].explain().contains("run `opal install`"));
}

#[test]
fn test_an_undeclared_import_reads_as_an_error() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    sandbox.install().expect("install");
    std::fs::write(
        sandbox.path("index.js"),
        "module.exports = require('never-declared');\n",
    )
    .expect("entry");

    let root = NormalizedPath::from_native(&sandbox.project).expect("utf-8");
    let resolution = resolver::resolve(
        &root,
        &NormalizedPath::new("index.js"),
        &ResolverOptions::default(),
    )
    .expect("resolve");

    let findings = diagnose::classify(&resolution.graph, &sandbox.project);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Error);
    assert!(findings[0].explain().contains("declared by nothing"));
}

#[test]
fn test_node_can_require_the_installed_tree() {
    // The compatibility check from the exit criteria. Skipped rather than failed
    // where node is not installed, so it gates CI without blocking a laptop.
    let Ok(node) = which_node() else {
        eprintln!("skipping: node is not on PATH");
        return;
    };

    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("b", "1.0.0").file("index.js", "module.exports = 'b';\n"))
        .publish(
            Package::new("a", "1.0.0")
                .dependency("b", "^1.0.0")
                .file("index.js", "module.exports = 'a:' + require('b');\n"),
        );
    sandbox.project(serde_json::json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "a": "^1.0.0" }
    }));
    sandbox.install().expect("install");

    let output = std::process::Command::new(node)
        .arg("-e")
        .arg("process.stdout.write(require('a'))")
        .current_dir(&sandbox.project)
        .output()
        .expect("run node");

    assert!(
        output.status.success(),
        "node failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "a:b");
}

fn which_node() -> Result<PathBuf, ()> {
    let path = std::env::var_os("PATH").ok_or(())?;
    std::env::split_paths(&path)
        .map(|directory| directory.join("node"))
        .find(|candidate| candidate.is_file())
        .ok_or(())
}

#[test]
fn test_install_records_the_project_for_collection() {
    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));

    assert!(sandbox.projects.known().expect("known").is_empty());
    sandbox.install().expect("install");

    let known = sandbox.projects.known().expect("known");
    assert_eq!(known.len(), 1);
    assert_eq!(known[0], std::fs::canonicalize(&sandbox.project).unwrap());
}

#[test]
fn test_gc_keeps_an_installed_tree_alive() {
    use opal_core::cas::gc::GcOptions;
    use std::collections::BTreeSet;

    let mut sandbox = Sandbox::new();
    sandbox
        .registry
        .publish(Package::new("b", "1.0.0"))
        .publish(Package::new("a", "1.0.0").dependency("b", "^1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    sandbox.install().expect("install");

    let outcome = opal_pm::gc::collect(
        &sandbox.store,
        &sandbox.projects,
        &[],
        &BTreeSet::new(),
        &GcOptions::default(),
    )
    .expect("gc");
    assert_eq!(outcome.marks.projects, 1);
    assert_eq!(outcome.marks.packages, 2);
    assert_eq!(
        outcome.sweep.objects_removed, 0,
        "a lockfile's packages must survive collection"
    );
    assert!(sandbox.store.cas().audit().expect("audit").is_clean());

    // Still installable and still intact afterwards.
    let after = sandbox.install().expect("reinstall");
    assert_eq!(after.link.added, 0);
    assert_eq!(after.fetched, 0);
}

#[test]
fn test_gc_collects_once_the_project_is_gone() {
    use opal_core::cas::gc::{self, GcOptions};

    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    sandbox.install().expect("install");
    let before = sandbox.store.cas().object_hashes().expect("objects").len();
    assert!(before > 0);

    std::fs::remove_dir_all(&sandbox.project).expect("delete project");

    let marks = opal_pm::gc::mark(&sandbox.store, &sandbox.projects, &[]).expect("mark");
    assert_eq!(marks.forgotten.len(), 1);
    assert!(marks.live.is_empty());
    assert!(sandbox.projects.known().expect("known").is_empty());

    let report = gc::collect(sandbox.store.cas(), &marks.live, &GcOptions::default()).expect("gc");
    assert_eq!(report.objects_removed, before);
    assert_eq!(
        opal_pm::gc::prune_pointers(&sandbox.store).expect("prune"),
        1
    );
}

#[test]
fn test_a_shared_package_survives_while_any_project_needs_it() {
    use opal_core::cas::gc::{self, GcOptions};

    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("shared", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "shared": "^1.0.0" } }));
    sandbox.install().expect("install first");

    // A second project in the same cache, depending on the same package.
    let second = sandbox.project.parent().expect("parent").join("second");
    write_project(
        &second,
        serde_json::json!({ "dependencies": { "shared": "^1.0.0" } }),
    );
    sandbox
        .install_at(&second, &InstallOptions::default())
        .expect("install second");
    assert_eq!(sandbox.projects.known().expect("known").len(), 2);

    // Deleting the first must not collect what the second still needs.
    std::fs::remove_dir_all(&sandbox.project).expect("delete first");
    let marks = opal_pm::gc::mark(&sandbox.store, &sandbox.projects, &[]).expect("mark");
    let report = gc::collect(sandbox.store.cas(), &marks.live, &GcOptions::default()).expect("gc");

    assert_eq!(marks.projects, 1);
    assert_eq!(report.objects_removed, 0);
    assert!(second.join("node_modules/shared/.opal-package").is_file());
}

#[test]
fn test_an_extra_project_root_marks_without_being_recorded() {
    use opal_core::cas::gc::{self, GcOptions};

    let mut sandbox = Sandbox::new();
    sandbox.registry.publish(Package::new("a", "1.0.0"));
    sandbox.project(serde_json::json!({ "dependencies": { "a": "^1.0.0" } }));
    sandbox.install().expect("install");

    // The CI case: forget the project, then mark it explicitly by path.
    sandbox.projects.forget(&sandbox.project).expect("forget");
    let marks = opal_pm::gc::mark(
        &sandbox.store,
        &sandbox.projects,
        &[sandbox.project.clone()],
    )
    .expect("mark");
    let report = gc::collect(sandbox.store.cas(), &marks.live, &GcOptions::default()).expect("gc");

    assert_eq!(report.objects_removed, 0);
    assert!(
        sandbox.projects.known().expect("known").is_empty(),
        "an --project root is marked, not recorded"
    );
}
