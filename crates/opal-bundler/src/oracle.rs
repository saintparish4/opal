//! The oracle: run a program and its bundle under Node, and compare what an
//! observer of each could see.
//!
//! `SEMANTICS.md` ("What is observed") lists what has to match: stdout and
//! stderr bytes, the exit code, and the constructor name and message of an
//! uncaught error. This module observes exactly those and nothing else, so a
//! passing comparison means what the contract says it means.
//!
//! The uncaught error is read by a preloaded hook (`node --require`), through
//! `process.on('uncaughtExceptionMonitor')`. The hook is a CommonJS module
//! on purpose: preloading with `--import` makes Node run a CommonJS entry
//! through its ES module loader, which changes `module.id`, `require.main`
//! errors, and the origin of an uncaught error. The oracle must not change
//! the program it observes, and `--require` does not. A monitor sees the error before
//! Node's default handler prints it and does not change what happens next:
//! the exit code and the printed trace are Node's own. The trace itself is
//! not observed (it names files and lines), so the hook writes a marker to
//! stderr first, and everything from the marker on is cut.
//!
//! Only built with the `oracle` feature, since it spawns `node`.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Written to stderr right before Node prints an uncaught error.
const MARKER: &str = "\u{0}opal-oracle-uncaught\u{0}";

/// Preloaded into both runs. It deletes its own environment variable, so the
/// program sees the same `process.env` in both, and it leaves an error alone
/// when the program has its own `uncaughtException` handler: that error is
/// not uncaught, and output after it is still the program's. That includes
/// an error whose origin is `unhandledRejection`, which is how Node 24
/// reports a throw at the top level of an ES module: module evaluation is a
/// promise, and the handler receives the rejection.
const HOOK: &str = r#"'use strict';
const { closeSync, openSync, writeSync } = require('node:fs');
const report = process.env.OPAL_ORACLE_REPORT;
delete process.env.OPAL_ORACLE_REPORT;
process.on('uncaughtExceptionMonitor', (error, origin) => {
  if (process.listenerCount('uncaughtException') > 0) {
    return;
  }
  let name = null;
  let message = null;
  try {
    if (error !== null && (typeof error === 'object' || typeof error === 'function')) {
      name = String(error.constructor?.name);
      message = 'message' in error ? String(error.message) : null;
    } else {
      name = typeof error;
      message = String(error);
    }
  } catch {
    name = '<unreadable>';
  }
  writeSync(2, '\u0000opal-oracle-uncaught\u0000');
  if (report) {
    const fd = openSync(report, 'w');
    writeSync(fd, JSON.stringify({ origin, name, message }));
    closeSync(fd);
  }
});
"#;

/// What an observer of one run could see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub stdout: Vec<u8>,
    /// Stderr up to the point Node starts printing an uncaught error.
    pub stderr: Vec<u8>,
    /// `None` when the process was killed by a signal.
    pub exit_code: Option<i32>,
    pub uncaught: Option<Uncaught>,
    pub timed_out: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uncaught {
    /// `uncaughtException` or `unhandledRejection`.
    pub origin: String,
    /// The thrown value's constructor name, or `typeof` for a primitive.
    pub name: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("could not start {node}: {source}")]
    Spawn {
        node: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("the oracle hook wrote a report that is not JSON: {0}")]
    Report(String),
}

/// How to run Node.
#[derive(Clone, Debug)]
pub struct Node {
    program: String,
    timeout: Duration,
    /// Set for both runs, so a fixture can give the program its environment.
    env: Vec<(String, String)>,
    /// Extra node flags, for one run.
    flags: Vec<String>,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            program: std::env::var("OPAL_NODE").unwrap_or_else(|_| "node".to_string()),
            timeout: Duration::from_secs(20),
            env: Vec::new(),
            flags: Vec::new(),
        }
    }
}

