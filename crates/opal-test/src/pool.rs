//! Node workers and the channel to them.
//!
//! Each worker is a long-lived `node` running the embedded runtime's
//! `worker.cjs`, and runs one test file at a time, each in a fresh `vm`
//! context. Workers talk to opal over a loopback TCP connection, one JSON
//! message per line, rather than over stdout: a test that writes to stdout
//! must not be able to corrupt the channel, and the same transport works on
//! every platform. Each connection opens with the token opal handed the
//! worker in its environment, so nothing else on the machine can pose as one.
//!
//! Files are handed out one at a time as workers become free, which balances
//! the load without knowing how long each file takes.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::TestError;
use crate::service::TransformService;

/// How long a worker has to start Node and connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

enum Event {
    Connected(usize, TcpStream),
    Message(usize, Value),
    Closed(usize),
}

struct Slot {
    child: Child,
    stream: Option<TcpStream>,
    /// The file it is running, and when it started.
    job: Option<(PathBuf, Instant)>,
    spawned: Instant,
}

pub struct Pool {
    node: PathBuf,
    worker_script: PathBuf,
    port: u16,
    token: String,
    events: Receiver<Event>,
    sender: Sender<Event>,
    slots: HashMap<usize, Slot>,
    next_id: usize,
}

/// What a worker reported for one file, or what opal says about a file whose
/// worker died running it.
pub struct FileOutcome {
    pub path: PathBuf,
    pub result: Value,
}

