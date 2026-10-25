//! Fixtures for the differential tests: a directory holding a program, and
//! a `fixture.json` saying what the oracle should conclude about it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use opal_bundler::oracle::{self, Node, Observation};

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
            "main.mjs", "main.js", "main.cjs", "main.ts", "main.mts", "main.tsx",
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
        Some(value) => match value.get("refused").and_then(|v| v.as_str()) {
            Some(code) => Expect::Refused(code.to_string()),
            None => panic!("{name}: unknown expect {value}"),
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

    let observe = |file: &Path| -> Result<Observation, String> {
        node.observe(&program, file, &scratch)
            .map_err(|error| error.to_string())
    };
    let original = match observe(&program.join(&fixture.entry)) {
        Ok(observation) => observation,
        Err(error) => return Outcome::Fail(format!("original: {error}")),
    };
    let bundled = match observe(&bundle) {
        Ok(observation) => observation,
        Err(error) => return Outcome::Fail(format!("bundle: {error}")),
    };

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
