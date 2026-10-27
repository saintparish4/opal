//! `opal run` against `npm run` on the same package.json.
//!
//! Each case writes a project, runs one script through both, and requires the
//! same stdout and the same exit status (code, or the signal it died of). The
//! `env` cases compare the whole environment the script saw, minus what
//! [`npm_only`] and [`differs_by_design`] name: the variables npm exports for
//! its own settings, and the ones whose value has to name the tool that ran
//! the script. Anything else that differs fails the test, so a variable opal
//! forgets, or one it sets that npm doesn't, shows up here by name.
//!
//! Needs `npm` on PATH, which is why the tests are `#[ignore]`d in the
//! ordinary suite; CI runs them in their own job:
//!
//! ```text
//! cargo test -p opal-cli --test integration -- --ignored npm_run_cross_check::
//! ```

use std::collections::BTreeMap;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");

/// What one run produced that the two tools must agree on.
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    stdout: String,
    code: Option<i32>,
    signal: Option<i32>,
}

fn observe(output: &Output) -> Observed {
    Observed {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        code: output.status.code(),
        signal: output.status.signal(),
    }
}

/// The environment both tools start from: the test's own PATH (for sh, node,
/// and npm), a HOME, and one variable of the caller's to see it carried
/// through.
fn base(command: &mut Command) -> &mut Command {
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", std::env::var_os("HOME").unwrap_or_default())
        .env("CALLER_SET", "kept")
        .env("npm_config_update_notifier", "false")
}

fn npm(dir: &Path, script: &str, args: &[&str]) -> Output {
    let mut command = Command::new("npm");
    base(&mut command)
        .current_dir(dir)
        .args(["run", "--silent", script]);
    if !args.is_empty() {
        command.arg("--").args(args);
    }
    command.output().expect("run npm")
}

fn opal(dir: &Path, script: &str, args: &[&str]) -> Output {
    let mut command = Command::new(OPAL);
    base(&mut command)
        .current_dir(dir)
        .args(["run", script])
        .args(args);
    command.output().expect("run opal")
}

fn project(manifest: Value) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join("package.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    dir
}

#[track_caller]
fn same(dir: &Path, script: &str, args: &[&str]) -> Observed {
    let npm = observe(&npm(dir, script, args));
    let opal = observe(&opal(dir, script, args));
    assert_eq!(opal, npm, "opal run {script} {args:?} differs from npm run");
    opal
}

#[test]
#[ignore = "needs npm"]
fn pre_and_post_run_around_the_script_and_only_it_gets_arguments() {
    let dir = project(json!({
        "name": "order", "version": "1.0.0",
        "scripts": {
            "prego": "echo pre \"$@\"",
            "go": "echo main",
            "postgo": "echo post"
        }
    }));
    let seen = same(dir.path(), "go", &["one", "two"]);
    assert_eq!(seen.stdout, "pre\nmain one two\npost\n");
}

#[test]
#[ignore = "needs npm"]
fn arguments_reach_the_script_as_written() {
    let dir = project(json!({"scripts": {"show": "printf '[%s]\\n'"}}));
    same(
        dir.path(),
        "show",
        &[
            "a b",
            "$HOME",
            "it's",
            "",
            "*",
            "--flag=x",
            "'quoted'",
            "back\\slash",
            "~",
        ],
    );
}

#[test]
#[ignore = "needs npm"]
fn a_failing_script_sets_the_exit_code_and_stops_what_follows() {
    let dir = project(json!({"scripts": {
        "fails": "exit 3",
        "prestops": "exit 4", "stops": "echo never", "poststops": "echo never"
    }}));
    assert_eq!(same(dir.path(), "fails", &[]).code, Some(3));
    let stopped = same(dir.path(), "stops", &[]);
    assert_eq!(stopped.code, Some(4));
    assert_eq!(stopped.stdout, "");
}

#[test]
#[ignore = "needs npm"]
fn a_script_killed_by_a_signal_kills_the_runner_with_it() {
    let dir = project(json!({"scripts": {
        "dies": "kill -TERM $$",
        "predies2": "true", "dies2": "kill -TERM $$"
    }}));
    // Once with opal exec'ing the shell, once with it waiting on one.
    assert_eq!(same(dir.path(), "dies", &[]).signal, Some(libc::SIGTERM));
    assert_eq!(same(dir.path(), "dies2", &[]).signal, Some(libc::SIGTERM));
}

#[test]
#[ignore = "needs npm"]
fn missing_empty_and_non_string_scripts() {
    let dir = project(json!({"scripts": {"empty": "", "number": 1}}));
    assert_eq!(same(dir.path(), "missing", &[]).code, Some(1));
    assert_eq!(same(dir.path(), "number", &[]).code, Some(1));
    assert_eq!(
        same(dir.path(), "empty", &[]),
        Observed {
            stdout: String::new(),
            code: Some(0),
            signal: None
        }
    );
    let npm = Command::new("npm")
        .current_dir(dir.path())
        .args(["run", "--silent", "--if-present", "missing"])
        .output()
        .unwrap();
    let opal = Command::new(OPAL)
        .current_dir(dir.path())
        .args(["run", "--if-present", "missing"])
        .output()
        .unwrap();
    assert_eq!(observe(&opal), observe(&npm));
}

