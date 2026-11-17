//! `opal test --watch` and `--watchAll`, driven for real: a project in a git
//! repository, files edited while it watches, and the test files each rerun
//! takes on read from what it prints.
//!
//! Which tests a change relates to is held to Jest's answer by
//! compat/test-related.py on random projects. This checks the loop around
//! that query: the first run starts from git, each later one from what
//! changed since the run before, new files count, and the workers survive
//! between runs.
//!
//! Needs `node` and `git` on PATH, as `opal test` does.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const OPAL: &str = env!("CARGO_BIN_EXE_opal");
const WAITING: &str = "Watching for file changes";

struct Watching {
    child: Child,
    lines: Receiver<String>,
    _cache: tempfile::TempDir,
}

impl Watching {
    fn start(project: &Path, flag: &str) -> Watching {
        // Outside the project: the watcher mustn't see the cache being written.
        let cache = tempfile::tempdir().expect("tempdir");
        let mut child = Command::new(OPAL)
            .args(["test", flag])
            .current_dir(project)
            .env("OPAL_CACHE_DIR", cache.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("opal starts");
        let (sender, lines) = mpsc::channel();
        for stream in [
            Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
            Box::new(child.stderr.take().unwrap()),
        ] {
            let sender = sender.clone();
            thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let _ = sender.send(line);
                }
            });
        }
        Watching {
            child,
            lines,
            _cache: cache,
        }
    }

    /// Everything printed up to the next "Watching for file changes".
    fn next_run(&self) -> Vec<String> {
        let mut seen = Vec::new();
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|_| panic!("no run finished; printed so far: {seen:#?}"));
            if line.contains(WAITING) {
                return seen;
            }
            seen.push(line);
        }
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The test files a run took on, from its PASS and FAIL lines.
fn ran(output: &[String]) -> Vec<String> {
    let mut files: Vec<String> = output
        .iter()
        .filter_map(|line| {
            line.strip_prefix("PASS ")
                .or_else(|| line.strip_prefix("FAIL "))
        })
        .map(|rest| rest.split_whitespace().next().unwrap_or("").to_string())
        .collect();
    files.sort();
    files
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let write = |path: &str, text: &str| {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write("package.json", r#"{"name": "watched"}"#);
    write("src/a.js", "module.exports = 1;\n");
    write("src/b.js", "module.exports = 2;\n");
    write("src/uses-a.js", "module.exports = require('./a') + 1;\n");
    write(
        "src/a.test.js",
        "test('a', () => expect(require('./uses-a')).toBe(2));\n",
    );
    write(
        "src/b.test.js",
        "test('b', () => expect(require('./b')).toBe(2));\n",
    );
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t"])
            .args(args)
            .current_dir(dir.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "start"]);
    dir
}

/// Writes after a pause, so the change can't fall inside the run before it.
fn edit(project: &Path, path: &str, text: &str) {
    thread::sleep(Duration::from_millis(200));
    fs::write(project.join(path), text).unwrap();
}

#[test]
fn test_watch_reruns_the_tests_each_change_relates_to() {
    let dir = project();
    let root = dir.path();
    let watching = Watching::start(root, "--watch");

    let first = watching.next_run();
    assert!(
        ran(&first).is_empty(),
        "nothing has changed since the commit: {first:#?}"
    );

    edit(root, "src/a.js", "module.exports = 1; // edited\n");
    assert_eq!(ran(&watching.next_run()), vec!["src/a.test.js"]);

    edit(
        root,
        "src/b.test.js",
        "test('b', () => expect(require('./b')).toBe(2)); // edited\n",
    );
    assert_eq!(ran(&watching.next_run()), vec!["src/b.test.js"]);

    edit(
        root,
        "src/c.test.js",
        "test('c', () => expect(require('./a')).toBe(1));\n",
    );
    assert_eq!(ran(&watching.next_run()), vec!["src/c.test.js"]);

    // The new test depends on a.js too now.
    edit(root, "src/a.js", "module.exports = 1; // edited again\n");
    assert_eq!(
        ran(&watching.next_run()),
        vec!["src/a.test.js", "src/c.test.js"]
    );
}

#[test]
fn test_watch_starts_from_what_git_says_changed() {
    let dir = project();
    let root = dir.path();
    fs::write(
        root.join("src/b.js"),
        "module.exports = 2; // uncommitted\n",
    )
    .unwrap();
    let watching = Watching::start(root, "--watch");
    assert_eq!(ran(&watching.next_run()), vec!["src/b.test.js"]);
}

#[test]
fn test_watch_all_reruns_everything() {
    let dir = project();
    let root = dir.path();
    let watching = Watching::start(root, "--watchAll");
    assert_eq!(
        ran(&watching.next_run()),
        vec!["src/a.test.js", "src/b.test.js"]
    );

    edit(root, "src/b.js", "module.exports = 2; // edited\n");
    assert_eq!(
        ran(&watching.next_run()),
        vec!["src/a.test.js", "src/b.test.js"]
    );
}
