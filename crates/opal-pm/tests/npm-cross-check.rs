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
//! - **The whole tree.** The same set of package versions. Since opal picks
//!   versions in `npm-pick-manifest`'s order, one known policy difference
//!   remains: opal reuses any already-selected version that satisfies a
//!   range, while npm reuses only what is visible from the dependent's
//!   position. None of these fixtures exercises it. If one starts to after a
//!   registry change, the failure names the packages, and the fix is a
//!   decision about that difference, not a retry.
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
use opal_pm::registry::{NpmRegistry, Packument};
use opal_pm::resolve::{self, Resolution, ResolveOptions};
use opal_pm::semver::{Range, Version};
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

/// Runs `script` under node with one of npm's own dependencies loaded as
/// `module`, `input` as JSON on stdin, and whatever it prints parsed as JSON.
/// Loading npm's copy rather than installing one is what makes this npm's
/// answer: the version is whatever the installed npm resolves with.
fn with_npm_module<T: serde::de::DeserializeOwned>(
    module: &str,
    script: &str,
    input: &impl serde::Serialize,
) -> T {
    let root = Command::new("npm")
        .args(["root", "-g"])
        .output()
        .expect("run npm root -g");
    let path = format!(
        "{}/npm/node_modules/{module}",
        String::from_utf8_lossy(&root.stdout).trim()
    );
    let mut node = Command::new("node")
        .args([
            "-e",
            &format!(
                "const module = require(process.argv[1]);\
                 const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));\
                 process.stdout.write(JSON.stringify(({script})(module, input)));"
            ),
            &path,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run node; these tests need it on PATH");
    node.stdin
        .take()
        .expect("stdin")
        .write_all(&serde_json::to_vec(input).expect("serializable"))
        .expect("write input");
    let output = node.wait_with_output().expect("node output");
    assert!(output.status.success(), "node failed running {path}");
    serde_json::from_slice(&output.stdout).expect("node printed JSON")
}

/// Every (version, range) pair npm's own `semver` rejects.
fn rejected_by_npm_semver(pairs: &[(String, String)]) -> Vec<(String, String)> {
    with_npm_module(
        "semver",
        "(semver, pairs) => pairs.filter(([version, range]) =>\
           !semver.satisfies(version, range, { loose: true }))",
        &pairs,
    )
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
            Range::parse(&range_text).is_ok_and(|range| range.satisfies(&parsed))
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
        failures.push(format!("only opal installs {only}"));
    }
    for only in npm_set.difference(&opal_set) {
        failures.push(format!("only npm installs {only}"));
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

/// `resolve::pick` against `npm-pick-manifest` itself, over every combination
/// of a small registry: where `latest` points (or that there is no `latest`),
/// which versions are deprecated, and a range of each shape. That's 12,032
/// cases. The space is small enough to cover exhaustively in one `node` run,
/// so there's no reason to sample it.
///
/// Two npm rules opal leaves out on purpose are kept out of the space rather
/// than asserted as differences: no version declares `engines`, and a bare `*`
/// with a prerelease `latest` is skipped, because npm takes the prerelease.
#[test]
#[ignore = "needs node and npm"]
fn test_npm_pick_manifest_agrees_on_version_preference() {
    const VERSIONS: [&str; 7] = [
        "1.0.0",
        "1.1.0",
        "1.2.0-beta.1",
        "1.2.0",
        "1.3.0",
        "2.0.0-rc.1",
        "2.0.0",
    ];
    const RANGES: [&str; 12] = [
        "^1.0.0",
        "~1.1.0",
        "1.x",
        ">=1.1.0 <2.0.0",
        "^1.2.0-beta.0",
        ">=1.0.0",
        "^2.0.0-rc.0",
        "1.1.0",
        "^1.0.0 || ^2.0.0",
        "<1.2.0",
        "*",
        "^3.0.0",
    ];

    let mut packuments: Vec<Value> = Vec::new();
    let mut cases: Vec<(usize, &str)> = Vec::new();
    let mut skipped = 0;
    let latest_choices = std::iter::once(None).chain(VERSIONS.iter().copied().map(Some));
    for latest in latest_choices {
        for deprecated in 0u32..1 << VERSIONS.len() {
            let versions: serde_json::Map<String, Value> = VERSIONS
                .iter()
                .enumerate()
                .map(|(index, version)| {
                    let mut entry = serde_json::json!({
                        "name": "demo",
                        "version": version,
                        "dist": {
                            "tarball": format!("https://example.invalid/demo-{version}.tgz"),
                            "integrity": "sha512-Zm9vYmFy",
                        },
                    });
                    if deprecated & (1 << index) != 0 {
                        entry["deprecated"] = "deprecated".into();
                    }
                    (version.to_string(), entry)
                })
                .collect();
            let tags = latest.map_or_else(
                || serde_json::json!({}),
                |latest| serde_json::json!({ "latest": latest }),
            );
            packuments.push(serde_json::json!({
                "name": "demo",
                "dist-tags": tags,
                "versions": versions,
            }));
            for range in RANGES {
                if range == "*" && latest.is_some_and(|latest| latest.contains('-')) {
                    skipped += 1;
                    continue;
                }
                cases.push((packuments.len() - 1, range));
            }
        }
    }

    let npm: Vec<Option<String>> = with_npm_module(
        "npm-pick-manifest",
        "(pick, { packuments, cases }) => cases.map(([index, range]) => {\
           try { return pick(packuments[index], range).version; }\
           catch (error) { if (error.code === 'ETARGET') return null; throw error; }\
         })",
        &serde_json::json!({ "packuments": packuments, "cases": cases }),
    );

    let parsed: Vec<Packument> = packuments
        .iter()
        .map(|document| Packument::parse("demo", &serde_json::to_vec(document).expect("JSON")))
        .collect();
    let mut failures = Vec::new();
    for ((index, range), npm) in cases.iter().zip(&npm) {
        let opal = resolve::pick(
            &parsed[*index],
            &Range::parse(range).expect("a valid range"),
        )
        .map(|metadata| metadata.version.to_string());
        if opal != *npm {
            let packument = &packuments[*index];
            let deprecated: Vec<&String> = packument["versions"]
                .as_object()
                .expect("versions")
                .iter()
                .filter(|(_, entry)| entry.get("deprecated").is_some())
                .map(|(version, _)| version)
                .collect();
            failures.push(format!(
                "{range} with latest {} and {deprecated:?} deprecated: opal {opal:?}, npm {npm:?}",
                packument["dist-tags"]["latest"]
            ));
        }
    }
    println!(
        "{} cases, {skipped} skipped (bare * with a prerelease latest), {} disagree",
        cases.len(),
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "opal and npm-pick-manifest disagree on {} cases, first:\n  {}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n  ")
    );
}