#[test]
#[ignore = "needs npm"]
fn bin_folders_from_the_project_up_are_on_path_and_scripts_run_in_the_project() {
    use std::os::unix::fs::PermissionsExt;
    let outer = tempfile::tempdir().unwrap();
    let write_bin = |dir: &Path, name: &str, says: &str| {
        let bin = dir.join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(name), format!("#!/bin/sh\necho {says}\n")).unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let app = outer.path().join("app");
    std::fs::create_dir_all(app.join("src/deep")).unwrap();
    std::fs::write(
        app.join("package.json"),
        r#"{"scripts": {"tools": "near && far && shadowed", "where": "pwd; echo $INIT_CWD"}}"#,
    )
    .unwrap();
    write_bin(&app, "near", "near");
    write_bin(&app, "shadowed", "from-app");
    write_bin(outer.path(), "far", "far");
    write_bin(outer.path(), "shadowed", "from-outer");

    assert_eq!(same(&app, "tools", &[]).stdout, "near\nfar\nfrom-app\n");
    // From a subfolder: the script runs in the project, INIT_CWD is where
    // the command was typed.
    let seen = same(&app.join("src/deep"), "where", &[]);
    assert!(seen.stdout.ends_with("src/deep\n"), "{}", seen.stdout);
}

/// npm's exports of its own configuration, which opal has no equivalent of.
fn npm_only(key: &str) -> bool {
    (key.starts_with("npm_config_")
        && key != "npm_config_local_prefix"
        && key != "npm_config_user_agent")
        || key == "EDITOR"
        || key == "_"
}

/// Present in both, with a value that names the tool: compared for presence
/// only.
fn differs_by_design(key: &str) -> bool {
    matches!(key, "npm_config_user_agent" | "npm_execpath")
}

fn env_map(stdout: &[u8]) -> BTreeMap<String, String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// npm also puts its bundled node-gyp shim folder on PATH, which opal doesn't
/// ship.
fn without_node_gyp(path: &str) -> String {
    path.split(':')
        .filter(|entry| !entry.ends_with("node-gyp-bin"))
        .collect::<Vec<_>>()
        .join(":")
}

#[track_caller]
fn same_env(dir: &Path, script: &str) {
    let npm_out = npm(dir, script, &[]);
    let opal_out = opal(dir, script, &[]);
    assert!(npm_out.status.success() && opal_out.status.success());
    let mut npm = env_map(&npm_out.stdout);
    let mut opal = env_map(&opal_out.stdout);
    npm.retain(|k, _| !npm_only(k));
    opal.retain(|k, _| !npm_only(k));
    for map in [&mut npm, &mut opal] {
        if let Some(path) = map.get_mut("PATH") {
            *path = without_node_gyp(path);
        }
        for key in ["npm_config_user_agent", "npm_execpath"] {
            if let Some(value) = map.get_mut(key) {
                *value = "<tool>".into();
            }
        }
    }
    let keys: std::collections::BTreeSet<_> = npm.keys().chain(opal.keys()).collect();
    let mut differences = Vec::new();
    for key in keys {
        let (n, o) = (npm.get(key), opal.get(key));
        if n != o {
            differences.push(format!("{key}: npm {n:?}, opal {o:?}"));
        }
    }
    assert!(
        differences.is_empty(),
        "environments differ:\n  {}",
        differences.join("\n  ")
    );
    assert!(differs_by_design("npm_execpath"));
}

#[test]
#[ignore = "needs npm"]
fn the_script_environment_matches_npms() {
    let dir = project(json!({
        "name": "@scope/env-check",
        "version": "v2.0.0",
        "description": "not exported",
        "config": {"port": 8080, "nested": {"list": [1, "two", null], "off": false}},
        "engines": {"node": ">=18"},
        "bin": "./cli.js",
        "scripts": {"dump": "env", "predump2": "true", "dump2": "env"}
    }));
    // The built-in `env`, an `env` script, and one run with a pre-script, so
    // opal's exec path and its spawn path are both checked.
    same_env(dir.path(), "env");
    same_env(dir.path(), "dump");
    same_env(dir.path(), "dump2");
}

#[test]
#[ignore = "needs npm"]
fn the_environment_matches_for_a_package_with_no_name_or_version() {
    let dir =
        project(json!({"scripts": {"dump": "env"}, "bin": {"a": "a.js", "../b": "../../b.js"}}));
    same_env(dir.path(), "dump");
}

#[test]
#[ignore = "needs npm and the public registry"]
fn x_runs_a_real_package_the_way_npx_does() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("opal-cache");
    let args = ["cowsay@1.6.0", "hello from", "$both"];
    let npx = base(&mut Command::new("npx"))
        .current_dir(dir.path())
        .env("npm_config_cache", dir.path().join("npm-cache"))
        .arg("--yes")
        .args(args)
        .output()
        .expect("run npx");
    let opal = base(&mut Command::new(OPAL))
        .current_dir(dir.path())
        .args(["x", "--cache-dir"])
        .arg(&cache)
        .args(args)
        .output()
        .expect("run opal x");
    assert!(
        npx.status.success(),
        "{}",
        String::from_utf8_lossy(&npx.stderr)
    );
    assert_eq!(observe(&opal), observe(&npx));
}
