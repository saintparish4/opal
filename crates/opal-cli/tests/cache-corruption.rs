//! A store written correctly, then damaged: what `opal cache verify` reports,
//! and what `opal install` does next.
//!
//! The crash-safety suites cover a store that a kill interrupted. This one
//! covers a store that finished and was then changed underneath: a file
//! deleted, truncated, or rewritten, a mode or a marker changed. Every test
//! asserts both halves, because a suite that only asserted detection would
//! say nothing about the thing users care about, which is whether the next
//! install hands them the damage.
//!
//! Where today's install trusts a `.opal-package` marker or a package index
//! and keeps the damage, the test says `known_gap` in its name and asserts
//! what happens now. That set is the specification for `cache verify
//! --repair` (`base/release/v051.md`): every `known_gap` here is one it has to
//! flip. 7 of the 10 tests below are `known_gap`.
//!
//! Each attack is made on the real files with the binary under test, against
//! the fixture registry, so nothing here can drift from what a user's machine
//! does.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use opal_core::hash::ContentHash;
use opal_pm::fixtures::{FixtureRegistry, Package, write_project};
use opal_pm::packuments::{PackumentCache, Record};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");
const LEAF_JS: &str = "module.exports = 'leaf, as published';\n";

/// One registry and one store, with as many projects on that store as a
/// test needs: most of what can go wrong only shows in the next project.
struct Store {
    directory: tempfile::TempDir,
    registry: FixtureRegistry,
}

