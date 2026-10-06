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
//! - **The whole tree.** The same set of package versions.
//!
//! Fixtures are chosen for the edge case each exercises, not for popularity.
//! They need the public registry, `node`, and `npm`, so every test is
//! `#[ignore]`:
//!
//! ```text
//! cargo test -p opal-pm --test npm-cross-check -- --ignored --nocapture
//! ```
//!
//! **Identical fixtures are not the whole story, and the suite says so.** The
//! `agrees` fixtures were written to exercise semver and layout, and none of
//! them reaches a place where opal is known to differ from npm, so "every
//! fixture is identical" would hold however large those differences were.
//! The `known_difference` tests cover them. Each resolves a tree where opal
//! and npm are known to disagree and asserts the disagreement is exactly the
//! documented one, no more and no less:
//!
//! - **Reuse.** opal reuses any already-selected version that satisfies a
//!   range, and resolves `dependencies` before `devDependencies`; npm walks
//!   the root's dependencies by name and can place a newer copy first.
//! - **`bundleDependencies`.** opal also resolves, from the registry, what a
//!   package ships inside its own tarball.
//! - **Peers.** opal records peers and never installs them. Every other test
//!   here runs npm with `--legacy-peer-deps` so the trees are comparable at
//!   all, which is not how npm runs by default.
//!
//! A `known_difference` test failing means the difference grew, shrank, or
//! was fixed. Whichever it is, it's a decision about that difference and an
//! edit to the expectation, not a retry.
//!
//! **A locked project is a different question from a fresh one.** Everything
//! above resolves a manifest from nothing. `opal add` and `opal remove` start
//! from a lockfile, and what they must agree with npm about is what *stays*:
//! the last test walks one project through a sequence of changes with both
//! tools, installing for real, and compares the trees after every step.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::process::{Command, Stdio};

use opal_core::cache::CacheRoot;
use opal_pm::edit::AddRequest;
use opal_pm::install::{self, Change, InstallOptions};
use opal_pm::lockfile;
use opal_pm::manifest::{Manifest, Spec};
use opal_pm::package::PackageStore;
use opal_pm::progress::Silent;
use opal_pm::projects::ProjectIndex;
use opal_pm::registry::{NpmRegistry, Packument};
use opal_pm::resolve::{self, Resolution, ResolveOptions};
use opal_pm::semver::{Range, Version};
use serde_json::{Value, json};

/// npm lockfile v3 `packages`: `""` is the project, every other key a
/// `node_modules` placement.
type NpmTree = BTreeMap<String, Value>;

/// Whether npm installs peers, which is its default since npm 7.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Peers {
    /// `--legacy-peer-deps`: record them and install none, as opal does.
    Recorded,
    Installed,
}

fn npm_resolve(manifest: &Value, peers: Peers) -> NpmTree {
    let directory = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        directory.path().join("package.json"),
        serde_json::to_vec_pretty(manifest).expect("serializable"),
    )
    .expect("write package.json");
    let mut npm = Command::new("npm");
    npm.current_dir(directory.path())
        // `--prefer-online` because opal always revalidates here, and npm's
        // own cache can predate a release: once, electron-to-chromium 1.5.433
        // was 38 seconds old, npm answered 1.5.432 from cache, and the trees
        // "differed" over nothing but timing.
        .args([
            "install",
            "--package-lock-only",
            "--ignore-scripts",
            "--prefer-online",
            "--no-audit",
            "--no-fund",
        ]);
    if peers == Peers::Recorded {
        npm.arg("--legacy-peer-deps");
    }
    let output = npm.output().expect("run npm; these tests need it on PATH");
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
            ..ResolveOptions::default()
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

