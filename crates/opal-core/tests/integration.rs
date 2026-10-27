//! Every integration test of this crate, in one test binary.
//!
//! Each file in tests/ would otherwise be a binary of its own, each one
//! linking the crate and all its dependencies again and taking about 100 MB
//! of target/. The files stay where they are, each a module here, so a
//! file's tests are still run by naming it as a filter:
//!
//! ```text
//! cargo test -p opal-core --test integration -- cache_invalidation::
//! ```

/// Helpers the test files share.
mod support;

#[path = "cache-invalidation.rs"]
mod cache_invalidation;

#[path = "cas-crash-safety.rs"]
mod cas_crash_safety;

#[path = "exports-properties.rs"]
mod exports_properties;

#[path = "graph-resolution.rs"]
mod graph_resolution;

#[path = "node-resolution-cross-check.rs"]
mod node_resolution_cross_check;

#[path = "node-resolution.rs"]
mod node_resolution;

#[path = "strip-cross-check.rs"]
mod strip_cross_check;

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
