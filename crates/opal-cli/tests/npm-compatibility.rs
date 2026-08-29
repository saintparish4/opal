//! The npm compatibility suite (`testing_strategy.md` §5).
//!
//! Curated by the edge case each package exercises, not by download count. A
//! "top 50 by downloads" list skews toward well-behaved packages and misses
//! exactly what breaks real installs — `exports` map quirks, platform-gated
//! native binaries, optional peers, unusual `package.json` shapes.
//!
//! **Install and execute are separate.** An install failure is a
//! registry/resolution bug; a run failure is a runtime/resolver bug. They have
//! different triage paths, so they are different tests and different CI jobs:
//!
//! ```bash
//! cargo test -p opal-cli --test npm-compatibility -- --ignored test_install
//! cargo test -p opal-cli --test npm-compatibility -- --ignored test_execute
//! ```
//!
//! Every test is `#[ignore]`: these reach the public registry, and `cargo test`
//! is meant to run offline in seconds. CI runs them with `--ignored`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Shared across the suite so one run downloads each package once. Under
/// `target/`, which is already ignored, and safe to delete at any time.
fn cache() -> PathBuf {
    let cache = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/compat-cache")
        .canonicalize()
        .unwrap_or_else(|_| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/compat-cache");
            std::fs::create_dir_all(&path).expect("create the shared cache");
            path
        });
    std::fs::create_dir_all(&cache).expect("create the shared cache");
    cache
}

struct Case {
    directory: tempfile::TempDir,
}

impl Case {
    fn new(dependencies: serde_json::Value) -> Self {
        Self::with_manifest(serde_json::json!({
            "name": "compat",
            "version": "1.0.0",
            "dependencies": dependencies,
        }))
    }

