//! `opal test`: a test runner with Jest's semantics.
//!
//! The work is split across a process boundary. This crate finds the test
//! files, schedules them over a pool of long-lived Node workers, and adds up
//! and reports their results. The runtime under `runtime/` runs inside each
//! worker: it gives every test file a fresh `vm` context with Node's globals,
//! loads it through its own module registry, and runs it with a port of
//! jest-circus's scheduler. Jest's isolation model is kept (one context per
//! file, nothing shared between files but Node's built-in modules); Jest's
//! cost of starting it is not.
//!
//! `SEMANTICS.md` states what the runtime promises and where each rule comes
//! from in Jest's source; `compat/test-runners.py` checks it against Jest,
//! Vitest and Bun.

mod changed;
mod colors;
pub mod config;
mod discover;
pub mod glob;
mod pool;
mod related;
mod report;
mod runtime;
mod service;
pub mod transform;
mod tsconfig;
mod watch;

use std::io::Write;
use std::path::PathBuf;

use serde_json::json;

pub use report::Aggregate;

#[derive(Debug, thiserror::Error)]
pub enum TestError {
    #[error("could not start `{}`: {source}; opal test runs on Node, so `node` has to be on PATH", node.display())]
    Node {
        node: PathBuf,
        source: std::io::Error,
    },
    #[error("could not talk to the test workers: {0}")]
    Channel(std::io::Error),
    #[error("test worker {0} exited {1}")]
    WorkerExited(usize, String),
    #[error("test worker {0} did not connect")]
    WorkerTimeout(usize),
    #[error("{0}")]
    Config(String),
    #[error(transparent)]
    Glob(#[from] glob::GlobError),
    #[error("invalid pattern `{0}`: {1}")]
    Pattern(String, String),
    #[error("could not unpack the test runtime: {0}")]
    Runtime(std::io::Error),
    #[error("watch mode: {0}")]
    Watch(String),
    #[error("could not write {}: {source}", path.display())]
    Output {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// `--watch` reruns the tests related to what changed; `--watchAll` reruns
/// every test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    Off,
    Related,
    All,
}

/// Whether snapshots that don't exist yet, or don't match, are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateSnapshot {
    /// `--ci`: nothing is written, and a missing snapshot fails.
    None,
    /// The default: missing snapshots are written, mismatches fail.
    New,
    /// `-u`: every snapshot is rewritten from what the tests produce.
    All,
}

impl UpdateSnapshot {
    fn as_str(self) -> &'static str {
        match self {
            UpdateSnapshot::None => "none",
            UpdateSnapshot::New => "new",
            UpdateSnapshot::All => "all",
        }
    }
}

pub struct RunOptions {
    /// Where to look for a config, and the default `rootDir`.
    pub root: PathBuf,
    pub config: Option<PathBuf>,
    /// Regular expressions; a test file has to match one, when any are given.
    /// With `find_related_tests`, the source files whose tests to run instead.
    pub path_patterns: Vec<String>,
    /// `--findRelatedTests`: run the tests that depend on `path_patterns`.
    pub find_related_tests: bool,
    /// `--listTests`: print the test files that would run, and run none.
    pub list_tests: bool,
    /// `-o`: run the tests related to the files git says have changed.
    pub only_changed: bool,
    /// `--color` or `--no-color`; otherwise whether stdout supports color.
    pub color: Option<bool>,
    pub watch: Watch,
    pub test_name_pattern: Option<String>,
    pub update_snapshot: UpdateSnapshot,
    pub ci: bool,
    pub max_workers: Option<usize>,
    pub pass_with_no_tests: bool,
    pub test_timeout: Option<u64>,
    /// Print Jest's `--json` result, to `output_file` or else to stdout.
    pub json: bool,
    pub output_file: Option<PathBuf>,
    pub node: PathBuf,
    pub cache_dir: PathBuf,
}

pub struct Outcome {
    pub success: bool,
    pub files: usize,
}

/// Jest's default worker count: one fewer than the CPUs, at least one.
fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .saturating_sub(1)
        .max(1)
}

pub fn run(options: &RunOptions, out: &mut dyn Write) -> Result<Outcome, TestError> {
    let runtime_dir = runtime::unpack(&options.cache_dir).map_err(TestError::Runtime)?;
    let transforms = std::sync::Arc::new(service::TransformService::new(&options.cache_dir));
    let color = colors::stdout_supports_color(options.color);
    // Beside the unpacked runtime, sharing its content hash; none for a
    // runtime folder given with OPAL_TEST_RUNTIME, which is a source tree.
    let code_cache = std::env::var_os("OPAL_TEST_RUNTIME")
        .is_none()
        .then(|| runtime_dir.with_extension("v8"));
    let mut pool = pool::Pool::start(
        &options.node,
        &runtime_dir.join("worker.cjs"),
        transforms,
        color,
        code_cache,
    )?;
    pool.spawn()?;
    let result = match options.watch {
        Watch::Off => run_once(&mut pool, options, out),
        mode => watch::watch(&mut pool, options, mode, out),
    };
    pool.shutdown();
    result
}