/// The npm that answered, printed with every comparison: it is the reference,
/// and its behaviour is not the same from one major version to the next.
fn npm_version() -> String {
    let output = Command::new("npm")
        .arg("--version")
        .output()
        .expect("run npm --version");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// What resolving one manifest with both tools turned up.
struct Comparison {
    /// Disagreements about a root's version or about what satisfies a range.
    /// Never expected, in any test.
    failures: Vec<String>,
    /// `name@version` in opal's tree and not in npm's.
    only_opal: BTreeSet<String>,
    /// `name@version` in npm's tree and not in opal's.
    only_npm: BTreeSet<String>,
}

impl Comparison {
    /// Package names behind a set of `name@version` entries.
    fn names(entries: &BTreeSet<String>) -> BTreeSet<&str> {
        entries
            .iter()
            .map(|entry| {
                entry
                    .rsplit_once('@')
                    .map_or(entry.as_str(), |(name, _)| name)
            })
            .collect()
    }
}

fn cross_check(manifest: Value) {
    let comparison = compare(&manifest, Peers::Recorded);
    let mut failures = comparison.failures;
    for only in &comparison.only_opal {
        failures.push(format!("only opal installs {only}"));
    }
    for only in &comparison.only_npm {
        failures.push(format!("only npm installs {only}"));
    }
    assert!(
        failures.is_empty(),
        "opal and npm disagree:\n  {}",
        failures.join("\n  ")
    );
}

fn compare(manifest: &Value, peers: Peers) -> Comparison {
    let tree = npm_resolve(manifest, peers);
    let resolution = opal_resolve(manifest);
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
        "npm {}: {} packages from opal, {} from npm; {} in both",
        npm_version(),
        opal_set.len(),
        npm_set.len(),
        opal_set.intersection(&npm_set).count()
    );
    Comparison {
        failures,
        only_opal: opal_set.difference(&npm_set).cloned().collect(),
        only_npm: npm_set.difference(&opal_set).cloned().collect(),
    }
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

/// The `package.json` that `create-next-app@16.3.2` writes: about 360
/// packages, and the tree both known resolution differences were found on.
fn next_scaffold() -> Value {
    serde_json::json!({
        "name": "next-test",
        "version": "0.1.0",
        "private": true,
        "dependencies": {
            "next": "16.3.2",
            "react": "19.2.8",
            "react-dom": "19.2.8"
        },
        "devDependencies": {
            "@tailwindcss/postcss": "^4",
            "@types/node": "^20",
            "@types/react": "^19",
            "@types/react-dom": "^19",
            "eslint": "^9",
            "eslint-config-next": "16.3.2",
            "tailwindcss": "^4",
            "typescript": "^5"
        }
    })
}

#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_known_difference_on_a_real_app_is_reuse_and_bundled_dependencies_only() {
    let comparison = compare(&next_scaffold(), Peers::Recorded);
    assert!(
        comparison.failures.is_empty(),
        "opal and npm disagree on a root or a range:\n  {}",
        comparison.failures.join("\n  ")
    );

    // Reuse: `next` pins an exact postcss, and opal gives the same copy to
    // `@tailwindcss/postcss`, whose range it satisfies. npm reaches
    // `@tailwindcss/postcss` first, places the newest postcss for it, and
    // nests the pinned one under `next`.
    assert_eq!(
        Comparison::names(&comparison.only_npm),
        BTreeSet::from(["postcss"]),
        "only npm installs: {:?}",
        comparison.only_npm
    );
    // bundleDependencies: `@tailwindcss/oxide-wasm32-wasi` ships these inside
    // its tarball, so npm resolves nothing for them.
    assert_eq!(
        Comparison::names(&comparison.only_opal),
        BTreeSet::from(["@emnapi/core", "@emnapi/wasi-threads"]),
        "only opal installs: {:?}",
        comparison.only_opal
    );
}

#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_known_difference_with_npm_installing_peers_is_the_peer_itself() {
    // npm as it runs by default. react-dom declares react only as a peer:
    // npm installs it, opal records it and installs nothing.
    let comparison = compare(
        &serde_json::json!({ "dependencies": { "react-dom": "19.0.0" } }),
        Peers::Installed,
    );
    assert!(comparison.failures.is_empty(), "{:?}", comparison.failures);
    assert_eq!(
        Comparison::names(&comparison.only_npm),
        BTreeSet::from(["react"]),
        "only npm installs: {:?}",
        comparison.only_npm
    );
    assert!(
        comparison.only_opal.is_empty(),
        "only opal installs: {:?}",
        comparison.only_opal
    );
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

/// One project, changed step by step by both tools.
struct Twins {
    directory: tempfile::TempDir,
    store: PackageStore,
    projects: ProjectIndex,
}