impl Pool {
    pub fn start(
        node: &Path,
        worker_script: &Path,
        transforms: Arc<TransformService>,
    ) -> Result<Pool, TestError> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(TestError::Channel)?;
        let port = listener.local_addr().map_err(TestError::Channel)?.port();
        let token = format!("{:016x}{:016x}", std::process::id(), random_u64());
        let (sender, events) = mpsc::channel();
        let accept_sender = sender.clone();
        let accept_token = token.clone();
        thread::spawn(move || accept_loop(listener, &accept_token, &accept_sender, &transforms));
        Ok(Pool {
            node: node.to_path_buf(),
            worker_script: worker_script.to_path_buf(),
            port,
            token,
            events,
            sender,
            slots: HashMap::new(),
            next_id: 0,
        })
    }

    pub fn spawn(&mut self) -> Result<usize, TestError> {
        let id = self.next_id;
        self.next_id += 1;
        let child = Command::new(&self.node)
            .arg(&self.worker_script)
            .env("OPAL_TEST_PORT", self.port.to_string())
            .env("OPAL_TEST_TOKEN", &self.token)
            .env("OPAL_TEST_WORKER", id.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| TestError::Node {
                node: self.node.clone(),
                source,
            })?;
        // What a test writes to the real stdout or stderr is copied through
        // here rather than inherited: Node puts an inherited pipe into
        // non-blocking mode, and opal's own writes to it would then fail
        // whenever it filled.
        let mut child = child;
        if let Some(stdout) = child.stdout.take() {
            thread::spawn(move || forward(stdout, io::stdout()));
        }
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || forward(stderr, io::stderr()));
        }
        self.slots.insert(
            id,
            Slot {
                child,
                stream: None,
                job: None,
                spawned: Instant::now(),
            },
        );
        Ok(id)
    }

    fn send(&mut self, id: usize, message: &Value) -> io::Result<()> {
        let slot = self.slots.get_mut(&id).expect("a live worker");
        let stream = slot.stream.as_mut().expect("a connected worker");
        let mut line = serde_json::to_vec(message).expect("serializable");
        line.push(b'\n');
        stream.write_all(&line)
    }

    /// Waits for the next event, failing if a worker never connects.
    fn next_event(&mut self) -> Result<Event, TestError> {
        loop {
            match self.events.recv_timeout(Duration::from_millis(500)) {
                Ok(event) => return Ok(event),
                Err(RecvTimeoutError::Timeout) => {
                    for (id, slot) in &mut self.slots {
                        if slot.stream.is_none() {
                            if let Ok(Some(status)) = slot.child.try_wait() {
                                return Err(TestError::WorkerExited(*id, status.to_string()));
                            }
                            if slot.spawned.elapsed() > CONNECT_TIMEOUT {
                                return Err(TestError::WorkerTimeout(*id));
                            }
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => unreachable!("the pool holds a sender"),
            }
        }
    }

    fn on_connected(&mut self, id: usize, stream: TcpStream) {
        match self.slots.get_mut(&id) {
            Some(slot) => slot.stream = Some(stream),
            None => drop(stream),
        }
    }

    /// Sends `request` to the first worker and returns its reply.
    pub fn request(&mut self, request: Value) -> Result<Value, TestError> {
        let id = *self.slots.keys().min().expect("a worker");
        loop {
            if self.slots[&id].stream.is_some() {
                break;
            }
            match self.next_event()? {
                Event::Connected(worker, stream) => self.on_connected(worker, stream),
                Event::Closed(worker) if worker == id => {
                    return Err(TestError::WorkerExited(id, "before replying".into()));
                }
                _ => {}
            }
        }
        self.send(id, &request).map_err(TestError::Channel)?;
        loop {
            match self.next_event()? {
                Event::Connected(worker, stream) => self.on_connected(worker, stream),
                Event::Message(worker, message) if worker == id => return Ok(message),
                Event::Closed(worker) if worker == id => {
                    return Err(TestError::WorkerExited(id, "before replying".into()));
                }
                _ => {}
            }
        }
    }

    /// Runs every file, calling `on_result` as each finishes. `max_workers`
    /// caps how many run at once; the pool grows to it as files remain.
    pub fn run(
        &mut self,
        files: Vec<PathBuf>,
        context: &Value,
        max_workers: usize,
        mut on_result: impl FnMut(FileOutcome),
    ) -> Result<(), TestError> {
        let mut queue: VecDeque<PathBuf> = files.into();
        let wanted = max_workers.max(1).min(queue.len().max(1));
        while self.slots.len() < wanted {
            self.spawn()?;
        }
        loop {
            let idle: Vec<usize> = self
                .slots
                .iter()
                .filter(|(_, slot)| slot.stream.is_some() && slot.job.is_none())
                .map(|(id, _)| *id)
                .collect();
            for id in idle {
                let Some(path) = queue.pop_front() else { break };
                let message = json!({ "type": "run", "path": path, "context": context });
                if self.send(id, &message).is_err() {
                    queue.push_front(path);
                    continue;
                }
                self.slots.get_mut(&id).expect("live").job = Some((path, Instant::now()));
            }
            let busy = self.slots.values().any(|slot| slot.job.is_some());
            if queue.is_empty() && !busy {
                return Ok(());
            }
            match self.next_event()? {
                Event::Connected(id, stream) => self.on_connected(id, stream),
                Event::Message(id, message) => {
                    if message["type"] == "result" {
                        let slot = self.slots.get_mut(&id).expect("live");
                        if let Some((path, _)) = slot.job.take() {
                            on_result(FileOutcome {
                                path,
                                result: message["result"].clone(),
                            });
                        }
                    }
                }
                Event::Closed(id) => {
                    let Some(mut slot) = self.slots.remove(&id) else {
                        continue;
                    };
                    let status = slot.child.wait().map(|s| s.to_string()).unwrap_or_default();
                    if let Some((path, started)) = slot.job.take() {
                        on_result(FileOutcome {
                            result: crashed(&path, &status, started.elapsed()),
                            path,
                        });
                    }
                    if !queue.is_empty() && self.slots.len() < wanted {
                        self.spawn()?;
                    }
                }
            }
        }
    }

    pub fn shutdown(mut self) {
        let ids: Vec<usize> = self.slots.keys().copied().collect();
        for id in &ids {
            if self.slots[id].stream.is_some() {
                let _ = self.send(*id, &json!({ "type": "exit" }));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        for (_, mut slot) in self.slots.drain() {
            loop {
                match slot.child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    _ => {
                        let _ = slot.child.kill();
                        let _ = slot.child.wait();
                        break;
                    }
                }
            }
        }
        drop(self.sender);
    }
}

fn forward(mut from: impl io::Read, to: impl Write) {
    let mut to = to;
    let mut buffer = [0u8; 8192];
    loop {
        match from.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                if to.write_all(&buffer[..n]).is_err() {
                    return;
                }
                let _ = to.flush();
            }
        }
    }
}

/// The result Jest reports for a file whose worker went away mid-run.
fn crashed(path: &Path, status: &str, elapsed: Duration) -> Value {
    let message = format!(
        "Test suite failed to run\n\n    The worker running this file exited unexpectedly ({status})."
    );
    json!({
        "testFilePath": path,
        "testExecError": { "message": message, "stack": message },
        "failureMessage": format!("  ● {message}"),
        "numFailingTests": 0,
        "numPassingTests": 0,
        "numPendingTests": 0,
        "numTodoTests": 0,
        "testResults": [],
        "perfStats": { "runtime": elapsed.as_millis() as u64 },
    })
}

fn accept_loop(
    listener: TcpListener,
    token: &str,
    events: &Sender<Event>,
    transforms: &Arc<TransformService>,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let events = events.clone();
        let token = token.to_string();
        let transforms = Arc::clone(transforms);
        thread::spawn(move || {
            let _ = stream.set_nodelay(true);
            let Ok(reader_stream) = stream.try_clone() else {
                return;
            };
            let mut reader = BufReader::new(reader_stream);
            let mut hello = String::new();
            if reader.read_line(&mut hello).is_err() {
                return;
            }
            let Ok(hello) = serde_json::from_str::<Value>(&hello) else {
                return;
            };
            if hello["token"] != token.as_str() {
                return;
            }
            let Some(id) = hello["worker"].as_u64().map(|id| id as usize) else {
                return;
            };
            if hello["role"] == "transform" {
                serve_transforms(reader, stream, &transforms);
                return;
            }
            if events.send(Event::Connected(id, stream)).is_err() {
                return;
            }
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => match serde_json::from_str(&line) {
                        Ok(message) => {
                            if events.send(Event::Message(id, message)).is_err() {
                                return;
                            }
                        }
                        Err(_) => break,
                    },
                }
            }
            let _ = events.send(Event::Closed(id));
        });
    }
}

/// A worker's transform connection: one request per line, one reply each.
fn serve_transforms(
    mut reader: BufReader<TcpStream>,
    mut stream: TcpStream,
    service: &TransformService,
) {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => service.answer(&request),
            Err(error) => json!({ "error": format!("unreadable transform request: {error}") }),
        };
        let mut bytes = serde_json::to_vec(&reply).expect("serializable");
        bytes.push(b'\n');
        if stream.write_all(&bytes).is_err() {
            return;
        }
    }
}

/// A token nothing else on the machine can guess; not a key for anything.
fn random_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    );
    hasher.finish()
}
