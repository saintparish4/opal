//! Exit criterion: SIGKILL anywhere in the install pipeline, re-run,
//! and converge on exactly the state an uninterrupted install produces.
//!
//! The kill lands at a named point the process announces on stderr, so each
//! trial interrupts a specific stage rather than whatever the scheduler happened
//! to be doing. `testing_strategy.md` §8 names the stages: mid-download,
//! mid-verify, mid-rename, mid-link, mid-lockfile-write.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use opal_core::fault::{FAULT_ENV, READY_MARKER};
use opal_core::hash::ContentHash;
use opal_pm::fixtures::{FixtureRegistry, Package, write_project};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

/// Every stage a kill can land in, named by the code under test.
const FAULT_POINTS: &[&str] = &[
    "pm-mid-download",
    "pm-before-verify",
    "pm-mid-extract",
    "cas-before-rename",
    "pm-before-lockfile-rename",
    "pm-mid-link",
    "pm-between-packages",
];

/// One registry, shared by every world in a test, so tarball URLs — and
/// therefore lockfiles — are byte-identical across runs.
struct Fixtures {
    _directory: tempfile::TempDir,
    registry: FixtureRegistry,
}

impl Fixtures {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut registry = FixtureRegistry::new(directory.path().join("registry"));
        registry
            .publish(Package::new("leaf", "1.0.0"))
            .publish(Package::new("shared", "1.0.0"))
            .publish(Package::new("shared", "2.0.0"))
            .publish(
                Package::new("tool", "1.0.0")
                    .executable("cli.js", "#!/usr/bin/env node\nconsole.log('tool');\n")
                    .bin("tool", "./cli.js"),
            )
            .publish(
                Package::new("a", "1.0.0")
                    .dependency("leaf", "^1.0.0")
                    .dependency("shared", "^1.0.0"),
            )
            .publish(Package::new("b", "1.0.0").dependency("shared", "^2.0.0"));

        Self {
            _directory: directory,
            registry,
        }
    }
}

/// A project plus its own cache, so each trial starts cold.
struct World {
    _directory: tempfile::TempDir,
    project: PathBuf,
    cache: PathBuf,
    registry_url: String,
}

impl World {
    fn new(fixtures: &Fixtures) -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let project = directory.path().join("project");
        write_project(
            &project,
            serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "a": "^1.0.0", "b": "^1.0.0", "tool": "^1.0.0" }
            }),
        );
        Self {
            cache: directory.path().join("cache"),
            registry_url: fixtures.registry.url(),
            project,
            _directory: directory,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(OPAL);
        command
            .arg("install")
            .arg("--root")
            .arg(&self.project)
            .arg("--cache-dir")
            .arg(&self.cache)
            .arg("--registry")
            .arg(&self.registry_url);
        command
    }

    fn install(&self) {
        let output = self.command().output().expect("run opal install");
        assert!(
            output.status.success(),
            "install failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Runs an install that parks at `point`, and SIGKILLs it there.
    ///
    /// Returns whether the point was reached — a stage can legitimately not
    /// occur on a given run, and a test that assumed otherwise would be lying.
    fn install_killed_at(&self, point: &str) -> bool {
        let mut child = self
            .command()
            .env(FAULT_ENV, point)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn opal install");

        let stderr = child.stderr.take().expect("stderr");
        let mut reached = false;
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if line.starts_with(READY_MARKER) {
                reached = true;
                break;
            }
        }

        if reached {
            // SIGKILL: no unwinding, no destructors, no flush.
            child.kill().expect("kill");
        }
        child.wait().expect("reap");
        reached
    }

    /// Starts an install and waits for it to park at `point`, leaving it alive
    /// and holding the cache lock.
    fn install_parked_at(&self, point: &str) -> Child {
        let mut child = self
            .command()
            .env(FAULT_ENV, point)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn opal install");

        let stderr = child.stderr.take().expect("stderr");
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if line.starts_with(READY_MARKER) {
                return child;
            }
        }
        // Reap before failing, so a missed fault point does not also leave a
        // zombie behind for the rest of the suite.
        let _ = child.kill();
        let _ = child.wait();
        panic!("install exited without reaching {point}");
    }

    fn gc(&self) -> Command {
        let mut command = Command::new(OPAL);
        command
            .arg("cache")
            .arg("gc")
            .arg("--cache-dir")
            .arg(&self.cache);
        command
    }

    fn cache_is_clean(&self) -> bool {
        let output = Command::new(OPAL)
            .arg("cache")
            .arg("verify")
            .arg("--cache-dir")
            .arg(&self.cache)
            .output()
            .expect("run opal cache verify");
        output.status.success()
    }

    /// Everything an install is responsible for, in a comparable form.
    fn snapshot(&self) -> BTreeMap<String, String> {
        let mut entries = BTreeMap::new();
        entries.insert(
            "opal.lock".to_string(),
            std::fs::read_to_string(self.project.join("opal.lock")).unwrap_or_default(),
        );
        walk(
            &self.project.join("node_modules"),
            &self.project,
            &mut entries,
        );
        entries
    }
}

