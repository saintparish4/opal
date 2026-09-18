//! Resolution, checked against npm on the same `package.json`.
//!
//! `testing_strategy.md` §1: a silent wrong-version install is worse than a
//! crash, and the property tests only hold opal to its own reading of semver.
//! These run npm's resolver on each fixture and hold the two to what they have
//! to agree on:
//!
//! - **Roots.** Each dependency the project declares resolves to the same
//!   version.
//! - **Semver, both ways.** Every version npm chose for an edge satisfies that
//!   edge's range as opal parses it, and every version opal chose satisfies it
//!   as npm's own `semver` reads it.
//!
//! The rest of the tree is compared and printed, not asserted, because two
//! differences in selection policy make it diverge without either side
//! misreading a range:
//!
//! - opal reuses any already-selected version of a package that satisfies a
//!   range; npm reuses only what is visible from the dependent's position.
//! - npm prefers the `latest` dist-tag whenever it satisfies the range (and
//!   avoids deprecated versions); opal takes the highest satisfying version.
//!   `get-intrinsic@^1.3.0` inside express is the live example: `latest` is
//!   1.3.0, and 1.3.1 was published after it without moving the tag.
//!
//! Fixtures are chosen for the edge case each exercises, not for popularity.
//! They need the public registry, `node`, and `npm`, so every test is
//! `#[ignore]`:
//!
//! ```text
//! cargo test -p opal-pm --test npm-cross-check -- --ignored --nocapture
//! ```
//!
//! npm runs with `--legacy-peer-deps`, because opal records peers and doesn't
//! install them yet. Without it, npm's tree holds packages opal's can't.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::process::{Command, Stdio};

use opal_pm::manifest::{Manifest, Spec};
use opal_pm::registry::NpmRegistry;
use opal_pm::resolve::{self, Resolution, ResolveOptions};
use opal_pm::semver::Version;
use serde_json::Value;

/// npm lockfile v3 `packages`: `""` is the project, every other key a
/// `node_modules` placement.
type NpmTree = BTreeMap<String, Value>;

fn npm_resolve(manifest: &Value) -> NpmTree {
    let directory = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        directory.path().join("package.json"),
        serde_json::to_vec_pretty(manifest).expect("serializable"),
    )
    .expect("write package.json");
    let output = Command::new("npm")
        .current_dir(directory.path())
        .args([
            "install",
            "--package-lock-only",
            "--ignore-scripts",
            "--legacy-peer-deps",
            "--no-audit",
            "--no-fund",
        ])
        .output()
        .expect("run npm; these tests need it on PATH");
    assert!(
        output.status.success(),
        "npm could not resolve the fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lock: Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("package-lock.json")).expect("package-lock.json"),
    )
    .expect("lockfile JSON");
    lock["packages"]
        .as_object()
        .expect("a lockfile v3 `packages` map")
        .iter()
        .map(|(key, entry)| (key.clone(), entry.clone()))
        .collect()
}

fn opal_resolve(manifest: &Value) -> Resolution {
    resolve::resolve(
        &NpmRegistry::discover(),
        &Manifest::from_value(manifest),
        &ResolveOptions {
            include_development: true,
        },
    )
    .expect("opal resolves the fixture")
}

/// The placement a `require(name)` from the package at `from` reaches,
/// walking up `node_modules` the way Node does.
fn visible<'a>(tree: &'a NpmTree, from: &str, name: &str) -> Option<&'a Value> {
    let mut base = from;
    loop {
        let key = if base.is_empty() {
            format!("node_modules/{name}")
        } else {
            format!("{base}/node_modules/{name}")
        };
        if let Some(entry) = tree.get(&key) {
            return Some(entry);
        }
        if base.is_empty() {
            return None;
        }
        base = base
            .rfind("/node_modules/")
            .map_or("", |index| &base[..index]);
    }
}

fn is_bundled(entry: &Value) -> bool {
    entry.get("inBundle").and_then(Value::as_bool) == Some(true)
}

