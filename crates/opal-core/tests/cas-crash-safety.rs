//! crash-safety exit criterion: a SIGKILL during a CAS write never
//! leaves a corrupt entry, and re-running converges.
//!
//! The kill is delivered to a real child process at a fault point it announces
//! on stderr, so the timing is exact rather than hopeful, and the child runs no
//! destructor on the way out — which is the whole point.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use opal_core::cas::{Cas, FAULT_BEFORE_RENAME, FAULT_MID_WRITE};
use opal_core::fault::{FAULT_ENV, FaultPoint, READY_MARKER};
use opal_core::hash::ContentHash;

const PROBE: &str = env!("CARGO_BIN_EXE_cas-crash-probe");

/// Big enough that the mid-write fault point lands with bytes already on disk
/// and the write still in progress.
const PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

struct Fixture {
    _directory: tempfile::TempDir,
    cas_root: std::path::PathBuf,
    payload: std::path::PathBuf,
    expected: ContentHash,
}

fn fixture(seed: u8) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let cas_root = directory.path().join("cas");
    let payload = directory.path().join("payload.bin");

    let bytes: Vec<u8> = (0..PAYLOAD_BYTES)
        .map(|index| (index as u8).wrapping_add(seed))
        .collect();
    std::fs::write(&payload, &bytes).unwrap();

    Fixture {
        _directory: directory,
        cas_root,
        payload,
        expected: ContentHash::of(&bytes),
    }
}

/// Runs the probe, waits for it to park at `point`, and SIGKILLs it there.
fn kill_at(fixture: &Fixture, point: FaultPoint) {
    let mut child: Child = Command::new(PROBE)
        .arg(&fixture.cas_root)
        .arg(&fixture.payload)
        .env(FAULT_ENV, point.name())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn probe");

    let stderr = child.stderr.take().expect("probe stderr");
    let mut reached = false;
    for line in BufReader::new(stderr).lines() {
        let line = line.expect("read probe stderr");
        if line.starts_with(READY_MARKER) {
            reached = true;
            break;
        }
    }
    assert!(
        reached,
        "probe exited without reaching {} — the fault point moved",
        point.name()
    );

    // Child::kill is SIGKILL on Unix: no unwinding, no Drop, no flush.
    child.kill().expect("kill probe");
    let status = child.wait().expect("reap probe");
    assert!(!status.success(), "probe should not have finished");
}

fn run_to_completion(fixture: &Fixture) -> ContentHash {
    let output = Command::new(PROBE)
        .arg(&fixture.cas_root)
        .arg(&fixture.payload)
        .output()
        .expect("run probe");
    assert!(
        output.status.success(),
        "probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    ContentHash::parse_hex(String::from_utf8_lossy(&output.stdout).trim()).expect("probe hash")
}

fn open(fixture: &Fixture) -> Cas {
    Cas::open(&fixture.cas_root).expect("open cas")
}

#[test]
fn test_kill_before_rename_leaves_no_object() {
    let fixture = fixture(1);
    kill_at(&fixture, FAULT_BEFORE_RENAME);

    let cas = open(&fixture);
    assert!(
        !cas.contains(&fixture.expected),
        "an object appeared even though the rename never ran"
    );
    assert_eq!(cas.object_hashes().unwrap(), Vec::new());

    // The interrupted write is expected to leave exactly this behind: inert
    // garbage in tmp/, which `opal cache gc` sweeps.
    assert_eq!(cas.temp_files().unwrap().len(), 1);
    assert!(cas.audit().unwrap().is_clean());
}

#[test]
fn test_kill_mid_write_leaves_no_object() {
    let fixture = fixture(2);
    kill_at(&fixture, FAULT_MID_WRITE);

    let cas = open(&fixture);
    assert!(!cas.contains(&fixture.expected));
    assert!(cas.audit().unwrap().is_clean());
    assert_eq!(cas.temp_files().unwrap().len(), 1);
}

#[test]
fn test_rerunning_after_a_kill_converges_to_a_verified_store() {
    let fixture = fixture(3);
    kill_at(&fixture, FAULT_BEFORE_RENAME);

    let stored = run_to_completion(&fixture);
    assert_eq!(stored, fixture.expected);

    let cas = open(&fixture);
    assert_eq!(cas.object_hashes().unwrap(), vec![fixture.expected]);
    assert_eq!(cas.read(&fixture.expected).unwrap().len(), PAYLOAD_BYTES);

    let report = cas.audit().unwrap();
    assert!(report.is_clean(), "audit after recovery: {report:?}");
    assert_eq!(report.objects, 1);
}

#[test]
fn test_repeated_kills_never_produce_a_mismatched_entry() {
    let fixture = fixture(4);

    // Alternate the fault point across trials; every intermediate state must
    // still pass a full audit.
    for trial in 0..6 {
        let point = if trial % 2 == 0 {
            FAULT_MID_WRITE
        } else {
            FAULT_BEFORE_RENAME
        };
        kill_at(&fixture, point);

        let cas = open(&fixture);
        let report = cas.audit().unwrap();
        assert!(report.is_clean(), "trial {trial}: {report:?}");
        assert!(report.corrupt.is_empty());
    }

    assert_eq!(run_to_completion(&fixture), fixture.expected);

    let cas = open(&fixture);
    assert!(cas.audit().unwrap().is_clean());
    // Six killed writes, six orphans, one good object.
    assert_eq!(cas.temp_files().unwrap().len(), 6);
    assert_eq!(cas.object_hashes().unwrap(), vec![fixture.expected]);
}

#[test]
fn test_gc_sweeps_the_orphans_a_kill_left_behind() {
    use std::collections::BTreeSet;
    use std::time::Duration;

    use opal_core::cas::gc::{self, GcOptions};

    let fixture = fixture(5);
    kill_at(&fixture, FAULT_BEFORE_RENAME);
    let stored = run_to_completion(&fixture);

    let cas = open(&fixture);
    let options = GcOptions {
        temp_max_age: Duration::ZERO,
        ..GcOptions::default()
    };
    let report = gc::collect(&cas, &BTreeSet::from([stored]), &options).unwrap();

    assert_eq!(report.temp_files_removed, 1);
    assert_eq!(report.objects_removed, 0);
    assert!(cas.temp_files().unwrap().is_empty());
    assert!(cas.contains(&stored));
}

#[test]
fn test_probe_binary_exists_where_the_suite_expects_it() {
    assert!(Path::new(PROBE).is_file(), "probe binary missing: {PROBE}");
}
