//! `opal run` against JavaScript that frees memory a native call is using.
//!
//! The scripts are in `tests/memory-safety/`, so the nightly AddressSanitizer
//! job runs the same files: there a use-after-free is a report, here it would
//! only sometimes be a wrong answer. Unlike `run-file.rs`, the expected output
//! is not Node's. Node 24.19.0 has the bug each script describes, and what it
//! prints is the bug's result.

use std::path::Path;
use std::process::{Command, Stdio};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

fn run(script: &str) -> String {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/memory-safety")
        .join(script);
    let directory = tempfile::tempdir().expect("temp dir");
    let output = Command::new(OPAL)
        .current_dir(directory.path())
        .arg("run")
        .arg(&fixture)
        .stdin(Stdio::null())
        .output()
        .expect("run opal");
    assert!(
        output.status.success(),
        "{script}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_getter_that_frees_an_earlier_buffer_of_a_vectored_call_leaves_only_live_memory_used() {
    // op, bytes, the victim's length afterwards, and how often the getter ran.
    assert_eq!(run("vectored-sync.js"), "read 8 0 2\nwrite 8 0 2\n");
}

#[test]
fn a_pool_readv_whose_buffer_is_transferred_away_writes_only_its_own_memory() {
    assert_eq!(run("vectored-async.js"), "ok 4194304 true\n");
}
