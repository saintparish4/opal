//! The differential test: each fixture is run unbundled and bundled under
//! Node, and the two runs must be indistinguishable by `SEMANTICS.md`'s
//! definition. Needs `node` (24 or newer), so it is `#[ignore]`d in the
//! ordinary suite and run by its own CI job:
//!
//! ```text
//! cargo test -p opal-bundler --test differential -- --ignored --nocapture
//! ```
//!
//! `OPAL_FIXTURE=<text>` runs only the fixtures whose names contain it, and
//! `OPAL_NODE` names the node binary.

mod support;

use std::path::{Path, PathBuf};

use opal_bundler::oracle::Node;
use support::{Fixture, OUT_DIR};

/// Until the linker exists, a fixture's bundle is the one written by hand
/// next to it, in `hand-bundle.mjs`.
fn hand_bundle(fixture: &Fixture, program: &Path) -> Result<PathBuf, (String, String)> {
    let source = fixture.dir.join("hand-bundle.mjs");
    if !source.is_file() {
        return Err((
            "NO-BUNDLER".to_string(),
            "no hand-written bundle, and no bundler yet".to_string(),
        ));
    }
    let out = program.join(OUT_DIR);
    std::fs::create_dir_all(&out).unwrap();
    let stem = Path::new(&fixture.entry)
        .file_stem()
        .unwrap()
        .to_string_lossy();
    let bundle = out.join(format!("{stem}.mjs"));
    std::fs::copy(&source, &bundle).unwrap();
    Ok(bundle)
}

#[test]
#[ignore = "needs node; run with --ignored"]
fn test_every_fixture_matches_its_expectation() {
    let fixtures = support::load_all();
    assert!(!fixtures.is_empty(), "no fixtures matched");
    let failed = support::run_all(&fixtures, &hand_bundle);
    assert_eq!(failed, 0, "{failed} fixture(s) failed; see the table above");
}

/// The oracle's own checks, at the level of a single observation.
#[test]
#[ignore = "needs node; run with --ignored"]
fn test_oracle_observes_an_uncaught_error_without_its_trace() {
    let fixture = support::load(&support::fixtures_dir().join("oracle-two-file"));
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("program");
    support::copy_program(&fixture, &program);

    let seen = Node::default()
        .observe(
            &program,
            &program.join("main.mjs"),
            &temp.path().join("oracle"),
        )
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&seen.stdout),
        "lib evaluated\nmain start 0\nafter bump 1\n"
    );
    assert_eq!(String::from_utf8_lossy(&seen.stderr), "to stderr\n");
    assert_eq!(seen.exit_code, Some(1));
    let uncaught = seen.uncaught.expect("the throw is uncaught");
    // Node 24 evaluates an ES module as a promise, so a throw at its top
    // level reaches the process as a rejection.
    assert_eq!(uncaught.origin, "unhandledRejection");
    assert_eq!(uncaught.name.as_deref(), Some("TypeError"));
    assert_eq!(uncaught.message.as_deref(), Some("done"));
}

#[test]
#[ignore = "needs node; run with --ignored"]
fn test_oracle_keeps_output_after_a_handled_exception() {
    let fixture = support::load(&support::fixtures_dir().join("oracle-handled-exception"));
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("program");
    support::copy_program(&fixture, &program);

    let seen = Node::default()
        .observe(
            &program,
            &program.join("main.mjs"),
            &temp.path().join("oracle"),
        )
        .unwrap();
    assert_eq!(seen.uncaught, None);
    assert_eq!(
        String::from_utf8_lossy(&seen.stderr),
        "handled: boom\nstill running\n"
    );
    assert_eq!(seen.exit_code, Some(3));
}

#[test]
#[ignore = "needs node; run with --ignored"]
fn test_oracle_sees_an_unhandled_rejection() {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("program");
    std::fs::create_dir_all(&program).unwrap();
    std::fs::write(
        program.join("main.mjs"),
        "console.log('before');\nPromise.reject(new SyntaxError('late'));\n",
    )
    .unwrap();
    let seen = Node::default()
        .observe(
            &program,
            &program.join("main.mjs"),
            &temp.path().join("oracle"),
        )
        .unwrap();
    let uncaught = seen.uncaught.expect("an unhandled rejection is reported");
    assert_eq!(uncaught.origin, "unhandledRejection");
    assert_eq!(uncaught.name.as_deref(), Some("SyntaxError"));
    assert_eq!(uncaught.message.as_deref(), Some("late"));
    assert_eq!(String::from_utf8_lossy(&seen.stdout), "before\n");
    assert!(
        seen.stderr.is_empty(),
        "the trace is cut: {:?}",
        seen.stderr
    );
}
