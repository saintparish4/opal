//! Every integration test of this crate, in one test binary.
//!
//! Each file in tests/ would otherwise be a binary of its own, each one
//! linking the crate and all its dependencies again and taking about 100 MB
//! of target/. The files stay where they are, each a module here, so a
//! file's tests are still run by naming it as a filter:
//!
//! ```text
//! cargo test -p opal-cli --test integration -- build_output::
//! ```

#[path = "build-output.rs"]
mod build_output;

#[path = "cache-corruption.rs"]
mod cache_corruption;

#[path = "deep-nesting.rs"]
mod deep_nesting;

#[path = "install-crash-safety.rs"]
mod install_crash_safety;

#[path = "install-output.rs"]
mod install_output;

#[path = "install-relative-root.rs"]
mod install_relative_root;

#[path = "node-cross-check.rs"]
mod node_cross_check;

#[path = "npm-compatibility.rs"]
mod npm_compatibility;

#[path = "npm-run-cross-check.rs"]
mod npm_run_cross_check;

#[path = "run-and-x.rs"]
mod run_and_x;

#[path = "run-file.rs"]
mod run_file;

#[path = "upgrade.rs"]
mod upgrade;

/// A file in tests/ that isn't a module here would never be compiled, so
/// its tests would silently stop running.
#[test]
fn every_test_file_is_a_module_here() {
    let this = include_str!("integration.rs");
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for entry in std::fs::read_dir(tests).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.ends_with(".rs") && name != "integration.rs" {
            assert!(
                this.contains(&format!("#[path = \"{name}\"]")),
                "tests/{name} is not a module in tests/integration.rs"
            );
        }
    }
}
