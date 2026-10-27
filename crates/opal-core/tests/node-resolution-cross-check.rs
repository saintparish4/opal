//! `graph::node_resolution` against Node's own resolution, on package trees
//! generated from a seed: what `require.resolve` (createRequire) and the ES
//! module resolver (`defaultResolve`, from Node's internals) answer for the
//! same specifier from the same file, file for file and error for error.
//!
//! The trees exercise what Node's rules are made of: nested and scoped
//! packages, a symlinked one, `main` that names a file, a file without its
//! extension, a directory or nothing; `exports` as a string, an array, a
//! conditions object and a subpath map with patterns, null and invalid
//! targets; `imports`; `type`; and a package requiring itself by name. Each
//! of our errors is turned into the message Node would print by Node's own
//! internal/errors, so messages are compared too, not only codes.
//!
//! Needs node 24.19.0 on PATH, so it is `#[ignore]`d in the ordinary suite:
//!
//! ```text
//! cargo test -p opal-core --test integration -- --ignored --nocapture node_resolution_cross_check::
//! ```
//! `OPAL_RESOLUTION_TREES` sets how many trees (default 60).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use opal_core::graph::node_resolution::{Failure, NodeResolver, Resolved};
use serde_json::{Value, json};

/// xorshift64*, so a failing seed can be rerun.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const FILES: &[&str] = &[
    "index.js",
    "index.json",
    "a.js",
    "a.json",
    "b.cjs",
    "b.mjs",
    "lib/index.js",
    "lib/x.js",
    "lib/x.json",
    "lib/y/index.js",
    "dist/main.js",
    "dist/esm.mjs",
    "features/f.js",
    "features/g.js",
    "features/sub/h.js",
    "main.js",
    "data.json",
    "src/main.js",
    "esm/index.js",
];

const CONDITIONS: &[&str] = &[
    "require",
    "import",
    "node",
    "default",
    "browser",
    "module-sync",
    "node-addons",
    "types",
];

fn target(rng: &mut Rng, pattern: bool) -> Value {
    let files = [
        "./index.js",
        "./a.js",
        "./lib/x.js",
        "./lib/",
        "./dist/main.js",
        "./dist/esm.mjs",
        "./missing.js",
        "./features/",
    ];
    let patterns = [
        "./features/*.js",
        "./features/*",
        "./lib/*.js",
        "./lib/*",
        "./*",
        "./dist/*.js",
    ];
    let invalid = [
        "../outside.js",
        "lib/x.js",
        "./node_modules/x.js",
        "./lib/../a.js",
        ".//a.js",
        "/abs.js",
    ];
    match rng.below(12) {
        0 => Value::Null,
        1 => json!(rng.pick(&invalid)),
        2 => json!([rng.pick(&invalid), rng.pick(&files)]),
        3 | 4 => {
            let mut map = serde_json::Map::new();
            for _ in 0..1 + rng.below(3) {
                let condition = rng.pick(CONDITIONS).to_string();
                map.insert(condition, target(rng, pattern));
            }
            Value::Object(map)
        }
        _ if pattern => json!(rng.pick(&patterns)),
        _ => json!(rng.pick(&files)),
    }
}

fn exports(rng: &mut Rng) -> Value {
    match rng.below(5) {
        0 => target(rng, false),
        1 => {
            let mut map = serde_json::Map::new();
            for _ in 0..1 + rng.below(2) {
                map.insert(rng.pick(CONDITIONS).to_string(), target(rng, false));
            }
            Value::Object(map)
        }
        _ => {
            let mut map = serde_json::Map::new();
            let keys = [
                ".",
                "./sub",
                "./lib/",
                "./features/*",
                "./features/*.js",
                "./*",
                "./x",
                "./package.json",
                "./lib/*.js",
            ];
            for _ in 0..1 + rng.below(4) {
                let key = *rng.pick(&keys);
                map.insert(key.to_string(), target(rng, key.contains('*')));
            }
            Value::Object(map)
        }
    }
}

