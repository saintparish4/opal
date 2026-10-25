//! `opal run` and `opal x` behaviour that npm can't be the oracle for, or
//! that needs a registry: `npm-run-cross-check.rs` holds `opal run` to npm.
//!
//! `opal x` runs against a file-backed registry, so these run offline.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use opal_pm::fixtures::{FixtureRegistry, Package};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

fn x(cwd: &Path, cache: &Path, registry: &FixtureRegistry, args: &[&str]) -> Output {
    Command::new(OPAL)
        .current_dir(cwd)
        .args(["x", "--cache-dir"])
        .arg(cache)
        .arg("--registry")
        .arg(registry.url())
        .args(args)
        .output()
        .expect("run opal x")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A package whose one command prints its name and arguments, one per line.
fn echoing(name: &str, version: &str, command: &str) -> Package {
    Package::new(name, version)
        .executable(
            "cli.sh",
            &format!(
                "#!/bin/sh\necho \"{command} {version}\"\nfor a in \"$@\"; do echo \"[$a]\"; done\n"
            ),
        )
        .bin(command, "cli.sh")
}

#[test]
fn x_installs_a_package_once_and_runs_its_command_with_the_arguments_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = FixtureRegistry::new(dir.path().join("registry"));
    registry.publish(echoing("greeter", "1.0.0", "greet"));
    let cache = dir.path().join("cache");

    let first = x(
        dir.path(),
        &cache,
        &registry,
        &["greeter", "a b", "$HOME", "--flag"],
    );
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert_eq!(
        text(&first.stdout),
        "greet 1.0.0\n[a b]\n[$HOME]\n[--flag]\n"
    );
    assert!(text(&first.stderr).contains("installing greeter"));

    // The second run uses what the first installed, and needs no registry.
    let second = Command::new(OPAL)
        .current_dir(dir.path())
        .args(["x", "--offline", "--cache-dir"])
        .arg(&cache)
        .args(["greeter", "again"])
        .output()
        .unwrap();
    assert!(second.status.success(), "{}", text(&second.stderr));
    assert_eq!(text(&second.stdout), "greet 1.0.0\n[again]\n");
    assert!(!text(&second.stderr).contains("installing"));
}

#[test]
fn x_takes_a_version_and_installs_each_spec_on_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = FixtureRegistry::new(dir.path().join("registry"));
    registry
        .publish(echoing("greeter", "1.0.0", "greet"))
        .publish(echoing("greeter", "2.0.0", "greet"));
    let cache = dir.path().join("cache");
    let old = x(dir.path(), &cache, &registry, &["greeter@1.0.0"]);
    let new = x(dir.path(), &cache, &registry, &["greeter@^2"]);
    assert_eq!(text(&old.stdout), "greet 1.0.0\n", "{}", text(&old.stderr));
    assert_eq!(text(&new.stdout), "greet 2.0.0\n", "{}", text(&new.stderr));
}

#[test]
fn x_prefers_a_command_already_in_node_modules_bin() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FixtureRegistry::new(dir.path().join("registry"));
    let bin = dir.path().join("node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("tsc"),
        "#!/bin/sh\necho local tsc \"$npm_lifecycle_event\"\n",
    )
    .unwrap();
    std::fs::set_permissions(bin.join("tsc"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let nested = dir.path().join("src");
    std::fs::create_dir_all(&nested).unwrap();

    // Found from a subfolder, without asking the (empty) registry anything.
    let output = x(&nested, &dir.path().join("cache"), &registry, &["tsc"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "local tsc npx\n");
}

#[test]
fn x_picks_the_command_named_after_the_package_when_there_are_several() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = FixtureRegistry::new(dir.path().join("registry"));
    registry.publish(
        Package::new("@acme/tool", "1.0.0")
            .executable("a.sh", "#!/bin/sh\necho right\n")
            .executable("b.sh", "#!/bin/sh\necho wrong\n")
            .bin("tool", "a.sh")
            .bin("tool-helper", "b.sh"),
    );
    let output = x(
        dir.path(),
        &dir.path().join("cache"),
        &registry,
        &["@acme/tool"],
    );
    assert_eq!(text(&output.stdout), "right\n", "{}", text(&output.stderr));
}

#[test]
fn x_passes_on_the_commands_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = FixtureRegistry::new(dir.path().join("registry"));
    registry.publish(
        Package::new("fails", "1.0.0")
            .executable("cli.sh", "#!/bin/sh\nexit 7\n")
            .bin("fails", "cli.sh"),
    );
    let output = x(dir.path(), &dir.path().join("cache"), &registry, &["fails"]);
    assert_eq!(output.status.code(), Some(7), "{}", text(&output.stderr));
}

#[test]
fn run_passes_a_sigterm_sent_to_opal_on_to_the_script() {
    // A pre-script makes opal wait on each script instead of exec'ing the
    // shell, which is the case that needs forwarding.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"scripts": {"preserve": "true", "serve": "trap 'echo got-term; exit 0' TERM; echo ready; while :; do sleep 0.05; done"}}"#,
    )
    .unwrap();
    let mut child = Command::new(OPAL)
        .current_dir(dir.path())
        .args(["run", "serve"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut seen = Vec::new();
    let mut buffer = [0u8; 64];
    use std::io::Read;
    while !String::from_utf8_lossy(&seen).contains("ready") {
        let n = stdout.read(&mut buffer).unwrap();
        assert!(n > 0, "script ended before it was ready");
        seen.extend_from_slice(&buffer[..n]);
    }
    std::thread::sleep(Duration::from_millis(50));
    // SAFETY: signalling a child this test started.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    stdout.read_to_end(&mut seen).unwrap();
    let status = child.wait().unwrap();
    assert!(String::from_utf8_lossy(&seen).contains("got-term"));
    assert_eq!(status.code(), Some(0), "signal {:?}", status.signal());
}

#[test]
fn run_lists_the_scripts_with_no_name_given() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name": "app", "version": "1.0.0", "scripts": {"build": "tsc", "test": "jest"}}"#,
    )
    .unwrap();
    let listed = Command::new(OPAL)
        .current_dir(dir.path())
        .arg("run")
        .output()
        .unwrap();
    assert_eq!(
        text(&listed.stdout),
        "Scripts in app@1.0.0:\n  build\n    tsc\n  test\n    jest\n"
    );
}