impl Store {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut registry = FixtureRegistry::new(directory.path().join("registry"));
        registry
            .publish(Package::new("leaf", "1.0.0").file("index.js", LEAF_JS))
            .publish(Package::new("other", "1.0.0").file("index.js", "module.exports = 2;\n"));
        Self {
            directory,
            registry,
        }
    }

    fn cache(&self) -> PathBuf {
        self.directory.path().join("cache")
    }

    /// A project depending on `leaf`, installed.
    fn installed(&self, name: &str) -> PathBuf {
        let project = self.project(name);
        let output = self.opal(&project, &["install"]);
        assert!(output.status.success(), "install: {}", stderr(&output));
        project
    }

    fn project(&self, name: &str) -> PathBuf {
        let project = self.directory.path().join(name);
        write_project(
            &project,
            serde_json::json!({ "name": name, "dependencies": { "leaf": "^1.0.0" } }),
        );
        project
    }

    fn opal(&self, project: &Path, arguments: &[&str]) -> Output {
        Command::new(OPAL)
            .args(arguments)
            .arg("--root")
            .arg(project)
            .arg("--cache-dir")
            .arg(self.cache())
            .arg("--registry")
            .arg(self.registry.url())
            .output()
            .expect("run opal")
    }

    fn verify(&self) -> Output {
        Command::new(OPAL)
            .args(["cache", "verify", "--cache-dir"])
            .arg(self.cache())
            .output()
            .expect("run opal cache verify")
    }

    fn object(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.to_hex();
        self.cache()
            .join("cas")
            .join("objects")
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(&hex)
    }

    /// The store's copy of `leaf/index.js`: the object its content hashes to.
    fn leaf_object(&self) -> PathBuf {
        self.object(&ContentHash::of(LEAF_JS.as_bytes()))
    }

    /// The package index object `leaf`'s marker names.
    fn leaf_index(&self, project: &Path) -> PathBuf {
        let marker = std::fs::read_to_string(project.join("node_modules/leaf/.opal-package"))
            .expect("leaf's marker");
        let hash = marker.split_whitespace().nth(2).expect("an index hash");
        self.object(&ContentHash::parse_hex(hash).expect("a hash"))
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn leaf_js(project: &Path) -> String {
    std::fs::read_to_string(project.join("node_modules/leaf/index.js")).expect("leaf/index.js")
}

/// Store objects are read-only by design, so damaging one in place starts
/// with what a user, or a script they ran, would have to do first.
fn writable(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
}

fn assert_clean(output: &Output) {
    assert!(output.status.success(), "{}", stderr(output));
    assert!(stdout(output).contains("all objects match their hash keys"));
}

fn assert_corrupt(output: &Output, object: &Path) {
    let name = object.file_name().unwrap().to_string_lossy();
    assert!(
        !output.status.success(),
        "verify passed: {}",
        stdout(output)
    );
    assert!(
        stderr(output).contains(&format!("corrupt: {name}")),
        "{}",
        stderr(output)
    );
}

#[test]
fn test_deleted_object_known_gap_verify_reports_clean_and_the_next_project_fails_to_link() {
    let store = Store::new();
    let first = store.installed("first");
    std::fs::remove_file(store.leaf_object()).expect("delete the object");

    // `verify` re-hashes the objects present. Nothing tells it which should
    // be, since it reads no package index.
    assert_clean(&store.verify());

    // The first project's file is its own name for the inode, so it is
    // untouched, and the marker says the package is complete.
    let output = store.opal(&first, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(leaf_js(&first), LEAF_JS);

    // The next project finds `leaf`'s index, so it fetches nothing and then
    // has no file to link.
    let second = store.project("second");
    let output = store.opal(&second, &["install"]);
    assert!(
        !output.status.success(),
        "the second install linked nothing"
    );
    assert!(
        stderr(&output).contains("No such file or directory"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn test_object_truncated_in_place_is_reported_known_gap_and_every_project_keeps_it() {
    let store = Store::new();
    let first = store.installed("first");
    let object = store.leaf_object();
    writable(&object);
    std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&object)
        .expect("truncate the object");

    assert_corrupt(&store.verify(), &object);

    // One inode: the project's copy is the store's, so it is empty too, and
    // the install that follows trusts the marker.
    let output = store.opal(&first, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(leaf_js(&first), "");
    let second = store.installed("second");
    assert_eq!(leaf_js(&second), "");
}

#[test]
fn test_object_replaced_by_rename_is_reported_known_gap_and_the_next_project_links_it() {
    let store = Store::new();
    let first = store.installed("first");
    let object = store.leaf_object();
    let replacement = object.with_extension("new");
    std::fs::write(&replacement, "module.exports = 'not what was published';\n")
        .expect("write the replacement");
    std::fs::rename(&replacement, &object).expect("rename over the object");

    assert_corrupt(&store.verify(), &object);

    // A new inode under the store's name: the first project keeps the old
    // one, and the next project links the new one without hashing it.
    let output = store.opal(&first, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(leaf_js(&first), LEAF_JS);
    let second = store.installed("second");
    assert_eq!(
        leaf_js(&second),
        "module.exports = 'not what was published';\n"
    );
}

#[test]
fn test_edit_through_a_hardlink_is_reported_known_gap_and_install_keeps_the_edit() {
    let store = Store::new();
    let first = store.installed("first");
    let file = first.join("node_modules/leaf/index.js");
    writable(&file);
    std::fs::write(&file, "module.exports = 'edited in place';\n").expect("edit");

    assert_corrupt(&store.verify(), &store.leaf_object());

    let output = store.opal(&first, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(leaf_js(&first), "module.exports = 'edited in place';\n");
    let second = store.installed("second");
    assert_eq!(leaf_js(&second), "module.exports = 'edited in place';\n");
}

#[test]
fn test_object_made_writable_known_gap_verify_reports_clean_and_install_keeps_the_mode() {
    let store = Store::new();
    let first = store.installed("first");
    let object = store.leaf_object();
    writable(&object);

    // A mode is not content, and `verify` only hashes content. The mode is
    // what stood between a stray write and every project sharing the inode.
    assert_clean(&store.verify());

    let output = store.opal(&first, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&object), 0o644);
    assert_eq!(mode(&first.join("node_modules/leaf/index.js")), 0o644);
}

#[test]
fn test_deleted_package_directory_is_clean_and_install_rebuilds_it() {
    let store = Store::new();
    let first = store.installed("first");
    std::fs::remove_dir_all(first.join("node_modules/leaf")).expect("delete leaf");

    assert_clean(&store.verify());

    let output = store.opal(&first, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(leaf_js(&first), LEAF_JS);
}

#[test]
fn test_deleted_marker_is_clean_and_install_rebuilds_the_package() {
    let store = Store::new();
    let first = store.installed("first");
    std::fs::remove_file(first.join("node_modules/leaf/.opal-package")).expect("delete marker");
    std::fs::write(first.join("node_modules/leaf/stray.js"), "left behind").expect("stray");

    assert_clean(&store.verify());

    let output = store.opal(&first, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(first.join("node_modules/leaf/.opal-package").is_file());
    assert!(
        !first.join("node_modules/leaf/stray.js").exists(),
        "a rebuilt package starts from nothing"
    );
}

#[test]
fn test_corrupt_package_index_is_reported_known_gap_and_every_install_fails_on_it() {
    let store = Store::new();
    let first = store.installed("first");
    let index = store.leaf_index(&first);
    writable(&index);
    std::fs::write(&index, "not an index").expect("overwrite the index");

    assert_corrupt(&store.verify(), &index);

    // Every install reads the index of every package it plans, linked or
    // not, so this one is loud rather than trusted. Still a gap: nothing
    // fetches the package again, so the store stays broken until it is
    // cleared by hand.
    let unreadable = format!(
        "package index {} is unreadable",
        index.file_name().unwrap().to_string_lossy()
    );
    let output = store.opal(&first, &["install"]);
    assert!(!output.status.success(), "the install read a bad index");
    assert!(stderr(&output).contains(&unreadable), "{}", stderr(&output));
    assert_eq!(leaf_js(&first), LEAF_JS, "and left the project alone");

    let second = store.project("second");
    let output = store.opal(&second, &["install"]);
    assert!(!output.status.success(), "the install read a bad index");
    assert!(stderr(&output).contains(&unreadable), "{}", stderr(&output));
}

#[test]
fn test_unknown_cas_version_is_refused_by_verify_and_install() {
    let store = Store::new();
    let first = store.installed("first");
    std::fs::write(store.cache().join("cas/CAS_VERSION"), "999\n").expect("edit CAS_VERSION");

    let verify = store.verify();
    assert!(!verify.status.success());
    assert!(
        stderr(&verify).contains("has layout version 999, this build understands 1"),
        "{}",
        stderr(&verify)
    );

    let output = store.opal(&first, &["install"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("has layout version 999"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn test_garbled_packument_record_known_gap_fails_every_add_that_keeps_the_package() {
    let store = Store::new();
    let first = store.installed("first");
    let packuments = PackumentCache::new(store.cache().join("packuments"));
    let base = store.registry.url();
    let record = packuments.get(&base, "leaf").expect("leaf's record");
    packuments.put(
        &base,
        "leaf",
        &Record {
            body: b"\x00 not json".to_vec(),
            ..record
        },
    );

    // The packument cache isn't in the CAS, so `verify` doesn't read it.
    assert_clean(&store.verify());

    // With the lockfile matching, nothing reads metadata.
    let output = store.opal(&first, &["install"]);
    assert!(output.status.success(), "{}", stderr(&output));

    // An add re-resolves. The record parses as a package with no versions,
    // so `leaf` can't be kept: an error, not a panic. Ageing doesn't heal it.
    // A kept package's record is read at any age, and a revalidation the
    // server answers with 304 keeps the body, so it lasts until it is pruned
    // after 30 days.
    let output = store.opal(&first, &["add", "other"]);
    assert!(!output.status.success(), "the add read a garbled record");
    assert!(stderr(&output).contains("leaf"), "{}", stderr(&output));
    assert!(!stderr(&output).contains("panicked"), "{}", stderr(&output));
}