/// (dependent, dependency, range, version npm chose) for every edge npm
/// resolved itself. Bundled packages are skipped: they came inside a tarball,
/// so nothing chose them.
fn npm_edges(tree: &NpmTree) -> Vec<(String, String, String, String)> {
    let mut edges = Vec::new();
    for (key, entry) in tree {
        if is_bundled(entry) {
            continue;
        }
        let mut fields = vec!["dependencies", "optionalDependencies"];
        if key.is_empty() {
            fields.push("devDependencies");
        }
        for field in fields {
            let Some(dependencies) = entry.get(field).and_then(Value::as_object) else {
                continue;
            };
            for (name, range) in dependencies {
                let (Some(range), Some(target)) = (range.as_str(), visible(tree, key, name)) else {
                    continue;
                };
                if is_bundled(target) {
                    continue;
                }
                let Some(version) = target.get("version").and_then(Value::as_str) else {
                    continue;
                };
                let dependent = if key.is_empty() { "(project)" } else { key };
                edges.push((
                    dependent.to_string(),
                    name.clone(),
                    range.to_string(),
                    version.to_string(),
                ));
            }
        }
    }
    edges
}

/// The range text of a specifier, with an `npm:` alias unwrapped. `None` for
/// anything that isn't a range: a dist-tag, a URL, a path.
fn range_text(spec: &str) -> Option<String> {
    match Spec::parse(spec) {
        Spec::Range(_) => Some(spec.trim().to_string()),
        Spec::Alias { raw, .. } => raw.rsplit_once('@').map(|(_, range)| range.to_string()),
        Spec::Tag(_) | Spec::Unsupported(_) => None,
    }
}

