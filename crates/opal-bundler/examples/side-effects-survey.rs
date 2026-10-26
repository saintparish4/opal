//! Which installed packages claim `"sideEffects"`, and what their code does
//! at the top level: the evidence `compat/COMPATIBILITY.md` records for
//! SEMANTICS.md rule 15.
//!
//! ```text
//! cargo run --release -p opal-bundler --features oracle --example side-effects-survey -- [--probe] <project>...
//! ```
//!
//! With `--probe`, each package that says `"sideEffects": false` and that
//! the project can import by name is also tested: a program imports it for
//! nothing but its effects, between a module that records the global
//! object and the built-in prototypes and one that prints what changed.
//! Run unbundled, that shows the package's effects; bundled, with the claim
//! honored, the package's code is left out. If the two runs differ, the
//! claim is false.
//!
//! For every package.json under each project's node_modules that has a
//! `sideEffects` field, it prints the field, how many of the package's ES
//! modules the field calls side-effect free, and how many of those have a
//! top-level statement the bundler's own analysis cannot prove pure. A pure
//! claim on such a module is what the bundler takes on trust.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;

use opal_bundler::load::PackageTypes;

#[derive(Default)]
struct Package {
    project: PathBuf,
    name: String,
    field: String,
    claimed_pure: usize,
    impure_statements: usize,
    modules_with_effects: Vec<String>,
}

const BEFORE: &str = "globalThis.__probe = { keys: new Set(Reflect.ownKeys(globalThis)), protos: [Object, Array, Function, String, Number, Promise, Map, Set, RegExp, Date, Error].map((C) => Reflect.ownKeys(C.prototype).length + Reflect.ownKeys(C).length) };\n";
const AFTER: &str = "const { keys, protos } = globalThis.__probe;\nconst added = Reflect.ownKeys(globalThis).filter((k) => !keys.has(k) && k !== '__probe').map(String);\nconst changed = [Object, Array, Function, String, Number, Promise, Map, Set, RegExp, Date, Error].map((C) => Reflect.ownKeys(C.prototype).length + Reflect.ownKeys(C).length).map((n, i) => n - protos[i]);\nconsole.log(JSON.stringify({ added, changed }));\n";

