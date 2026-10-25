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

pub mod config;
mod discover;
pub mod glob;
mod pool;
mod report;
mod runtime;

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
    #[error("could not write {}: {source}", path.display())]
    Output {
        path: PathBuf,
        source: std::io::Error,
    },
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
    pub path_patterns: Vec<String>,
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
    let mut pool = pool::Pool::start(&options.node, &runtime_dir.join("worker.cjs"))?;
    pool.spawn()?;
    let result = run_with(&mut pool, options, out);
    pool.shutdown();
    result
}

fn run_with(
    pool: &mut pool::Pool,
    options: &RunOptions,
    out: &mut dyn Write,
) -> Result<Outcome, TestError> {
    let reply = pool.request(json!({
        "type": "config",
        "rootDir": options.root,
        "configPath": options.config,
    }))?;
    if let Some(error) = reply["error"].as_str() {
        return Err(TestError::Config(error.to_string()));
    }
    let config_value = reply["config"].clone();
    let config = config::ProjectConfig::from_value(&config_value).map_err(|error| {
        TestError::Config(format!("the runtime sent an unreadable config: {error}"))
    })?;

    let filters = discover::compile(&options.path_patterns)?;
    let files = discover::find_tests(&config, &filters)?;
    let mut aggregate = Aggregate::new(&config.root_dir);
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
