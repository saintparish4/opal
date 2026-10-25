//! Fixtures for the differential tests: a directory holding a program, and
//! a `fixture.json` saying what the oracle should conclude about it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use opal_bundler::oracle::{self, Node, Observation};
use opal_bundler::{BuildOptions, Define, Target};
use opal_core::graph::resolve;
use opal_core::path::NormalizedPath;

/// Where each fixture's bundle is written, inside the copied program. Inside
/// rather than beside it, so the bundle reaches the original files by the
/// same relative paths a real out-dir would.
pub const OUT_DIR: &str = ".opal-out";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expect {
    /// The bundle must be indistinguishable from the program.
    Equivalent,
    /// The bundler must refuse, with this code.
    Refused(String),
    /// Only for the oracle's own fixtures: a hand-written bundle that is
    /// wrong on purpose, which the oracle must catch.
    Diverges,
    /// A divergence SEMANTICS.md lists: the runs must differ, and only in
    /// stdout lines containing `lines`. If they stop differing, the contract
    /// is out of date.
    KnownDivergence { section: String, lines: String },
}

#[derive(Clone, Debug)]
pub struct Fixture {
    pub name: String,
    pub dir: PathBuf,
    /// Relative to `dir`.
    pub entry: String,
    pub expect: Expect,
    pub config: serde_json::Value,
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture, sorted by name. `OPAL_FIXTURE` narrows it to the names
/// containing that text.
pub fn load_all() -> Vec<Fixture> {
    let filter = std::env::var("OPAL_FIXTURE").ok();
    let mut fixtures: Vec<Fixture> = std::fs::read_dir(fixtures_dir())
        .expect("tests/fixtures exists")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| load(&entry.path()))
        .filter(|fixture| {
            filter
                .as_deref()
                .is_none_or(|text| fixture.name.contains(text))
        })
        .collect();
    fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    fixtures
}

pub fn load(dir: &Path) -> Fixture {
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let config: serde_json::Value = match std::fs::read_to_string(dir.join("fixture.json")) {
        Ok(text) => serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{name}/fixture.json: {error}")),
        Err(_) => serde_json::json!({}),
    };
    let entry = match config.get("entry").and_then(|v| v.as_str()) {
        Some(entry) => entry.to_string(),
        None => [
            "main.mjs", "main.js", "main.cjs", "main.ts", "main.mts", "main.cts", "main.tsx",
            "main.jsx",
        ]
        .iter()
        .find(|candidate| dir.join(candidate).is_file())
        .unwrap_or_else(|| panic!("{name}: no entry; name one in fixture.json"))
        .to_string(),
    };
    let expect = match config.get("expect") {
        None => Expect::Equivalent,
        Some(serde_json::Value::String(text)) if text == "equivalent" => Expect::Equivalent,
        Some(serde_json::Value::String(text)) if text == "diverges" => Expect::Diverges,
        Some(value) => match (value.get("refused"), value.get("known-divergence")) {
            (Some(code), _) => Expect::Refused(code.as_str().unwrap().to_string()),
            (_, Some(known)) => Expect::KnownDivergence {
                section: known["section"].as_str().unwrap().to_string(),
                lines: known["lines"].as_str().unwrap().to_string(),
            },
            _ => panic!("{name}: unknown expect {value}"),
        },
    };
    Fixture {
        name,
        dir: dir.to_path_buf(),
        entry,
        expect,
        config,
    }
}

/// Copies a fixture's program into `into`, leaving out its test files.
pub fn copy_program(fixture: &Fixture, into: &Path) {
    copy_tree(&fixture.dir, into, &|name| {
        !matches!(name, "fixture.json" | "hand-bundle.mjs")
    });
}