/// Runs the probe for one package: `None` when the project cannot import
/// it or the bundler refuses, else whether the runs were equivalent.
fn probe(package: &Package) -> Option<Result<bool, String>> {
    use opal_core::graph::resolve;
    use opal_core::path::NormalizedPath;
    let dir = package.project.join(".opal-probe");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let safe: String = package
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let entry = format!(".opal-probe/{safe}.mjs");
    std::fs::write(dir.join("before.mjs"), BEFORE).ok()?;
    std::fs::write(dir.join("after.mjs"), AFTER).ok()?;
    std::fs::write(
        package.project.join(&entry),
        format!(
            "import './before.mjs';\nimport {:?};\nimport './after.mjs';\n",
            package.name
        ),
    )
    .ok()?;
    let root = NormalizedPath::from_native(&std::fs::canonicalize(&package.project).ok()?).ok()?;
    let options = opal_bundler::BuildOptions::default();
    let resolution = resolve(
        &root,
        &NormalizedPath::new(&entry),
        &options.target.resolver_options(None),
    )
    .ok()?;
    let out = dir.join(format!("{safe}-dist"));
    let mut output = opal_bundler::build(&resolution.graph, &out, &options).ok()?;
    opal_bundler::write(&out, &mut output).ok()?;
    let node = opal_bundler::oracle::Node::default();
    let scratch = dir.join(format!("{safe}-oracle"));
    let original = node
        .observe(&package.project, &package.project.join(&entry), &scratch)
        .ok()?;
    if original.exit_code != Some(0) {
        return None;
    }
    let bundled = node
        .observe(&package.project, &out.join(&output.file_name), &scratch)
        .ok()?;
    // Whether the bundle left anything out on the claim's word: only then
    // does an equivalent run test the claim.
    let exercised = !output.trusted.is_empty();
    Some(
        opal_bundler::oracle::compare(&original, &bundled)
            .map(|()| exercised)
            .map_err(|d| d.to_string()),
    )
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let probing = args.first().is_some_and(|a| a == "--probe");
    if probing {
        args.remove(0);
    }
    let projects: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
    let mut packages: BTreeMap<String, Package> = BTreeMap::new();
    let mut types = PackageTypes::default();
    for project in &projects {
        let mut manifests = Vec::new();
        find_manifests(&project.join("node_modules"), &mut manifests);
        for manifest in manifests {
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(field) = value.get("sideEffects") else {
                continue;
            };
            let name = value.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let version = value.get("version").and_then(|v| v.as_str()).unwrap_or("?");
            let key = format!("{name}@{version}");
            if packages.contains_key(&key) {
                continue;
            }
            let dir = manifest.parent().unwrap().to_path_buf();
            let mut package = Package {
                project: project.clone(),
                name: name.to_string(),
                field: field.to_string(),
                ..Package::default()
            };
            let mut files = Vec::new();
            find_js(&dir, &mut files);
            for file in files {
                if !types.side_effect_free(&file) {
                    continue;
                }
                let Ok(source) = std::fs::read_to_string(&file) else {
                    continue;
                };
                let allocator = Allocator::default();
                let parsed = Parser::new(&allocator, &source, SourceType::mjs()).parse();
                if !parsed.diagnostics.is_empty() || !parsed.module_record.has_module_syntax {
                    // CommonJS, which rule 15 does not shake.
                    continue;
                }
                package.claimed_pure += 1;
                let scoping = SemanticBuilder::new()
                    .build(&parsed.program)
                    .semantic
                    .into_scoping();
                let impure = opal_bundler::shake::statements(&parsed.program, &scoping)
                    .iter()
                    .filter(|statement| !statement.pure && !statement.linkage)
                    .count();
                if impure > 0 {
                    package.impure_statements += impure;
                    package.modules_with_effects.push(
                        file.strip_prefix(&dir)
                            .unwrap_or(&file)
                            .display()
                            .to_string(),
                    );
                }
            }
            packages.insert(key, package);
        }
    }
    println!(
        "| Package | `sideEffects` | ES modules it calls side-effect free | Of those, with a statement the bundler cannot prove pure |"
    );
    println!("|---|---|---|---|");
    for (key, package) in &packages {
        let field = if package.field.len() > 60 {
            format!("{}…", &package.field[..60])
        } else {
            package.field.clone()
        };
        println!(
            "| {key} | `{}` | {} | {} ({} statements) |",
            field.replace('|', "\\|"),
            package.claimed_pure,
            package.modules_with_effects.len(),
            package.impure_statements
        );
    }
    eprintln!("{} packages with a sideEffects field", packages.len());
    if !probing {
        return;
    }
    let mut held = Vec::new();
    let mut untested = 0;
    let mut broke = Vec::new();
    let mut not_probed = 0;
    for (key, package) in &packages {
        if package.field != "false" {
            continue;
        }
        match probe(package) {
            None => not_probed += 1,
            Some(Ok(true)) => held.push(key.clone()),
            Some(Ok(false)) => untested += 1,
            Some(Err(detail)) => broke.push((key.clone(), detail)),
        }
    }
    println!();
    println!(
        "Probed {} packages that say `false`. For {}, the bundle left out code on the claim's word and behaved the same: the claim held. For {}, it left out code and behaved differently: the claim is false. For {}, it left out nothing on the claim (a CommonJS entry, which rule 15 does not shake, or code the bundler proved pure itself), so the probe tests nothing. {} could not be probed (not importable by name from the project root).",
        held.len() + broke.len() + untested,
        held.len(),
        broke.len(),
        untested,
        not_probed
    );
    for (key, detail) in &broke {
        println!(
            "- {key}: {}",
            detail.lines().take(3).collect::<Vec<_>>().join(" / ")
        );
    }
}

fn find_manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || entry.file_type().is_ok_and(|t| t.is_symlink()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('@') {
            find_manifests(&path, out);
            continue;
        }
        if path.join("package.json").is_file() {
            out.push(path.join("package.json"));
        }
        find_manifests(&path.join("node_modules"), out);
    }
}

fn find_js(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if name != "node_modules" {
                find_js(&path, out);
            }
        } else if name.ends_with(".js") || name.ends_with(".mjs") {
            out.push(path);
        }
    }
}
