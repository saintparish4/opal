//! Exit criterion: SIGKILL anywhere in the install pipeline, re-run,
//! and converge on exactly the state an uninterrupted install produces.
//!
//! Most kills land at a named point the process announces on stderr, so each
//! trial interrupts a specific stage rather than whatever the scheduler happened
//! to be doing. `testing_strategy.md` §8 names the stages: mid-download,
//! mid-verify, mid-rename, mid-link, mid-lockfile-write. Those are only the
//! stages someone thought to instrument, so one test also kills at random
//! moments to find the ones nobody did.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

fn publish_standard(registry: &mut FixtureRegistry) {
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
}

/// A tree three `node_modules` levels deep, for [`NESTED_DEPENDENCIES`]: each
/// version of `z` conflicts with the one already placed above it, so the last
/// one nests under `x/node_modules/y`.
fn publish_nested(registry: &mut FixtureRegistry) {
    registry
        .publish(Package::new("z", "1.0.0"))
        .publish(Package::new("z", "2.0.0"))
        .publish(Package::new("z", "3.0.0"))
        .publish(Package::new("y", "1.0.0"))
        .publish(Package::new("y", "2.0.0").dependency("z", "^2.0.0"))
        .publish(
            Package::new("x", "1.0.0")
                .dependency("y", "^2.0.0")
                .dependency("z", "^3.0.0"),
        );
}

impl Fixtures {
    fn new() -> Self {
        Self::publishing(publish_standard)
    }

    fn nested() -> Self {
        Self::publishing(publish_nested)
    }

    /// Both trees at once: a bin to link, a conflict to nest, and three
    /// depths to order, so a random kill has every kind of state to land in.
    fn chaos() -> Self {
        Self::publishing(|registry| {
            publish_standard(registry);
            publish_nested(registry);
        })
    }

    fn publishing(publish: impl FnOnce(&mut FixtureRegistry)) -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut registry = FixtureRegistry::new(directory.path().join("registry"));
        publish(&mut registry);
        Self {
            _directory: directory,
            registry,
        }
    }
}

const STANDARD_DEPENDENCIES: &[(&str, &str)] =
    &[("a", "^1.0.0"), ("b", "^1.0.0"), ("tool", "^1.0.0")];
const NESTED_DEPENDENCIES: &[(&str, &str)] = &[("x", "^1.0.0"), ("y", "^1.0.0"), ("z", "^1.0.0")];
const DEEPEST_MARKER: &str = "node_modules/x/node_modules/y/node_modules/z/.opal-package";

/// A project plus its own cache, so each trial starts cold.
struct World {
    _directory: tempfile::TempDir,
    project: PathBuf,
    cache: PathBuf,
    registry_url: String,
}

impl World {
    fn new(fixtures: &Fixtures) -> Self {
        Self::depending_on(fixtures, STANDARD_DEPENDENCIES)
    }

    fn depending_on(fixtures: &Fixtures, dependencies: &[(&str, &str)]) -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let project = directory.path().join("project");
        let dependencies: serde_json::Map<String, serde_json::Value> = dependencies
            .iter()
            .map(|(name, spec)| ((*name).to_string(), serde_json::json!(spec)))
            .collect();
        write_project(
            &project,
            serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": dependencies
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

    /// Starts an install and SIGKILLs it after `delay`, wherever it is by
    /// then. It may already have finished, which is one of the moments a
    /// random delay is supposed to sample.
    fn install_killed_after(&self, delay: Duration) {
        let mut child = self
            .command()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn opal install");
        std::thread::sleep(delay);
        let _ = child.kill();
        child.wait().expect("reap");
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
fn test_a_kill_while_linking_a_nested_tree_converges() {
    // Linking runs one nesting depth at a time, threads within a depth. A
    // package materialized too early would clear a directory another package
    // is already nested in, and the failure is a silently missing package, so
    // the comparison has to reach three levels down.
    let fixtures = Fixtures::nested();
    let reference = World::depending_on(&fixtures, NESTED_DEPENDENCIES);
    reference.install();
    let expected = reference.snapshot();
    assert!(
        expected.contains_key(DEEPEST_MARKER),
        "the fixture no longer nests three levels deep"
    );

    for point in ["pm-mid-link", "pm-between-packages"] {
        let world = World::depending_on(&fixtures, NESTED_DEPENDENCIES);
        assert!(world.install_killed_at(point), "{point} was never reached");

        world.install();
        let recovered = world.snapshot();
        assert!(
            recovered.contains_key(DEEPEST_MARKER),
            "{point}: the deepest nested package is missing after re-running"
        );
        assert_eq!(
            recovered, expected,
            "{point}: re-running did not converge on the clean state"
        );
    }
}

/// splitmix64. Enough randomness to spread kill moments, and small enough
/// that a failing run replays exactly from the seed it prints.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }
}

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

#[test]
fn test_kills_at_random_moments_converge() {
    // Replay a failure with the OPAL_CHAOS_SEED it prints; run longer with
    // OPAL_CHAOS_TRIALS.
    let seed = env_number("OPAL_CHAOS_SEED").unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or_default()
    });
    let trials = env_number("OPAL_CHAOS_TRIALS").unwrap_or(12);
    let mut random = SplitMix64(seed);

    let fixtures = Fixtures::chaos();
    let dependencies = [STANDARD_DEPENDENCIES, NESTED_DEPENDENCIES].concat();
    let reference = World::depending_on(&fixtures, &dependencies);
    let started = Instant::now();
    reference.install();
    // Anywhere in a whole cold install, and a little past its end, so a run
    // that finishes before the kill arrives is sampled too.
    let window = started.elapsed().as_micros() as u64 * 5 / 4;
    let expected = reference.snapshot();

    for trial in 0..trials {
        let world = World::depending_on(&fixtures, &dependencies);
        // Up to three kills in a row, so one can land in the recovery from
        // the last.
        let delays: Vec<Duration> = (0..1 + random.below(3))
            .map(|_| Duration::from_micros(random.below(window)))
            .collect();
        let context = format!("OPAL_CHAOS_SEED={seed}, trial {trial}, killed after {delays:?}");

        for delay in &delays {
            world.install_killed_after(*delay);
            assert!(
                world.cache_is_clean(),
                "{context}: cache failed verification"
            );
        }
        world.install();
        assert!(
            world.cache_is_clean(),
            "{context}: cache dirty after recovery"
        );
        assert_eq!(
            world.snapshot(),
            expected,
            "{context}: re-running did not converge on the clean state"
        );
    }
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