    fn with_manifest(manifest: serde_json::Value) -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            directory.path().join("package.json"),
            serde_json::to_vec_pretty(&manifest).expect("serializable"),
        )
        .expect("write package.json");
        Self { directory }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.directory.path().join(relative)
    }

    fn install(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_opal"))
            .arg("install")
            .arg("--root")
            .arg(self.directory.path())
            .env("OPAL_CACHE_DIR", cache())
            .output()
            .expect("run opal install")
    }

    /// Installs, failing the test with the binary's own diagnostics if it did
    /// not — an exit code alone is not a triageable report.
    fn installed(&self) -> &Self {
        let output = self.install();
        assert!(
            output.status.success(),
            "opal install failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        self
    }

    fn node(&self, script: &str) -> Output {
        Command::new("node")
            .arg("--input-type=module")
            .arg("-e")
            .arg(script)
            .current_dir(self.directory.path())
            .output()
            .expect("run node")
    }

    /// Runs a script against the installed tree and returns its stdout.
    fn ran(&self, script: &str) -> String {
        let output = self.node(script);
        assert!(
            output.status.success(),
            "node failed against the installed tree\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}

// ---------------------------------------------------------------- install --

#[test]
#[ignore = "reaches the public registry"]
fn test_install_platform_gated_native_binaries() {
    // 25 platform variants declared as optionalDependencies, 256 MB of them.
    // Exactly one belongs on this host and the rest must be recorded and
    // skipped, not downloaded.
    let case = Case::new(serde_json::json!({ "esbuild": "0.25.0" }));
    case.installed();

    let variants: Vec<String> = std::fs::read_dir(case.path("node_modules/@esbuild"))
        .expect("@esbuild is installed")
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .collect();
    assert_eq!(variants.len(), 1, "one host, one binary: {variants:?}");

    let lockfile = std::fs::read_to_string(case.path("opal.lock")).expect("opal.lock");
    let recorded = lockfile
        .lines()
        .filter(|l| l.starts_with("pkg @esbuild/"))
        .count();
    assert!(
        recorded > 20,
        "every variant stays in the lockfile so it is portable, found {recorded}"
    );
}

#[test]
#[ignore = "reaches the public registry"]
fn test_install_an_optional_dependency_for_another_platform_only() {
    // fsevents is darwin-only, and half of npm depends on it *optionally* —
    // chokidar, jest, vite. On Linux it must be absent without failing
    // anything, which is what makes those installs work at all.
    let case = Case::with_manifest(serde_json::json!({
        "name": "compat",
        "version": "1.0.0",
        "optionalDependencies": { "fsevents": "2.3.3" },
    }));
    case.installed();

    let present = case.path("node_modules/fsevents").exists();
    assert_eq!(
        present,
        cfg!(target_os = "macos"),
        "fsevents belongs on darwin and nowhere else"
    );
}

#[test]
#[ignore = "reaches the public registry"]
fn test_install_refuses_a_required_dependency_this_platform_cannot_run() {
    // The other half of the same rule, and npm's own behaviour: declared as a
    // plain dependency rather than an optional one, a darwin-only package on
    // Linux is `EBADPLATFORM`, because skipping it silently produces a tree
    // that cannot work. Verified against `npm install` directly.
    if cfg!(target_os = "macos") {
        return;
    }
    let case = Case::new(serde_json::json!({ "fsevents": "2.3.3" }));
    let output = case.install();

    assert!(
        !output.status.success(),
        "a required dependency cannot be skipped"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires os darwin"), "{stderr}");
}

#[test]
#[ignore = "reaches the public registry"]
fn test_install_a_deep_transitive_tree() {
    // ~70 packages, CJS throughout, the shape most real apps have.
    let case = Case::new(serde_json::json!({ "express": "4.21.2" }));
    case.installed();
    assert!(case.path("node_modules/express/package.json").is_file());
    assert!(
        case.path("node_modules/accepts").is_file() || case.path("node_modules/accepts").is_dir(),
        "transitive dependencies hoist to the top level"
    );
}

#[test]
#[ignore = "reaches the public registry"]
fn test_install_a_scoped_package_tree() {
    // Scoped names are a directory layer in node_modules and an escape in the
    // registry URL; @babel/core brings a large tree of them.
    let case = Case::new(serde_json::json!({ "@babel/core": "7.26.0" }));
    case.installed();
    assert!(case.path("node_modules/@babel/core/package.json").is_file());
}

#[test]
#[ignore = "reaches the public registry"]
fn test_install_a_package_with_an_extensionless_bin_script() {
    // typescript is one large package whose bins are extensionless shebang
    // scripts — the shape that used to walk as zero edges.
    let case = Case::new(serde_json::json!({ "typescript": "5.7.3" }));
    case.installed();
    assert!(case.path("node_modules/typescript/bin/tsc").is_file());
    assert!(
        case.path("node_modules/.bin/tsc").exists(),
        "bin shim linked"
    );
}

#[test]
#[ignore = "reaches the public registry"]
fn test_install_an_optional_peer_that_is_legitimately_absent() {
    // debug declares supports-color as an optional peer. Absent is correct and
    // must not read as a broken tree.
    let case = Case::new(serde_json::json!({ "debug": "4.4.0" }));
    case.installed();
    assert!(case.path("node_modules/debug/package.json").is_file());
}

#[test]
#[ignore = "reaches the public registry"]
fn test_install_an_alias_specifier() {
    // `@isaacs/cliui` declares `string-width@^5` *and* `string-width-cjs`, an
    // alias for `string-width@^4` — one package depending on two majors of
    // another, which is what aliases are for. Everything under `glob@10.x`
    // carries it: rimraf@5, node-gyp@10, sucrase.
    let case = Case::new(serde_json::json!({ "rimraf": "5.0.10" }));
    case.installed();

    for alias in ["string-width-cjs", "strip-ansi-cjs", "wrap-ansi-cjs"] {
        assert!(
            case.path(&format!("node_modules/{alias}")).is_dir(),
            "{alias} is a directory name, not a package name"
        );
    }

    let manifest = std::fs::read_to_string(case.path("node_modules/string-width-cjs/package.json"))
        .expect("the alias directory holds the aliased package");
    assert!(
        manifest.contains("\"name\": \"string-width\""),
        "{manifest}"
    );
    // The unaliased major sits beside it rather than being displaced by it.
    assert!(case.path("node_modules/string-width").is_dir());
}

// ---------------------------------------------------------------- execute --

#[test]
#[ignore = "reaches the public registry"]
fn test_execute_a_tree_containing_aliased_majors() {
    // Two majors of one package in one tree, reached under different names.
    // Installing them is not the same as resolving through them at run time.
    let case = Case::new(serde_json::json!({ "glob": "10.4.5" }));
    case.installed();
    // Read off disk rather than through module resolution: `string-width@5`'s
    // `exports` map deliberately does not expose `./package.json`, and Node is
    // right to refuse it.
    let output = case.ran(
        "import { readFileSync } from 'node:fs';\
         import { createRequire } from 'node:module';\
         const require = createRequire(process.cwd() + '/');\
         const at = (p) => JSON.parse(readFileSync(`node_modules/${p}/package.json`, 'utf8'));\
         const four = at('string-width-cjs');\
         const five = at('string-width');\
         const { globSync } = require('glob');\
         process.stdout.write(`${four.name}:${four.version[0]}${five.version[0]}${globSync('package.json').length}`);",
    );
    assert_eq!(
        output, "string-width:451",
        "both majors are string-width, at 4 and 5, and glob runs through them"
    );
}

#[test]
#[ignore = "reaches the public registry"]
fn test_execute_a_deep_commonjs_tree() {
    let case = Case::new(serde_json::json!({ "express": "4.21.2" }));
    case.installed();
    let output = case.ran(
        "import { createRequire } from 'node:module';\
         const require = createRequire(process.cwd() + '/');\
         const express = require('express');\
         process.stdout.write(typeof express);",
    );
    assert_eq!(output, "function");
}

#[test]
#[ignore = "reaches the public registry"]
fn test_execute_a_pure_esm_package_with_an_exports_map() {
    // chalk@5 is ESM-only with an `exports` map and no `main`, so requiring it
    // fails by design and importing it must work.
    let case = Case::new(serde_json::json!({ "chalk": "5.4.1" }));
    case.installed();
    let output = case.ran(
        "const { default: chalk } = await import('chalk');\
         process.stdout.write(typeof chalk.red);",
    );
    assert_eq!(output, "function");
}

#[test]
#[ignore = "reaches the public registry"]
fn test_execute_a_subpath_export() {
    // semver publishes a dozen subpath entries in its exports map; reaching one
    // exercises resolution beyond the package root.
    let case = Case::new(serde_json::json!({ "semver": "7.6.3" }));
    case.installed();
    let output = case.ran(
        "const { default: gt } = await import('semver/functions/gt.js');\
         process.stdout.write(String(gt('2.0.0', '1.0.0')));",
    );
    assert_eq!(output, "true");
}

#[test]
#[ignore = "reaches the public registry"]
fn test_execute_a_conditional_export_picks_the_node_build() {
    // readable-stream ships browser and node builds behind export conditions.
    let case = Case::new(serde_json::json!({ "readable-stream": "4.5.2" }));
    case.installed();
    let output = case.ran(
        "const { Readable } = await import('readable-stream');\
         process.stdout.write(typeof Readable);",
    );
    assert_eq!(output, "function");
}

#[test]
#[ignore = "reaches the public registry"]
fn test_execute_a_native_binary_from_a_platform_gated_optional() {
    // The one variant that was installed has to actually run: this is the
    // difference between "resolved the right package" and "linked a usable
    // binary", and only executing it tells them apart.
    let case = Case::new(serde_json::json!({ "esbuild": "0.25.0" }));
    case.installed();

    let binary = case.path("node_modules/.bin/esbuild");
    let output = Command::new(&binary)
        .arg("--version")
        .output()
        .expect("run esbuild");
    assert!(
        output.status.success(),
        "the installed native binary does not run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0.25.0");
}

#[test]
#[ignore = "reaches the public registry"]
fn test_execute_an_extensionless_bin_script() {
    let case = Case::new(serde_json::json!({ "typescript": "5.7.3" }));
    case.installed();

    let output = Command::new("node")
        .arg(case.path("node_modules/typescript/bin/tsc"))
        .arg("--version")
        .output()
        .expect("run tsc");
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("5.7.3"),
        "tsc did not report its version"
    );
}