/// Every (version, range) pair npm's own `semver` rejects.
fn rejected_by_npm_semver(pairs: &[(String, String)]) -> Vec<(String, String)> {
    let root = Command::new("npm")
        .args(["root", "-g"])
        .output()
        .expect("run npm root -g");
    let semver = format!(
        "{}/npm/node_modules/semver",
        String::from_utf8_lossy(&root.stdout).trim()
    );
    let mut node = Command::new("node")
        .args([
            "-e",
            "const semver = require(process.argv[1]);\
             const pairs = JSON.parse(require('fs').readFileSync(0, 'utf8'));\
             process.stdout.write(JSON.stringify(\
               pairs.filter(([version, range]) => !semver.satisfies(version, range, { loose: true }))));",
            &semver,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run node; these tests need it on PATH");
    node.stdin
        .take()
        .expect("stdin")
        .write_all(&serde_json::to_vec(pairs).expect("serializable"))
        .expect("write pairs");
    let output = node.wait_with_output().expect("node output");
    assert!(
        output.status.success(),
        "npm's semver could not be loaded from {semver}"
    );
    serde_json::from_slice(&output.stdout).expect("rejected pairs")
}

fn cross_check(manifest: Value) {
    let tree = npm_resolve(&manifest);
    let resolution = opal_resolve(&manifest);
    let mut failures: Vec<String> = Vec::new();

    for requirement in &resolution.requirements {
        let Some(version) = &requirement.version else {
            continue;
        };
        let npm = tree
            .get(&format!("node_modules/{}", requirement.name))
            .and_then(|entry| entry.get("version"))
            .and_then(Value::as_str);
        if npm != Some(version.to_string().as_str()) {
            failures.push(format!(
                "root {} {}: opal chose {version}, npm chose {npm:?}",
                requirement.name, requirement.spec
            ));
        }
    }

    for (dependent, name, range, version) in npm_edges(&tree) {
        let Some(range_text) = range_text(&range) else {
            continue;
        };
        let agrees = Version::parse(&version).is_ok_and(|parsed| {
            opal_pm::semver::Range::parse(&range_text).is_ok_and(|range| range.satisfies(&parsed))
        });
        if !agrees {
            failures.push(format!(
                "{dependent} -> {name} {range}: npm chose {version}, which opal reads as outside the range"
            ));
        }
    }

    let mut chosen: Vec<(String, String)> = Vec::new();
    let mut described: Vec<String> = Vec::new();
    let mut edge = |from: &str, name: &str, spec: &str, version: &Version| {
        if let Some(range) = range_text(spec) {
            chosen.push((version.to_string(), range));
            described.push(format!("{from} -> {name} {spec}"));
        }
    };
    for requirement in &resolution.requirements {
        if let Some(version) = &requirement.version {
            edge("(project)", &requirement.name, &requirement.spec, version);
        }
    }
    for package in resolution.packages.values() {
        for dependency in &package.dependencies {
            edge(
                &package.id.to_string(),
                &dependency.name,
                &dependency.spec,
                &dependency.version,
            );
        }
    }
    for (version, range) in rejected_by_npm_semver(&chosen) {
        let index = chosen
            .iter()
            .position(|pair| *pair == (version.clone(), range.clone()))
            .expect("a pair that was sent");
        failures.push(format!(
            "{}: opal chose {version}, which npm's semver reads as outside {range}",
            described[index]
        ));
    }

    let opal_set: BTreeSet<String> = resolution
        .packages
        .keys()
        .map(ToString::to_string)
        .collect();
    let npm_set: BTreeSet<String> = tree
        .iter()
        .filter(|(key, entry)| !key.is_empty() && !is_bundled(entry))
        .filter_map(|(key, entry)| {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    key.rsplit_once("node_modules/")
                        .map_or(key, |(_, name)| name)
                });
            Some(format!("{name}@{}", entry.get("version")?.as_str()?))
        })
        .collect();
    println!(
        "{} packages from opal, {} from npm; {} in both",
        opal_set.len(),
        npm_set.len(),
        opal_set.intersection(&npm_set).count()
    );
    for only in opal_set.difference(&npm_set) {
        println!("  only opal: {only}");
    }
    for only in npm_set.difference(&opal_set) {
        println!("  only npm:  {only}");
    }

    assert!(
        failures.is_empty(),
        "opal and npm disagree:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_npm_agrees_on_every_range_syntax() {
    cross_check(serde_json::json!({
        "dependencies": {
            "debug": "~4.3.1",
            "ms": "2.x",
            "semver": "5 || 7",
            "lodash": "4.17.0 - 4.17.20",
            "chalk": ">=2 <4",
            "minimist": "=1.2.5",
            "commander": "latest"
        }
    }));
}

#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_npm_agrees_on_versions_that_have_to_nest() {
    // express 4.18.2 needs body-parser 1.20.1 and debug 2.6.9; the project
    // pins other versions of both, so express's copies nest.
    cross_check(serde_json::json!({
        "dependencies": {
            "express": "4.18.2",
            "body-parser": "1.19.0",
            "debug": "4.3.4"
        }
    }));
}

#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_npm_agrees_on_platform_gated_optionals() {
    // fsevents is chokidar's macOS-only optional; esbuild ships a binary per
    // platform as optionals. Both lockfiles record every platform's variant.
    cross_check(serde_json::json!({
        "dependencies": { "chokidar": "^3.5.3", "esbuild": "0.19.12" }
    }));
}

#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_npm_agrees_on_aliases() {
    cross_check(serde_json::json!({
        "dependencies": {
            "string-width": "^5.1.2",
            "string-width-cjs": "npm:string-width@^4.2.3",
            "@isaacs/cliui": "8.0.2"
        }
    }));
}

#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_npm_agrees_on_prerelease_ranges() {
    // A prerelease in a comparator admits prereleases of that version only;
    // `^19.0.0-rc.0` still resolves to the newest stable 19.x.
    cross_check(serde_json::json!({
        "dependencies": {
            "typescript": ">=5.0.0-beta <5.1.0",
            "react": "^19.0.0-rc.0"
        }
    }));
}

#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_npm_agrees_on_a_deep_dev_tree() {
    cross_check(serde_json::json!({
        "devDependencies": { "webpack": "^5.90.0" }
    }));
}
