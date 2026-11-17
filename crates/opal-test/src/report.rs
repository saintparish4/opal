//! Adding up per-file results and reporting them the way Jest does.
//!
//! The counting follows `@jest/test-result`'s `addResult` and the JSON
//! follows `formatTestResults`, field for field, because `--json` output is
//! read by tools that were written against Jest's. The console report is
//! Jest's default reporter in outline: a PASS or FAIL line per file, the
//! failures under it, and the totals. Like Jest's, it goes to stderr.

use std::io::{self, Write};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

const EMPTY_SUITE: &str = "Your test suite must contain at least one test.";

pub struct Aggregate {
    root: std::path::PathBuf,
    started: Instant,
    start_time: u64,
    results: Vec<Value>,
    pub failed_suites: u64,
    pub passed_suites: u64,
    pub pending_suites: u64,
    pub runtime_error_suites: u64,
    pub failed_tests: u64,
    pub passed_tests: u64,
    pub pending_tests: u64,
    pub todo_tests: u64,
    snapshot: Map<String, Value>,
}

fn count(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

impl Aggregate {
    pub fn new(root: &Path) -> Self {
        let mut snapshot = Map::new();
        for key in [
            "added",
            "filesAdded",
            "filesRemoved",
            "filesUnmatched",
            "filesUpdated",
            "matched",
            "total",
            "unchecked",
            "unmatched",
            "updated",
        ] {
            snapshot.insert(key.into(), json!(0));
        }
        snapshot.insert("didUpdate".into(), json!(false));
        snapshot.insert("failure".into(), json!(false));
        snapshot.insert("filesRemovedList".into(), json!([]));
        snapshot.insert("uncheckedKeysByFile".into(), json!([]));
        Aggregate {
            root: root.to_path_buf(),
            started: Instant::now(),
            start_time: now_millis(),
            results: Vec::new(),
            failed_suites: 0,
            passed_suites: 0,
            pending_suites: 0,
            runtime_error_suites: 0,
            failed_tests: 0,
            passed_tests: 0,
            pending_tests: 0,
            todo_tests: 0,
            snapshot,
        }
    }

    /// Adds one file's result, after Jest's scheduler has had its say: a file
    /// that ran and declared no tests at all failed.
    pub fn add(&mut self, mut result: Value, out: &mut dyn Write) -> io::Result<()> {
        let no_tests = result["testResults"].as_array().is_none_or(Vec::is_empty);
        if no_tests && result["testExecError"].is_null() {
            result["testExecError"] = json!({ "message": EMPTY_SUITE, "stack": EMPTY_SUITE });
            result["failureMessage"] = json!(format!(
                "  ● Test suite failed to run\n\n    {EMPTY_SUITE}\n"
            ));
        }
        let failing = count(&result, "numFailingTests");
        self.failed_tests += failing;
        self.passed_tests += count(&result, "numPassingTests");
        self.pending_tests += count(&result, "numPendingTests");
        self.todo_tests += count(&result, "numTodoTests");
        let exec_error = !result["testExecError"].is_null();
        if exec_error {
            self.runtime_error_suites += 1;
        }
        if result["skipped"] == true {
            self.pending_suites += 1;
        } else if failing > 0 || exec_error {
            self.failed_suites += 1;
        } else {
            self.passed_suites += 1;
        }
        let snap = &result["snapshot"];
        for (flag, key) in [
            ("added", "filesAdded"),
            ("fileDeleted", "filesRemoved"),
            ("unmatched", "filesUnmatched"),
            ("updated", "filesUpdated"),
        ] {
            if snap[flag].as_u64().unwrap_or(0) > 0 || snap[flag] == true {
                *self.snapshot.get_mut(key).expect("present") =
                    json!(self.snapshot[key].as_u64().unwrap_or(0) + 1);
            }
        }
        for key in ["added", "matched", "unchecked", "unmatched", "updated"] {
            let total = self.snapshot[key].as_u64().unwrap_or(0) + count(snap, key);
            self.snapshot.insert(key.into(), json!(total));
        }
        let total = self.snapshot["total"].as_u64().unwrap_or(0)
            + count(snap, "added")
            + count(snap, "matched")
            + count(snap, "unmatched")
            + count(snap, "updated");
        self.snapshot.insert("total".into(), json!(total));
        if let Some(keys) = snap["uncheckedKeys"].as_array().filter(|k| !k.is_empty()) {
            let list = self.snapshot["uncheckedKeysByFile"]
                .as_array_mut()
                .expect("present");
            list.push(json!({ "filePath": result["testFilePath"], "keys": keys }));
        }
        // Kept before printing: a failed write to the terminal must not lose
        // a result from the counts or the JSON.
        self.results.push(result);
        self.print_file(self.results.last().expect("just pushed"), out)
    }

    fn relative<'a>(&self, path: &'a str) -> std::borrow::Cow<'a, str> {
        match Path::new(path).strip_prefix(&self.root) {
            Ok(relative) => relative.to_string_lossy().into_owned().into(),
            Err(_) => path.into(),
        }
    }

    fn print_file(&self, result: &Value, out: &mut dyn Write) -> io::Result<()> {
        let path = result["testFilePath"].as_str().unwrap_or("");
        let failed = count(result, "numFailingTests") > 0 || !result["testExecError"].is_null();
        let seconds = count(&result["perfStats"], "runtime") as f64 / 1000.0;
        let timing = if seconds >= 1.0 {
            format!(" ({seconds:.3} s)")
        } else {
            String::new()
        };
        writeln!(
            out,
            "{} {}{timing}",
            if failed { "FAIL" } else { "PASS" },
            self.relative(path)
        )?;
        if let Some(entries) = result["console"].as_array() {
            for entry in entries {
                writeln!(out, "  console.{}", entry["type"].as_str().unwrap_or("log"))?;
                for line in entry["message"].as_str().unwrap_or("").lines() {
                    writeln!(out, "    {line}")?;
                }
                writeln!(out)?;
            }
        }
        if let Some(message) = result["failureMessage"]
            .as_str()
            .filter(|m| failed && !m.is_empty())
        {
            writeln!(out, "{}", message.trim_end())?;
            writeln!(out)?;
        }
        Ok(())
    }

    pub fn success(&self) -> bool {
        self.failed_suites == 0
            && self.runtime_error_suites == 0
            && self.snapshot["failure"] != true
    }

    pub fn print_summary(&self, out: &mut dyn Write) -> io::Result<()> {
        let suites_total = self.failed_suites + self.passed_suites + self.pending_suites;
        let tests_total =
            self.failed_tests + self.passed_tests + self.pending_tests + self.todo_tests;
        let line = |parts: &[(u64, &str)], total: u64| {
            let mut words: Vec<String> = parts
                .iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, label)| format!("{n} {label}"))
                .collect();
            words.push(format!("{total} total"));
            words.join(", ")
        };
        writeln!(
            out,
            "Test Suites: {}",
            line(
                &[
                    (self.failed_suites, "failed"),
                    (self.pending_suites, "skipped"),
                    (self.passed_suites, "passed")
                ],
                suites_total
            )
        )?;
        writeln!(
            out,
            "Tests:       {}",
            line(
                &[
                    (self.failed_tests, "failed"),
                    (self.pending_tests, "skipped"),
                    (self.todo_tests, "todo"),
                    (self.passed_tests, "passed")
                ],
                tests_total
            )
        )?;
        let snap = |key: &str| self.snapshot[key].as_u64().unwrap_or(0);
        writeln!(
            out,
            "Snapshots:   {}",
            line(
                &[
                    (snap("unmatched"), "failed"),
                    (snap("unchecked"), "obsolete"),
                    (snap("updated"), "updated"),
                    (snap("added"), "written"),
                    (snap("matched"), "passed")
                ],
                snap("total")
            )
        )?;
        writeln!(
            out,
            "Time:        {:.3} s",
            self.started.elapsed().as_secs_f64()
        )?;
        Ok(())
    }

    /// Jest's `--json` output.
    pub fn to_json(&self) -> Value {
        let tests_total =
            self.failed_tests + self.passed_tests + self.pending_tests + self.todo_tests;
        let test_results: Vec<Value> = self.results.iter().map(format_file).collect();
        json!({
            "numFailedTestSuites": self.failed_suites,
            "numFailedTests": self.failed_tests,
            "numPassedTestSuites": self.passed_suites,
            "numPassedTests": self.passed_tests,
            "numPendingTestSuites": self.pending_suites,
            "numPendingTests": self.pending_tests,
            "numRuntimeErrorTestSuites": self.runtime_error_suites,
            "numTodoTests": self.todo_tests,
            "numTotalTestSuites": self.failed_suites + self.passed_suites + self.pending_suites,
            "numTotalTests": tests_total,
            "openHandles": [],
            "snapshot": Value::Object(self.snapshot.clone()),
            "startTime": self.start_time,
            "success": self.success(),
            "testResults": test_results,
            "wasInterrupted": false,
        })
    }
}

fn format_file(result: &Value) -> Value {
    let message = result["failureMessage"].as_str().unwrap_or("");
    let perf = &result["perfStats"];
    let status = if !result["testExecError"].is_null() || count(result, "numFailingTests") > 0 {
        "failed"
    } else if result["skipped"] == true {
        "skipped"
    } else if count(result, "numPendingTests") > 0 {
        "focused"
    } else {
        "passed"
    };
    let message = if message.is_empty() && !result["testExecError"].is_null() {
        result["testExecError"]["message"].as_str().unwrap_or("")
    } else {
        message
    };
    json!({
        "assertionResults": result["testResults"],
        "coverage": {},
        "endTime": perf["end"],
        "message": message,
        "name": result["testFilePath"],
        "startTime": perf["start"],
        "status": status,
        "summary": "",
    })
}