fn package_json(rng: &mut Rng, name: &str) -> Value {
    let mut pkg = serde_json::Map::new();
    if rng.chance(85) {
        pkg.insert("name".into(), json!(name));
    }
    if rng.chance(50) {
        let mains = [
            "./dist/main.js",
            "dist/main",
            "lib",
            "./lib/",
            "missing",
            "a",
            "./index.js",
            "data.json",
            "",
        ];
        pkg.insert("main".into(), json!(rng.pick(&mains)));
    }
    if rng.chance(30) {
        pkg.insert(
            "type".into(),
            json!(rng.pick(&["module", "commonjs", "other"])),
        );
    }
    if rng.chance(55) {
        pkg.insert("exports".into(), exports(rng));
    }
    if rng.chance(30) {
        let mut imports = serde_json::Map::new();
        imports.insert("#x".into(), target(rng, false));
        imports.insert("#y/*".into(), target(rng, true));
        if rng.chance(40) {
            imports.insert(
                "#dep".into(),
                json!(rng.pick(&["dep", "dep/lib/x.js", "@s/p"])),
            );
        }
        pkg.insert("imports".into(), Value::Object(imports));
    }
    Value::Object(pkg)
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn fill(rng: &mut Rng, dir: &Path, name: &str) {
    if rng.chance(90) {
        let pkg = package_json(rng, name);
        write(
            &dir.join("package.json"),
            &serde_json::to_string_pretty(&pkg).unwrap(),
        );
    }
    for file in FILES {
        if rng.chance(45) {
            write(&dir.join(file), "module.exports = 1\n");
        }
    }
}

/// A project with packages around it, and the files to resolve from.
fn tree(rng: &mut Rng, root: &Path) -> Vec<PathBuf> {
    let project = root.join("project");
    fill(rng, &project, "project");
    let mut parents = vec![project.join("src/main.js"), project.join("index.js")];
    for name in ["dep", "@s/p", "other"] {
        if rng.chance(80) {
            let dir = project.join("node_modules").join(name);
            fill(rng, &dir, name);
            parents.push(dir.join("lib/inner.js"));
        }
    }
    if rng.chance(40) {
        let dir = project.join("node_modules/dep/node_modules/nested");
        fill(rng, &dir, "nested");
        parents.push(dir.join("index.js"));
    }
    if rng.chance(30) {
        // Installed one level up, as when a workspace hoists.
        let dir = root.join("node_modules/hoisted");
        fill(rng, &dir, "hoisted");
    }
    if rng.chance(30) {
        // A symlinked package: its files resolve by their real path.
        let real = root.join("packages/linked");
        fill(rng, &real, "linked");
        std::fs::create_dir_all(project.join("node_modules")).unwrap();
        let _ = std::os::unix::fs::symlink(&real, project.join("node_modules/linked"));
        parents.push(project.join("node_modules/linked/index.js"));
    }
    for parent in &parents {
        if !parent.exists() {
            write(parent, "module.exports = 1\n");
        }
    }
    parents
}

const SPECIFIERS: &[&str] = &[
    "./a",
    "./a.js",
    "./a.json",
    "./b",
    "./lib",
    "./lib/",
    "./lib/x",
    "./lib/y",
    "./missing",
    "../index.js",
    "..",
    ".",
    "./",
    "dep",
    "dep/lib/x.js",
    "dep/sub",
    "dep/x",
    "dep/features/f",
    "dep/features/f.js",
    "dep/features/sub/h",
    "dep/package.json",
    "dep/lib/",
    "@s/p",
    "@s/p/x",
    "@s/p/features/g.js",
    "other",
    "other/a",
    "nested",
    "hoisted",
    "linked",
    "linked/a",
    "project",
    "project/x",
    "project/features/f",
    "#x",
    "#y/f",
    "#y/sub/h",
    "#dep",
    "#missing",
    "#",
    "#y/",
    "./dist/esm.mjs",
    "dep/lib/x",
    "@s",
    "@s/",
    ".dep",
    "dep%2Fx",
    "node:fs",
    "fs",
    "./a%20b.js",
    "dep/features/%2e%2e/a.js",
];

const NODE_DRIVER: &str = r#"
const { createRequire } = require("node:module");
const { pathToFileURL, fileURLToPath } = require("node:url");
const { defaultResolve } = require("internal/modules/esm/resolve");
const { codes } = require("internal/errors");
const input = JSON.parse(require("fs").readFileSync(0, "utf8"));
const warnings = [];
// Warnings are delivered a tick later; the call that makes one is not.
process.emitWarning = (warning, type, code) => warnings.push(typeof type === "object" ? type.code : code);
const results = input.queries.map(({ parent, specifier, mode }) => {
  warnings.length = 0;
  try {
    if (mode === "require") {
      const resolved = createRequire(parent).resolve(specifier);
      return { ok: resolved, warnings: [...warnings] };
    }
    const { url } = defaultResolve(specifier, { parentURL: pathToFileURL(parent).href });
    return { ok: url.startsWith("file:") ? fileURLToPath(url) : url, warnings: [...warnings] };
  } catch (error) {
    return { code: error.code, message: error.message.split("\nRequire stack:")[0], path: error.path ?? null };
  }
});
const ours = input.ours.map((failure) => {
  if (!failure) return null;
  try {
    const message = new codes[failure.code](...failure.args.map((a) => a === "true" ? true : a === "false" ? false : a ?? undefined)).message;
    return failure.hint ? `${message}\n${failure.hint}` : message;
  } catch (error) {
    return `could not format: ${error.message}`;
  }
});
process.stdout.write(JSON.stringify({ results, ours }));
"#;

#[test]
#[ignore = "needs node on PATH"]
fn matches_node() {
    let trees: u64 = std::env::var("OPAL_RESOLUTION_TREES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(60);
    let mut failures = Vec::new();
    let (mut checked, mut same) = (0, 0);
    for seed in 1..=trees {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let parents = tree(&mut rng, &root);

        let mut queries = Vec::new();
        let mut ours = Vec::new();
        let mut resolver = NodeResolver::new(Path::new("/nonexistent/bin/opal"));
        for parent in &parents {
            for specifier in SPECIFIERS {
                for mode in ["require", "import"] {
                    if *specifier == "node:fs" || *specifier == "fs" {
                        continue; // Builtins are the loader's, not the resolver's.
                    }
                    resolver.warnings.clear();
                    let result = match mode {
                        "require" => resolver
                            .require(specifier, Some(parent), false)
                            .map(Resolved::File),
                        _ => resolver.import(specifier, parent),
                    };
                    let warnings: Vec<&str> = resolver.warnings.iter().map(|w| w.code).collect();
                    queries.push((
                        json!({ "parent": parent, "specifier": specifier, "mode": mode }),
                        result,
                        warnings,
                    ));
                }
            }
        }
        for (_, result, _) in &queries {
            ours.push(match result {
                Err(Failure::Node { code, args, hint }) => {
                    json!({ "code": code, "args": args, "hint": hint })
                }
                _ => Value::Null,
            });
        }

        let mut node = Command::new("node")
            .args(["--expose-internals", "-e", NODE_DRIVER])
            .env_remove("NODE_PATH")
            .env("HOME", "/nonexistent")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("run node");
        let payload = json!({ "queries": queries.iter().map(|(q, _, _)| q).collect::<Vec<_>>(), "ours": ours });
        node.stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let output = node.wait_with_output().unwrap();
        let answer: Value = serde_json::from_slice(&output.stdout).expect("node's answers");

        for (i, (query, result, warnings)) in queries.iter().enumerate() {
            checked += 1;
            let expected = &answer["results"][i];
            let node_warnings: Vec<&str> = expected["warnings"]
                .as_array()
                .map(|w| w.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let mismatch = match (result, expected.get("ok")) {
                (Ok(_), Some(_)) if result.is_ok() && *warnings != node_warnings => {
                    Some(format!("{result:?} with warnings {warnings:?}"))
                }
                (Ok(Resolved::File(path)), Some(Value::String(ok))) => {
                    (path.to_string_lossy() != ok.as_str()).then(|| format!("{path:?}"))
                }
                (Ok(Resolved::Url(url)), Some(Value::String(ok))) => {
                    (url != ok).then(|| url.clone())
                }
                (Ok(Resolved::Builtin(name)), Some(Value::String(ok))) => {
                    (format!("node:{name}") != *ok).then(|| name.clone())
                }
                (Ok(found), _) => Some(format!("{found:?}")),
                (Err(failure), None) => {
                    let code = expected["code"].as_str().unwrap_or("");
                    let (our_code, our_message, our_path) = match failure {
                        Failure::NotFound => ("MODULE_NOT_FOUND", None, None),
                        Failure::BadMain {
                            filename,
                            package_json,
                        } => (
                            "MODULE_NOT_FOUND",
                            Some(format!(
                                "Cannot find module '{}'. Please verify that the package.json has a valid \"main\" entry",
                                filename.display()
                            )),
                            Some(package_json.display().to_string()),
                        ),
                        Failure::EsmNotFound { request, path } => (
                            "MODULE_NOT_FOUND",
                            Some(format!("Cannot find module '{request}'")),
                            path.as_ref().map(|p| p.display().to_string()),
                        ),
                        Failure::Node { code, .. } => {
                            (*code, answer["ours"][i].as_str().map(str::to_string), None)
                        }
                    };
                    let message_differs = our_message
                        .as_ref()
                        .is_some_and(|m| Some(m.as_str()) != expected["message"].as_str());
                    let path_differs =
                        our_path.is_some() && our_path.as_deref() != expected["path"].as_str();
                    (our_code != code || message_differs || path_differs)
                        .then(|| format!("{our_code} {our_message:?} path={our_path:?}"))
                }
                (Err(failure), Some(_)) => Some(format!("{failure:?}")),
            };
            match mismatch {
                None => same += 1,
                Some(ours) => failures.push(format!(
                    "seed {seed}: {mode} {specifier:?} from {parent}\n  node: {expected}\n  opal: {ours}",
                    mode = query["mode"].as_str().unwrap(),
                    specifier = query["specifier"].as_str().unwrap(),
                    parent = query["parent"].as_str().unwrap().replace(&root.display().to_string(), "<root>"),
                    expected = expected.to_string().replace(&root.display().to_string(), "<root>"),
                )),
            }
        }
        if !failures.is_empty() {
            // Keep the first failing tree to look at.
            let keep = std::env::temp_dir().join(format!("opal-resolution-seed-{seed}"));
            let _ = std::fs::remove_dir_all(&keep);
            let _ = Command::new("cp").arg("-a").arg(&root).arg(&keep).status();
            eprintln!("tree kept at {}", keep.display());
            break;
        }
    }
    println!(
        "{checked} resolutions: {same} as node, {} differ",
        failures.len()
    );
    let shown: Vec<&String> = failures.iter().take(25).collect();
    assert!(
        failures.is_empty(),
        "{}",
        shown
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
