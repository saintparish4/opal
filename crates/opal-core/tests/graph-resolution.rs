//! What the resolver produces for shapes that occur in real projects.

mod support;

use std::fs;
use std::path::Path;

use opal_core::graph::resolver::{ResolverOptions, resolve};
use opal_core::graph::{DependencyKind, DependencyTarget, ModuleSystem, SourceKind};
use support::{Project, entry, realistic_app};

fn graph_of(project: &Project, from: &str) -> opal_core::graph::Resolution {
    resolve(&project.root(), &entry(from), &ResolverOptions::default()).expect("resolve")
}

fn specifiers(resolution: &opal_core::graph::Resolution, module: &str) -> Vec<(String, String)> {
    let graph = &resolution.graph;
    let id = graph.id_of(&entry(module)).expect("module in graph");
    graph
        .module(id)
        .dependencies
        .iter()
        .map(|dependency| {
            let target = match &dependency.target {
                DependencyTarget::Module { id } => graph.module(*id).path.to_string(),
                DependencyTarget::Builtin { name } => format!("builtin:{name}"),
                DependencyTarget::Unresolved { .. } => "unresolved".to_string(),
            };
            (dependency.specifier.clone(), target)
        })
        .collect()
}

#[test]
fn test_resolves_relative_extensionless_and_index_specifiers() {
    let project = Project::new();
    project
        .write(
            "index.js",
            "import './sibling.js';\nimport './extensionless';\nimport './folder';\n",
        )
        .write("sibling.js", "")
        .write("extensionless.js", "")
        .write("folder/index.js", "");

    let resolution = graph_of(&project, "index.js");
    assert_eq!(
        specifiers(&resolution, "index.js"),
        vec![
            ("./sibling.js".to_string(), "sibling.js".to_string()),
            (
                "./extensionless".to_string(),
                "extensionless.js".to_string()
            ),
            ("./folder".to_string(), "folder/index.js".to_string()),
        ]
    );
}

#[test]
fn test_cycles_terminate_with_one_module_each() {
    let project = Project::new();
    project
        .write("a.js", "import './b.js';\nexport const a = 1;\n")
        .write("b.js", "import './a.js';\nexport const b = 1;\n");

    let resolution = graph_of(&project, "a.js");
    assert_eq!(resolution.graph.len(), 2);
    assert_eq!(resolution.graph.edge_count(), 2);
}

#[test]
fn test_require_and_dynamic_import_are_distinct_edges() {
    let project = Project::new();
    project
        .write(
            "index.js",
            "const a = require('./a.js');\nconst b = import('./b.js');\nexport { a, b };\n",
        )
        .write("a.js", "")
        .write("b.js", "");

    let resolution = graph_of(&project, "index.js");
    let id = resolution.graph.id_of(&entry("index.js")).unwrap();
    let kinds: Vec<DependencyKind> = resolution
        .graph
        .module(id)
        .dependencies
        .iter()
        .map(|dependency| dependency.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![DependencyKind::Require, DependencyKind::DynamicImport]
    );
}

#[test]
fn test_type_only_imports_are_not_edges_by_default() {
    let project = Project::new();
    project
        .write(
            "index.ts",
            "import type { T } from './types';\nimport { value } from './value';\nexport const x: T = value;\n",
        )
        .write("types.ts", "export interface T { a: number }\n")
        .write("value.ts", "export const value = { a: 1 };\n");

    let resolution = graph_of(&project, "index.ts");
    assert_eq!(
        specifiers(&resolution, "index.ts"),
        vec![("./value".to_string(), "value.ts".to_string())]
    );

    let following = ResolverOptions {
        follow_type_only_imports: true,
        ..ResolverOptions::default()
    };
    let resolution = resolve(&project.root(), &entry("index.ts"), &following).unwrap();
    assert_eq!(resolution.graph.len(), 3);
}

#[test]
fn test_typescript_resolves_through_its_emitted_js_specifier() {
    let project = Project::new();
    project
        .write(
            "index.ts",
            "import { a } from './a.js';\nexport const b = a;\n",
        )
        .write("a.ts", "export const a = 1;\n");

    let resolution = graph_of(&project, "index.ts");
    assert_eq!(
        specifiers(&resolution, "index.ts"),
        vec![("./a.js".to_string(), "a.ts".to_string())]
    );
}