/// One change to a project, as each tool is told to make it.
enum Step {
    /// `package.json` rewritten by hand, then a plain install.
    Edit(Value),
    Add(&'static str),
    Remove(&'static str),
}

impl Twins {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = CacheRoot::at(directory.path().join("cache"));
        let store = PackageStore::open(cache.open_cas().expect("cas"), cache.path())
            .expect("package store");
        let projects = ProjectIndex::new(cache.path().join("projects")).expect("project index");
        for tool in ["npm", "opal"] {
            std::fs::create_dir_all(directory.path().join(tool)).expect("create project");
        }
        Self {
            directory,
            store,
            projects,
        }
    }

    fn project(&self, tool: &str) -> std::path::PathBuf {
        self.directory.path().join(tool)
    }

    fn npm(&self, arguments: &[&str]) {
        let output = Command::new("npm")
            .current_dir(self.project("npm"))
            .args(arguments)
            // Scripts off and peers recorded, which is what opal does. The
            // tree is installed for real: npm reads `node_modules` as well
            // as its lockfile when deciding what to keep.
            .args([
                "--ignore-scripts",
                "--legacy-peer-deps",
                "--prefer-online",
                "--no-audit",
                "--no-fund",
            ])
            .output()
            .expect("run npm; these tests need it on PATH");
        assert!(
            output.status.success(),
            "npm {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn opal(&self, change: Option<Change>) {
        let registry = NpmRegistry::discover();
        let (project, options) = (self.project("opal"), InstallOptions::default());
        let report = match &change {
            Some(change) => install::change(
                &project,
                change,
                &registry,
                &self.store,
                &self.projects,
                &options,
                &Silent,
            ),
            None => install::install(
                &project,
                &registry,
                &self.store,
                &self.projects,
                &options,
                &Silent,
            ),
        };
        report.unwrap_or_else(|error| panic!("opal failed: {error}"));
    }

    fn take(&self, step: &Step) {
        match step {
            Step::Edit(manifest) => {
                for tool in ["npm", "opal"] {
                    std::fs::write(
                        self.project(tool).join("package.json"),
                        serde_json::to_vec_pretty(manifest).expect("serializable"),
                    )
                    .expect("write package.json");
                }
                self.npm(&["install"]);
                self.opal(None);
            }
            Step::Add(argument) => {
                self.npm(&["install", argument]);
                self.opal(Some(Change::Add {
                    requests: vec![AddRequest::parse(argument).expect("a well-formed request")],
                    group: None,
                    exact: false,
                }));
            }
            Step::Remove(name) => {
                self.npm(&["uninstall", name]);
                self.opal(Some(Change::Remove {
                    names: vec![(*name).to_string()],
                }));
            }
        }
    }

    /// Every `name@version` npm's lockfile places, wherever it places it.
    fn npm_tree(&self) -> BTreeSet<String> {
        let lock: Value = serde_json::from_slice(
            &std::fs::read(self.project("npm").join("package-lock.json"))
                .expect("package-lock.json"),
        )
        .expect("lockfile JSON");
        lock["packages"]
            .as_object()
            .expect("a lockfile v3 `packages` map")
            .iter()
            .filter_map(|(placement, entry)| {
                let (_, name) = placement.rsplit_once("node_modules/")?;
                Some(format!("{name}@{}", entry["version"].as_str()?))
            })
            .collect()
    }

    fn opal_tree(&self) -> BTreeSet<String> {
        lockfile::read(&lockfile::path_in(&self.project("opal")))
            .expect("opal.lock parses")
            .expect("opal.lock exists")
            .packages
            .keys()
            .map(ToString::to_string)
            .collect()
    }

    fn declared(&self, tool: &str, name: &str) -> Value {
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(self.project(tool).join("package.json")).expect("package.json"),
        )
        .expect("package.json parses");
        manifest["dependencies"][name].clone()
    }
}

/// The versions here are pinned low on purpose. `ansi-styles` has a 4.3.0 and
/// `ms` a 2.1.3, so at every step there is a newer version each tool could
/// move to, and the test is that neither moves anything it was not told to.
#[test]
#[ignore = "reaches the public registry and needs npm"]
fn test_npm_agrees_at_every_step_of_changing_a_locked_project() {
    let steps = [
        (
            "a pinned install",
            Step::Edit(json!({
                "name": "locked",
                "version": "1.0.0",
                "dependencies": { "ansi-styles": "4.1.0", "chalk": "4.1.0", "ms": "2.0.0" }
            })),
        ),
        (
            "a pin loosened by hand keeps its version",
            Step::Edit(json!({
                "name": "locked",
                "version": "1.0.0",
                "dependencies": { "ansi-styles": "4.1.0", "chalk": "4.1.0", "ms": "^2.0.0" }
            })),
        ),
        (
            "removing a root leaves what another package still needs where it was",
            Step::Remove("ansi-styles"),
        ),
        (
            "adding something unrelated moves nothing",
            Step::Add("is-number"),
        ),
        ("naming a package that is present moves it", Step::Add("ms")),
        (
            "moving a package by name leaves its dependencies where they were",
            Step::Add("chalk@4.1.2"),
        ),
        (
            "naming a dependency moves its dependents to the same copy",
            Step::Add("ansi-styles@4.2.0"),
        ),
    ];

    let twins = Twins::new();
    println!("npm {}", npm_version());
    for (what, step) in &steps {
        twins.take(step);
        let (npm, opal) = (twins.npm_tree(), twins.opal_tree());
        println!(
            "{what}: {}",
            opal.iter().cloned().collect::<Vec<_>>().join(" ")
        );
        assert_eq!(opal, npm, "{what}: opal (left) and npm (right) disagree");
    }

    // A bare name is saved the same way by both. A typed version is not, on
    // purpose: npm widens `chalk@4.1.2` to `^4.1.2`, and opal keeps the pin.
    for name in ["is-number", "ms"] {
        assert_eq!(twins.declared("opal", name), twins.declared("npm", name));
    }
    assert_eq!(twins.declared("npm", "chalk"), "^4.1.2");
    assert_eq!(twins.declared("opal", "chalk"), "4.1.2");
}
