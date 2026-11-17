//! The run's type-check, for a project whose transform is ts-jest.
//!
//! Workers run ts-jest to emit only (transformers.cjs), and ask, for each
//! file it emits, what its type-check would have thrown. One Node process
//! answers for the whole run (checker.cjs), started with the run so it can
//! work ahead through the test files in the order they're handed out. A
//! worker's question arrives on that worker's transform connection, on its
//! own thread, and waits there for the answer; questions from different
//! workers are in flight at once.
//!
//! If the checker dies, every question from then on is answered with an
//! error, so a file whose types weren't checked fails rather than passes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde_json::{Value, json};

/// How long the end of a run waits for the checker to save its state.
const SAVE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct TypeChecker {
    session: RwLock<Option<Arc<Session>>>,
}

/// Question id -> where its answer goes; `None` once the checker is gone.
type Waiting = Arc<Mutex<Option<HashMap<u64, Sender<Value>>>>>;

struct Session {
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    next_id: AtomicU64,
    waiting: Waiting,
    done: Mutex<Receiver<()>>,
}

fn died() -> Value {
    json!({
        "throws": true,
        "message": "opal test: the type check ended before it checked this file",
        "diagnosticText": "opal test: the type check ended before it checked this file",
        "codes": [],
    })
}

impl TypeChecker {
    /// Starts the checker for a run: `config` is the project's, `tests` the
    /// run's test files in the order they're handed out.
    pub fn start(
        &self,
        node: &Path,
        script: &Path,
        cache_dir: &Path,
        config: &Value,
        tests: &[std::path::PathBuf],
    ) {
        self.finish();
        let mut command = Command::new(node);
        command
            .arg(script)
            .env("OPAL_TEST_CACHE", cache_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if std::env::var_os("NODE_ENV").is_none() {
            command.env("NODE_ENV", "test");
        }
        let Ok(mut child) = command.spawn() else {
            return;
        };
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return;
        };
        let start = json!({ "type": "start", "config": config, "tests": tests });
        if writeln!(stdin, "{start}").is_err() {
            let _ = child.kill();
            return;
        }
        let waiting: Waiting = Arc::new(Mutex::new(Some(HashMap::new())));
        let (done_sender, done) = mpsc::channel();
        let reader_waiting = Arc::clone(&waiting);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if message["type"] == "done" {
                    let _ = done_sender.send(());
                    continue;
                }
                let Some(id) = message["id"].as_u64() else {
                    continue;
                };
                let sender = reader_waiting
                    .lock()
                    .expect("lock")
                    .as_mut()
                    .and_then(|waiting| waiting.remove(&id));
                if let Some(sender) = sender {
                    let _ = sender.send(message["verdict"].clone());
                }
            }
            // Whoever is still waiting hears that the checker is gone.
            if let Some(waiting) = reader_waiting.lock().expect("lock").take() {
                for sender in waiting.into_values() {
                    let _ = sender.send(died());
                }
            }
            let _ = done_sender.send(());
        });
        *self.session.write().expect("lock") = Some(Arc::new(Session {
            child: Mutex::new(child),
            stdin: Mutex::new(Some(stdin)),
            next_id: AtomicU64::new(0),
            waiting,
            done: Mutex::new(done),
        }));
    }

    /// What the type-check has to say about `path`, under the transform
    /// entry `entry`: null, or the TSError ts-jest would throw. Null when no
    /// checker runs.
    pub fn ask(&self, path: &str, entry: u64) -> Value {
        let Some(session) = self.session.read().expect("lock").clone() else {
            return Value::Null;
        };
        let id = session.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, answer) = mpsc::channel();
        match session.waiting.lock().expect("lock").as_mut() {
            Some(waiting) => {
                waiting.insert(id, sender);
            }
            None => return died(),
        }
        let question = json!({ "type": "check", "id": id, "path": path, "entry": entry });
        let sent = session
            .stdin
            .lock()
            .expect("lock")
            .as_mut()
            .is_some_and(|stdin| writeln!(stdin, "{question}").is_ok());
        if !sent {
            if let Some(waiting) = session.waiting.lock().expect("lock").as_mut() {
                waiting.remove(&id);
            }
            return died();
        }
        answer.recv().unwrap_or_else(|_| died())
    }

    /// Ends the run's checker, once it has saved what it learned.
    pub fn finish(&self) {
        let Some(session) = self.session.write().expect("lock").take() else {
            return;
        };
        if let Some(mut stdin) = session.stdin.lock().expect("lock").take() {
            let _ = writeln!(stdin, "{}", json!({ "type": "finish" }));
        }
        let saved = session
            .done
            .lock()
            .expect("lock")
            .recv_timeout(SAVE_TIMEOUT)
            .is_ok();
        let mut child = session.child.lock().expect("lock");
        if !saved {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checker script in a scratch folder, run by the `node` on PATH.
    fn checker(script: &str) -> (tempfile::TempDir, TypeChecker) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("checker.cjs"), script).expect("write");
        let checker = TypeChecker::default();
        checker.start(
            Path::new("node"),
            &dir.path().join("checker.cjs"),
            dir.path(),
            &json!({}),
            &[],
        );
        (dir, checker)
    }

    #[test]
    fn test_no_checker_means_nothing_to_say() {
        assert_eq!(TypeChecker::default().ask("/a.ts", 0), Value::Null);
    }

    #[test]
    fn test_each_answer_reaches_its_question() {
        // Answers out of order: the second question's first.
        let (_dir, checker) = checker(
            r#"
            const lines = require('readline').createInterface({input: process.stdin});
            const held = [];
            lines.on('line', line => {
              const message = JSON.parse(line);
              if (message.type === 'check') held.push(message);
              if (held.length === 2) for (const {id, path} of held.reverse()) {
                process.stdout.write(JSON.stringify({id, verdict: {message: path}}) + '\n');
              }
              if (message.type === 'finish') process.stdout.write('{"type":"done"}\n');
            });
            "#,
        );
        let checker = Arc::new(checker);
        let asks: Vec<_> = ["/a.ts", "/b.ts"]
            .into_iter()
            .map(|file| {
                let checker = Arc::clone(&checker);
                std::thread::spawn(move || (file, checker.ask(file, 0)))
            })
            .collect();
        for ask in asks {
            let (file, verdict) = ask.join().expect("join");
            assert_eq!(verdict["message"], file);
        }
        checker.finish();
    }

    #[test]
    fn test_a_checker_that_dies_fails_the_file() {
        let (_dir, checker) = checker("process.stdin.once('data', () => process.exit(3));");
        let verdict = checker.ask("/a.ts", 0);
        assert_eq!(verdict["throws"], true);
        assert_eq!(checker.ask("/b.ts", 0)["throws"], true);
        checker.finish();
    }
}