/// The project's config, as the runtime normalized it: read for discovery,
/// and passed back to every worker.
struct Project {
    config: config::ProjectConfig,
    value: serde_json::Value,
}

fn load_project(pool: &mut pool::Pool, options: &RunOptions) -> Result<Project, TestError> {
    let reply = pool.request(json!({
        "type": "config",
        "rootDir": options.root,
        "configPath": options.config,
    }))?;
    if let Some(error) = reply["error"].as_str() {
        return Err(TestError::Config(error.to_string()));
    }
    let value = reply["config"].clone();
    let config = config::ProjectConfig::from_value(&value).map_err(|error| {
        TestError::Config(format!("the runtime sent an unreadable config: {error}"))
    })?;
    Ok(Project { config, value })
}

/// The test files a run takes on: those related to `changed` when it is
/// given, else every test file the path patterns let through.
fn select(
    options: &RunOptions,
    project: &Project,
    changed: Option<&[PathBuf]>,
) -> Result<discover::Found, TestError> {
    let Some(changed) = changed else {
        return discover::find_tests(&project.config, &discover::compile(&options.path_patterns)?);
    };
    let found = discover::find_tests(&project.config, &[])?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| options.root.clone());
    let mut tests = related::related_tests(&project.config, &found, changed, &cwd);
    // With -o, the path patterns still narrow what runs, as in Jest.
    if !options.find_related_tests && !options.path_patterns.is_empty() {
        let filters = discover::compile(&options.path_patterns)?;
        tests.retain(|path| filters.iter().any(|r| r.is_match(&path.to_string_lossy())));
    }
    Ok(discover::Found { tests, ..found })
}

fn run_once(
    pool: &mut pool::Pool,
    options: &RunOptions,
    out: &mut dyn Write,
) -> Result<Outcome, TestError> {
    let project = load_project(pool, options)?;
    let changed: Option<Vec<PathBuf>> = if options.find_related_tests {
        Some(options.path_patterns.iter().map(PathBuf::from).collect())
    } else if options.only_changed {
        Some(changed::changed_files(&project.config.roots)?)
    } else {
        None
    };
    let found = select(options, &project, changed.as_deref())?;
    if options.list_tests {
        list_tests(options, &found.tests, out)?;
        return Ok(Outcome {
            success: true,
            files: 0,
        });
    }
    run_files(pool, options, &project, found, out)
}

fn run_files(
    pool: &mut pool::Pool,
    options: &RunOptions,
    project: &Project,
    found: discover::Found,
    out: &mut dyn Write,
) -> Result<Outcome, TestError> {
    let mut config_value = project.value.clone();
    config_value["manualMocks"] = json!(found.manual_mocks);
    let files = found.tests;
    let mut aggregate = Aggregate::new(&project.config.root_dir);
    if files.is_empty() {
        let _ = writeln!(
            out,
            "No tests found, exiting with code {}",
            if options.pass_with_no_tests { 0 } else { 1 }
        );
        write_json(options, &aggregate)?;
        return Ok(Outcome {
            success: options.pass_with_no_tests,
            files: 0,
        });
    }

    let context = json!({
        "config": config_value,
        "globalConfig": {
            "ci": options.ci,
            "testNamePattern": options.test_name_pattern,
            "testTimeout": options.test_timeout,
            "updateSnapshot": options.update_snapshot.as_str(),
        },
    });
    let workers = options.max_workers.unwrap_or_else(default_workers);
    let count = files.len();
    let mut write_error = None;
    pool.run(files, &context, workers, |outcome| {
        let mut result = outcome.result;
        if result["testFilePath"].is_null() {
            result["testFilePath"] = json!(outcome.path);
        }
        if let Err(error) = aggregate.add(result, out) {
            write_error.get_or_insert(error);
        }
    })?;
    let _ = aggregate.print_summary(out);
    write_json(options, &aggregate)?;
    Ok(Outcome {
        success: aggregate.success(),
        files: count,
    })
}

/// runJest's `--listTests`: one path a line, or a JSON array with `--json`,
/// to `--outputFile` when given.
fn list_tests(
    options: &RunOptions,
    tests: &[PathBuf],
    out: &mut dyn Write,
) -> Result<(), TestError> {
    let text = if options.json {
        serde_json::to_string(tests).expect("serializable")
    } else {
        tests
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };
    match &options.output_file {
        Some(path) => std::fs::write(path, text).map_err(|source| TestError::Output {
            path: path.clone(),
            source,
        }),
        None => {
            let _ = writeln!(out, "{text}");
            Ok(())
        }
    }
}

fn write_json(options: &RunOptions, aggregate: &Aggregate) -> Result<(), TestError> {
    if !options.json {
        return Ok(());
    }
    let text = serde_json::to_string(&aggregate.to_json()).expect("serializable");
    match &options.output_file {
        Some(path) => std::fs::write(path, text).map_err(|source| TestError::Output {
            path: path.clone(),
            source,
        }),
        None => {
            println!("{text}");
            Ok(())
        }
    }
}
