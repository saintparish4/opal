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

    fn install_at(
        &self,
        project: &Path,
        options: &InstallOptions,
    ) -> Result<InstallReport, InstallError> {
        let registry = NpmRegistry::new(self.registry.url());
        install::install(project, &registry, &self.store, &self.projects, options)
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