fn walk(directory: &Path, root: &Path, entries: &mut BTreeMap<String, String>) {
    let Ok(listing) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in listing.filter_map(Result::ok) {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("under the project")
            .to_string_lossy()
            .to_string();
        let metadata = std::fs::symlink_metadata(&path).expect("metadata");

        if metadata.is_symlink() {
            let target = std::fs::read_link(&path).expect("read link");
            entries.insert(relative, format!("symlink {}", target.display()));
        } else if metadata.is_dir() {
            walk(&path, root, entries);
        } else {
            // The flock file is a mutex, not content: it exists after any run
            // and never has anything in it.
            if relative.ends_with(".opal-lock") {
                continue;
            }
            let contents = std::fs::read(&path).expect("read file");
            let executable = is_executable(&metadata);
            entries.insert(
                relative,
                format!("file {} {}", ContentHash::of(&contents), executable),
            );
        }
    }
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[test]
fn test_an_uninterrupted_install_is_reproducible() {
    let fixtures = Fixtures::new();
    let first = World::new(&fixtures);
    let second = World::new(&fixtures);
    first.install();
    second.install();

    assert_eq!(
        first.snapshot(),
        second.snapshot(),
        "two clean installs of the same project must agree"
    );
    assert!(!first.snapshot().is_empty());
}

#[test]
fn test_a_kill_at_any_stage_converges_on_re_run() {
    let fixtures = Fixtures::new();
    let reference = World::new(&fixtures);
    reference.install();
    let expected = reference.snapshot();

    let mut reached_any = false;
    for point in FAULT_POINTS {
        let world = World::new(&fixtures);
        let reached = world.install_killed_at(point);
        reached_any |= reached;

        // Whatever the kill left behind, the store must never contain an object
        // that disagrees with its own hash.
        assert!(world.cache_is_clean(), "{point}: cache failed verification");

        // Re-running is the entire resume mechanism.
        world.install();
        assert!(
            world.cache_is_clean(),
            "{point}: cache dirty after recovery"
        );
        assert_eq!(
            world.snapshot(),
            expected,
            "{point}: re-running did not converge on the clean state"
        );
    }
    assert!(
        reached_any,
        "no fault point was reached — the pipeline moved out from under this test"
    );
}

#[test]
fn test_repeated_kills_still_converge() {
    let fixtures = Fixtures::new();
    let reference = World::new(&fixtures);
    reference.install();
    let expected = reference.snapshot();

    let world = World::new(&fixtures);
    for point in FAULT_POINTS {
        world.install_killed_at(point);
        assert!(world.cache_is_clean(), "{point}: cache failed verification");
    }

    world.install();
    assert_eq!(world.snapshot(), expected);
}

#[test]
fn test_a_killed_install_never_leaves_a_torn_lockfile() {
    let fixtures = Fixtures::new();
    let world = World::new(&fixtures);

    // First a complete install, so there is a previous lockfile to protect.
    world.install();
    let original = std::fs::read_to_string(world.project.join("opal.lock")).expect("lockfile");

    // Then change the requirements and kill the rewrite mid-flight.
    write_project(
        &world.project,
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "a": "^1.0.0" }
        }),
    );
    let reached = world.install_killed_at("pm-before-lockfile-rename");
    assert!(reached, "the lockfile rewrite was never reached");

    let after = std::fs::read_to_string(world.project.join("opal.lock")).expect("lockfile");
    assert_eq!(
        after, original,
        "a killed rewrite must leave the previous lockfile exactly as it was"
    );
    assert!(
        opal_pm::lockfile::read(&world.project.join("opal.lock"))
            .expect("parse")
            .is_some()
    );
}

#[test]
fn test_concurrent_installs_serialize() {
    let fixtures = Fixtures::new();
    let reference = World::new(&fixtures);
    reference.install();
    let expected = reference.snapshot();

    let world = World::new(&fixtures);
    let first = world.command().spawn().expect("spawn first");
    let second = world.command().spawn().expect("spawn second");

    let outputs = [first, second].map(|child| child.wait_with_output().expect("wait"));
    for output in &outputs {
        assert!(
            output.status.success(),
            "a racing install failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // The flock makes the second run wait, then find the work already done —
    // never interleave writes with the first.
    assert_eq!(world.snapshot(), expected);
}

#[test]
fn test_collection_waits_for_an_in_flight_install() {
    // The race the cache lock closes: without it, `gc` marks, an install writes
    // objects the mark set does not name, and the sweep collects them.
    let fixtures = Fixtures::new();
    let world = World::new(&fixtures);

    // Parked mid-extract: the lockfile is written, some objects are in the CAS,
    // and the install still holds the cache lock shared.
    let mut install = world.install_parked_at("pm-mid-extract");

    let mut collection = world
        .gc()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn opal cache gc");

    // Give it long enough that finishing would mean it never waited.
    std::thread::sleep(Duration::from_millis(750));
    assert!(
        collection.try_wait().expect("poll gc").is_none(),
        "collection ran while an install held the cache lock"
    );

    install.kill().expect("kill install");
    install.wait().expect("reap install");

    let output = collection
        .wait_with_output()
        .expect("gc did not finish once the install was gone");
    assert!(
        output.status.success(),
        "gc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("projects: 1 tracked"),
        "the interrupted install's lockfile should still be marked"
    );

    // And the interrupted install still converges afterwards.
    world.install();
    assert!(world.cache_is_clean());
}