impl Node {
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_flags(mut self, flags: &[String]) -> Self {
        self.flags.extend(flags.iter().cloned());
        self
    }

    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    /// `node --version`, for reports.
    pub fn version(&self) -> Result<String, OracleError> {
        let output = Command::new(&self.program)
            .arg("--version")
            .output()
            .map_err(|source| OracleError::Spawn {
                node: self.program.clone(),
                source,
            })?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Runs `node <file>` with `cwd` as the working directory. `scratch` is
    /// a directory the oracle may write its hook and report into; it must
    /// not be inside the program's own tree, or the program could see it.
    pub fn observe(
        &self,
        cwd: &Path,
        file: &Path,
        scratch: &Path,
    ) -> Result<Observation, OracleError> {
        std::fs::create_dir_all(scratch)?;
        let hook = scratch.join("opal-oracle-hook.cjs");
        if !hook.exists() {
            std::fs::write(&hook, HOOK)?;
        }
        let report = unique_report_path(scratch);
        let _ = std::fs::remove_file(&report);

        let mut child = Command::new(&self.program)
            .arg("--disable-warning=ExperimentalWarning")
            .args(&self.flags)
            .arg("--require")
            .arg(&hook)
            .arg(file)
            .current_dir(cwd)
            .env("OPAL_ORACLE_REPORT", &report)
            .env_remove("NODE_OPTIONS")
            .env("TZ", "UTC")
            .env("LANG", "C")
            .env("NO_COLOR", "1")
            .envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| OracleError::Spawn {
                node: self.program.clone(),
                source,
            })?;

        let mut stdout_pipe = child.stdout.take().expect("stdout is piped");
        let mut stderr_pipe = child.stderr.take().expect("stderr is piped");
        let stdout_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout_pipe.read_to_end(&mut bytes);
            bytes
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut bytes);
            bytes
        });

        let started = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > self.timeout {
                timed_out = true;
                let _ = child.kill();
                break child.wait()?;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        let stdout = stdout_reader.join().unwrap_or_default();
        let mut stderr = stderr_reader.join().unwrap_or_default();
        if let Some(at) = find(&stderr, MARKER.as_bytes()) {
            stderr.truncate(at);
        }

        let uncaught = match std::fs::read_to_string(&report) {
            Ok(text) if !text.is_empty() => Some(parse_report(&text)?),
            _ => None,
        };
        let _ = std::fs::remove_file(&report);

        Ok(Observation {
            stdout,
            stderr,
            exit_code: status.code(),
            uncaught,
            timed_out,
        })
    }
}

fn unique_report_path(scratch: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    scratch.join(format!("report-{}-{n}.json", std::process::id()))
}

fn parse_report(text: &str) -> Result<Uncaught, OracleError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| OracleError::Report(text.to_string()))?;
    let field = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
    Ok(Uncaught {
        origin: field("origin").unwrap_or_default(),
        name: field("name"),
        message: field("message"),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// `file:///…` for an absolute path, percent-encoding what a URL path can't
/// hold. Node's `--import` takes a URL or a specifier, not a bare path.
pub fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                url.push(byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

/// A difference between two observations, in the terms of `SEMANTICS.md`.
#[derive(Debug, PartialEq, Eq)]
pub struct Divergence {
    pub differences: Vec<String>,
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for difference in &self.differences {
            writeln!(f, "{difference}")?;
        }
        Ok(())
    }
}

/// Compares what an observer of the original and of the bundle saw.
pub fn compare(original: &Observation, bundle: &Observation) -> Result<(), Divergence> {
    let mut differences = Vec::new();
    if original.timed_out || bundle.timed_out {
        differences.push(format!(
            "timed out: original {}, bundle {}",
            original.timed_out, bundle.timed_out
        ));
    }
    if original.stdout != bundle.stdout {
        differences.push(describe_bytes("stdout", &original.stdout, &bundle.stdout));
    }
    if original.stderr != bundle.stderr {
        differences.push(describe_bytes("stderr", &original.stderr, &bundle.stderr));
    }
    if original.exit_code != bundle.exit_code {
        differences.push(format!(
            "exit code: original {:?}, bundle {:?}",
            original.exit_code, bundle.exit_code
        ));
    }
    if original.uncaught != bundle.uncaught {
        differences.push(format!(
            "uncaught error: original {:?}, bundle {:?}",
            original.uncaught, bundle.uncaught
        ));
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(Divergence { differences })
    }
}

fn describe_bytes(stream: &str, original: &[u8], bundle: &[u8]) -> String {
    let original = String::from_utf8_lossy(original);
    let bundle = String::from_utf8_lossy(bundle);
    let original_lines: Vec<&str> = original.lines().collect();
    let bundle_lines: Vec<&str> = bundle.lines().collect();
    let first = original_lines
        .iter()
        .zip(&bundle_lines)
        .position(|(a, b)| a != b)
        .unwrap_or(original_lines.len().min(bundle_lines.len()));
    format!(
        "{stream} differs from line {}:\n  original: {:?}\n  bundle:   {:?}",
        first + 1,
        original_lines.get(first).copied().unwrap_or("<end>"),
        bundle_lines.get(first).copied().unwrap_or("<end>"),
    )
}