pub fn copy_tree(from: &Path, to: &Path, keep: &dyn Fn(&str) -> bool) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if !keep(&name) {
            continue;
        }
        let target = to.join(&name);
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target, keep);
        } else if file_type.is_symlink() {
            let link = std::fs::read_link(entry.path()).unwrap();
            std::os::unix::fs::symlink(link, target).unwrap();
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// What a fixture run concluded.
#[derive(Debug)]
pub enum Outcome {
    Pass(String),
    Fail(String),
}

impl Outcome {
    pub fn passed(&self) -> bool {
        matches!(self, Outcome::Pass(_))
    }
}

/// What produced a bundle for a fixture: the result is the bundle file, or
/// a refusal with its code and message.
pub type Bundler<'a> = &'a (dyn Fn(&Fixture, &Path) -> Result<PathBuf, (String, String)> + Sync);

/// Bundles one fixture and holds the result to its expectation.
pub fn run(fixture: &Fixture, bundler: Bundler<'_>, node: &Node) -> Outcome {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("program");
    let scratch = temp.path().join("oracle");
    copy_program(fixture, &program);

    let bundled = bundler(fixture, &program);
    let bundle = match (&fixture.expect, bundled) {
        (Expect::Refused(code), Err((got, message))) => {
            return if &got == code {
                Outcome::Pass(format!("refused {got}"))
            } else {
                Outcome::Fail(format!("refused {got} instead of {code}: {message}"))
            };
        }
        (Expect::Refused(code), Ok(_)) => {
            return Outcome::Fail(format!("bundled, but should have been refused {code}"));
        }
        (_, Err((code, message))) => {
            return Outcome::Fail(format!("refused {code}: {message}"));
        }
        (_, Ok(bundle)) => bundle,
    };

    if !fixture.dir.join("hand-bundle.mjs").is_file()
        && let Err(failure) = check_output(fixture, &program, &bundle, bundler)
    {
        return Outcome::Fail(failure);
    }
    let mut node = node.clone();
    if let Some(env) = fixture.config.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env {
            node = node.with_env(key, value.as_str().unwrap_or_default());
        }
    }
    let observe = |file: &Path| -> Result<Observation, String> {
        node.observe(&program, file, &scratch)
            .map_err(|error| error.to_string())
    };
    let original_file = match original_program(fixture, &program) {
        Ok(file) => file,
        Err(error) => return Outcome::Fail(format!("original: {error}")),
    };
    let flags: Vec<String> = fixture
        .config
        .get("node_flags")
        .and_then(|v| v.as_array())
        .map(|flags| {
            flags
                .iter()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let original = match node
        .clone()
        .with_flags(&flags)
        .observe(&program, &original_file, &scratch)
        .map_err(|e| e.to_string())
    {
        Ok(observation) => observation,
        Err(error) => return Outcome::Fail(format!("original: {error}")),
    };
    let bundled = match observe(&bundle) {
        Ok(observation) => observation,
        Err(error) => return Outcome::Fail(format!("bundle: {error}")),
    };

    if let Expect::KnownDivergence { section, lines } = &fixture.expect {
        let keep = |bytes: &[u8]| -> String {
            String::from_utf8_lossy(bytes)
                .lines()
                .filter(|line| !line.contains(lines.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if oracle::compare(&original, &bundled).is_ok() {
            return Outcome::Fail(format!(
                "now equivalent: SEMANTICS.md ({section}) lists a divergence that no longer happens"
            ));
        }
        let mut original_rest = original.clone();
        let mut bundled_rest = bundled.clone();
        original_rest.stdout = keep(&original.stdout).into_bytes();
        bundled_rest.stdout = keep(&bundled.stdout).into_bytes();
        return match oracle::compare(&original_rest, &bundled_rest) {
            Ok(()) => Outcome::Pass(format!("known divergence ({section})")),
            Err(divergence) => Outcome::Fail(format!(
                "diverged beyond the known divergence ({section}):\n{divergence}"
            )),
        };
    }
    match (oracle::compare(&original, &bundled), &fixture.expect) {
        (Ok(()), Expect::Equivalent) => Outcome::Pass("equivalent".to_string()),
        (Err(divergence), Expect::Diverges) => Outcome::Pass(format!(
            "caught: {}",
            divergence.differences[0].lines().next().unwrap_or_default()
        )),
        (Ok(()), Expect::Diverges) => {
            Outcome::Fail("the oracle called a wrong bundle equivalent".to_string())
        }
        (Err(divergence), _) => Outcome::Fail(format!("diverged:\n{divergence}")),
        (Ok(()), Expect::Refused(_)) => unreachable!("a refusal returns before running"),
        (_, Expect::KnownDivergence { .. }) => unreachable!("handled above"),
    }
}

/// Runs every fixture on all cores, prints a table, and returns how many
/// failed.
pub fn run_all(fixtures: &[Fixture], bundler: Bundler<'_>) -> usize {
    let node = Node::default();
    eprintln!(
        "oracle: {} ({} fixtures)",
        node.version().unwrap_or_else(|error| error.to_string()),
        fixtures.len()
    );
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(fixture) = fixtures.get(index) else {
                        break;
                    };
                    let outcome = run(fixture, bundler, &node);
                    results.lock().unwrap().push((index, outcome));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(index, _)| *index);

    let width = fixtures.iter().map(|f| f.name.len()).max().unwrap_or(0);
    let mut failed = 0;
    for (index, outcome) in &results {
        let name = &fixtures[*index].name;
        match outcome {
            Outcome::Pass(detail) => eprintln!("  ok    {name:width$}  {detail}"),
            Outcome::Fail(detail) => {
                failed += 1;
                eprintln!(
                    "  FAIL  {name:width$}  {}",
                    detail.replace('\n', "\n        ")
                );
            }
        }
    }
    eprintln!("{} passed, {failed} failed", results.len() - failed);
    failed
}

/// The build options a fixture's `fixture.json` asks for.
pub fn options(fixture: &Fixture) -> BuildOptions {
    let mut options = BuildOptions::default();
    if let Some(target) = fixture.config.get("target").and_then(|v| v.as_str()) {
        options.target = Target::parse(target).expect("target is node or browser");
    }
    if let Some(externals) = fixture.config.get("external").and_then(|v| v.as_array()) {
        options.externals = externals
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    if let Some(defines) = fixture.config.get("define").and_then(|v| v.as_object()) {
        for (path, value) in defines {
            let value = value
                .as_str()
                .expect("a define's value is JavaScript source in a string");
            options
                .defines
                .push(Define::parse(&format!("{path}={value}")).unwrap());
        }
    }
    options
}

/// Resolves and bundles a copied fixture into its out-dir.
pub fn build_fixture(fixture: &Fixture, program: &Path) -> Result<PathBuf, (String, String)> {
    let options = options(fixture);
    let root = NormalizedPath::from_native(&std::fs::canonicalize(program).unwrap()).unwrap();
    let resolver = options
        .target
        .resolver_options(Some(options.jsx_import_source.clone()));
    let resolution = resolve(&root, &NormalizedPath::new(&fixture.entry), &resolver)
        .map_err(|error| ("E-IO".to_string(), error.to_string()))?;
    let out = root.as_path().join(OUT_DIR);
    let mut output = opal_bundler::build(&resolution.graph, &out, &options)
        .map_err(|diagnostic| (diagnostic.code.to_string(), diagnostic.to_string()))?;
    opal_bundler::write(&out, &mut output)
        .map_err(|diagnostic| (diagnostic.code.to_string(), diagnostic.to_string()))?;
    Ok(out.join(&output.file_name))
}

/// The tools the tests compare against, installed by `npm ci` in
/// crates/opal-bundler/tools.
pub fn tool(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tools/node_modules/.bin")
        .join(name);
    assert!(
        path.exists(),
        "{} is missing: run `npm ci` in crates/opal-bundler/tools",
        path.display()
    );
    path
}

/// The program the bundle is held to. Usually the fixture's files run by
/// Node as they are; with `"original": "tsc"`, what `tsc` emits from the
/// fixture's tsconfig.json, which is the reference SEMANTICS.md rule 13
/// names for TypeScript Node cannot strip, and for JSX.
pub fn original_program(fixture: &Fixture, program: &Path) -> Result<PathBuf, String> {
    match fixture.config.get("original").and_then(|v| v.as_str()) {
        None => Ok(program.join(&fixture.entry)),
        Some("tsc") => {
            let out = program.join(".tsc-out");
            let compiled = std::process::Command::new(tool("tsc"))
                .args(["-p", "tsconfig.json", "--outDir"])
                .arg(&out)
                .current_dir(program)
                .output()
                .map_err(|error| format!("tsc: {error}"))?;
            let entry = Path::new(&fixture.entry);
            let js = out.join(entry.with_extension(
                match entry.extension().and_then(|e| e.to_str()) {
                    Some("mts") => "mjs",
                    Some("cts") => "cjs",
                    _ => "js",
                },
            ));
            // Type errors do not change what tsc emits, so the program is
            // whatever it wrote; only a missing file is a failure.
            if !js.is_file() {
                return Err(format!(
                    "tsc emitted no {}:\n{}",
                    js.display(),
                    String::from_utf8_lossy(&compiled.stdout)
                ));
            }
            Ok(js)
        }
        Some(other) => Err(format!("unknown original {other:?}")),
    }
}

/// Step 5's checks on a bundle, none of which trust the bundler: a second
/// parser accepts it (`node --check`), its source map round-trips through
/// @jridgewell/trace-mapping (tools/sourcemap-check.mjs), and building the
/// same program from a different path gives the same bytes.
fn check_output(
    fixture: &Fixture,
    program: &Path,
    bundle: &Path,
    bundler: Bundler<'_>,
) -> Result<(), String> {
    let node = std::env::var("OPAL_NODE").unwrap_or_else(|_| "node".to_string());
    let checked = std::process::Command::new(&node)
        .arg("--check")
        .arg(bundle)
        .output()
        .map_err(|error| format!("node --check: {error}"))?;
    if !checked.status.success() {
        return Err(format!(
            "node --check rejects the bundle:\n{}",
            String::from_utf8_lossy(&checked.stderr)
        ));
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/sourcemap-check.mjs");
    let mapped = std::process::Command::new(&node)
        .arg(&script)
        .arg(bundle)
        .output()
        .map_err(|error| format!("sourcemap-check: {error}"))?;
    if !mapped.status.success() {
        return Err(format!(
            "the source map does not round-trip:\n{}{}",
            String::from_utf8_lossy(&mapped.stdout),
            String::from_utf8_lossy(&mapped.stderr)
        ));
    }
    // The same program at another path, deeper and differently named.
    let other = tempfile::tempdir().unwrap();
    let elsewhere = other.path().join("somewhere/else entirely/program");
    copy_program(fixture, &elsewhere);
    let again = bundler(fixture, &elsewhere)
        .map_err(|(code, message)| format!("the second build refused {code}: {message}"))?;
    for (first, second) in [
        (bundle.to_path_buf(), again.clone()),
        (map_of(bundle), map_of(&again)),
    ] {
        let a = std::fs::read(&first).unwrap();
        let b = std::fs::read(&second).unwrap();
        if a != b {
            return Err(format!(
                "not deterministic: {} differs when the project is at another path",
                first.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    let _ = program;
    Ok(())
}

fn map_of(bundle: &Path) -> PathBuf {
    let mut name = bundle.as_os_str().to_owned();
    name.push(".map");
    PathBuf::from(name)
}
