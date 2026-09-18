//! Property: whatever a package's `exports` resolves a specifier to lies inside
//! that package.
//!
//! This is Node's rule, and a published package controls both halves of the
//! question: its own `exports` map, and (through `import` statements in any
//! other package) the specifiers pointed at it.
//!
//! An escape takes a specific shape: a target that climbs with `../` on its
//! own, or a `*` pattern whose captured part climbs. Uniformly random
//! segments hit those shapes about once in thousands of cases, which is why a
//! first version of this test passed against the resolver it was written to
//! catch. So the shapes are generated on purpose, next to random segments for
//! breadth.

mod support;

use opal_core::graph::DependencyTarget;
use opal_core::graph::resolver::{ResolverOptions, resolve};
use opal_core::path::NormalizedPath;
use proptest::prelude::*;
use proptest::sample::select;
use support::{Project, entry};

/// Every segment that can move a path, plus names that exist on disk.
const SEGMENTS: &[&str] = &["a", "a.js", "lib", "deep", "..", ".", "", "node_modules"];
/// Files that exist both inside the package and outside it.
const NAMES: &[&str] = &["a", "a.js", "lib/a", "lib/a.js"];
const KEYS: &[&str] = &[".", "./a", "./*", "./lib/*", "./*.js"];

fn soup(max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(select(SEGMENTS), 0..=max).prop_map(|parts| parts.join("/"))
}

/// Zero to three `../`.
fn climb() -> impl Strategy<Value = String> {
    (0usize..=3).prop_map(|levels| "../".repeat(levels))
}

fn target() -> impl Strategy<Value = String> {
    prop_oneof![
        // Leaves the package on its own.
        (climb(), select(NAMES)).prop_map(|(up, name)| format!("./{up}{name}")),
        // A pattern: somewhere in the package, then whatever `*` captured.
        (select(&["", "lib/", "lib/deep/"][..]), any::<bool>()).prop_map(|(directory, js)| {
            format!("./{directory}*{}", if js { ".js" } else { "" })
        }),
        soup(4).prop_map(|rest| format!("./{rest}")),
    ]
}

fn exports() -> impl Strategy<Value = serde_json::Value> {
    proptest::collection::vec((select(KEYS), target()), 1..=3).prop_map(|entries| {
        serde_json::Value::Object(
            entries
                .into_iter()
                .map(|(key, target)| (key.to_string(), serde_json::Value::String(target)))
                .collect(),
        )
    })
}

fn specifier() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("pkg".to_string()),
        // What a pattern captures, climbing.
        (climb(), select(NAMES)).prop_map(|(up, name)| format!("pkg/{up}{name}")),
        soup(4).prop_map(|rest| format!("pkg/{rest}")),
    ]
}

/// Files inside the package for targets to land on, and the same names
/// outside it, where an escape would land.
fn project() -> Project {
    let project = Project::new();
    for inside in [
        "index.js",
        "a",
        "a.js",
        "lib/a",
        "lib/a.js",
        "lib/deep/a.js",
    ] {
        project.write(&format!("node_modules/pkg/{inside}"), "export default 1;\n");
    }
    for outside in [
        "a",
        "a.js",
        "node_modules/a",
        "node_modules/a.js",
        "node_modules/lib/a.js",
    ] {
        project.write(outside, "export default 2;\n");
    }
    project
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn test_exports_never_resolve_outside_the_package(
        exports in exports(),
        specifier in specifier(),
    ) {
        let project = project();
        project
            .write("node_modules/pkg/package.json", &serde_json::json!({ "exports": exports }).to_string())
            .write("index.mjs", &format!("import x from {specifier:?};\n"));

        let resolution = resolve(&project.root(), &entry("index.mjs"), &ResolverOptions::default())
            .expect("resolve");
        let graph = &resolution.graph;
        let importer = graph.module(graph.id_of(&entry("index.mjs")).expect("entry"));
        for dependency in &importer.dependencies {
            if let DependencyTarget::Module { id } = &dependency.target {
                let target = &graph.module(*id).path;
                prop_assert!(
                    target.starts_with(&NormalizedPath::new("node_modules/pkg")),
                    "{specifier:?} through {exports} resolved outside the package, to {target}"
                );
            }
        }
    }
}