#[test]
fn test_exports_conditions_follow_the_edge_kind() {
    let project = Project::new();
    project
        .write(
            "index.js",
            "import esm from 'pkg';\nconst cjs = require('pkg');\nexport { esm, cjs };\n",
        )
        .write(
            "node_modules/pkg/package.json",
            r#"{ "exports": { "import": "./esm.mjs", "require": "./cjs.cjs" } }"#,
        )
        .write("node_modules/pkg/esm.mjs", "export default 1;\n")
        .write("node_modules/pkg/cjs.cjs", "module.exports = 1;\n");

    let resolution = graph_of(&project, "index.js");
    assert_eq!(
        specifiers(&resolution, "index.js"),
        vec![
            ("pkg".to_string(), "node_modules/pkg/esm.mjs".to_string()),
            ("pkg".to_string(), "node_modules/pkg/cjs.cjs".to_string()),
        ]
    );
}

#[test]
fn test_module_system_follows_nearest_package_type() {
    let project = Project::new();
    project
        .write("package.json", r#"{ "type": "module" }"#)
        .write("index.js", "import './legacy/old.js';\n")
        .write("legacy/package.json", r#"{ "type": "commonjs" }"#)
        .write("legacy/old.js", "module.exports = 1;\n");

    let resolution = graph_of(&project, "index.js");
    let entry_id = resolution.graph.id_of(&entry("index.js")).unwrap();
    let legacy_id = resolution.graph.id_of(&entry("legacy/old.js")).unwrap();
    assert_eq!(resolution.graph.module(entry_id).system, ModuleSystem::Esm);
    assert_eq!(resolution.graph.module(legacy_id).system, ModuleSystem::Cjs);
}

#[test]
fn test_unresolved_specifiers_are_recorded_not_fatal() {
    let project = Project::new();
    project.write(
        "index.js",
        "import 'not-installed';\nimport './gone.js';\nimport 'node:fs';\n",
    );

    let resolution = graph_of(&project, "index.js");
    assert_eq!(resolution.graph.unresolved().count(), 2);
    assert_eq!(resolution.graph.diagnostics().len(), 2);

    let targets = specifiers(&resolution, "index.js");
    assert_eq!(
        targets[2],
        ("node:fs".to_string(), "builtin:fs".to_string())
    );
}

#[test]
fn test_malformed_package_json_does_not_stop_resolution() {
    let project = Project::new();
    project
        .write("index.js", "import 'pkg';\n")
        .write("node_modules/pkg/package.json", "{ this is not json")
        .write("node_modules/pkg/index.js", "module.exports = 1;\n");

    let resolution = graph_of(&project, "index.js");
    assert_eq!(
        specifiers(&resolution, "index.js"),
        vec![("pkg".to_string(), "node_modules/pkg/index.js".to_string())]
    );
    assert!(
        resolution
            .graph
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("invalid JSON"))
    );
}

#[test]
fn test_json_and_opaque_files_are_leaf_modules() {
    let project = Project::new();
    project
        .write(
            "index.js",
            "import data from './data.json';\nconst native = require('./addon.node');\nexport { data, native };\n",
        )
        .write("data.json", r#"{ "a": 1 }"#)
        .write("addon.node", "not really a binary");

    let resolution = graph_of(&project, "index.js");
    let json = resolution.graph.id_of(&entry("data.json")).unwrap();
    let node = resolution.graph.id_of(&entry("addon.node")).unwrap();
    assert_eq!(resolution.graph.module(json).source, SourceKind::Json);
    assert_eq!(resolution.graph.module(node).source, SourceKind::Opaque);
    assert!(resolution.graph.module(json).dependencies.is_empty());
}

#[test]
fn test_trace_records_the_probes_a_result_depends_on() {
    let project = Project::new();
    project
        .write("index.js", "import './x';\n")
        .write("x/index.js", "");

    let resolution = graph_of(&project, "index.js");
    // `./x` was probed as a file, and as `./x.js`, before the directory index
    // won. Those absences are what a later run has to re-check.
    assert!(resolution.trace.missing.contains(&entry("x")));
    assert!(resolution.trace.missing.contains(&entry("x.js")));
    assert!(resolution.trace.files.contains_key(&entry("x/index.js")));
}

#[test]
fn test_realistic_project_matches_its_golden_snapshot() {
    let project = realistic_app();
    let resolution = graph_of(&project, "src/index.js");
    let json = resolution.graph.to_json();

    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/realistic-app.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, format!("{json}\n")).unwrap();
    }
    let expected = fs::read_to_string(&golden)
        .expect("golden missing; regenerate with UPDATE_GOLDEN=1 cargo test");
    assert_eq!(
        json.trim(),
        expected.trim(),
        "resolved graph changed shape; re-run with UPDATE_GOLDEN=1 if intended"
    );
}
