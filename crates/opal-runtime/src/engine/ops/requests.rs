//! The event loop's I/O: requests made by fs's callback and promise forms,
//! and the poll phase that delivers what they did.
//!
//! A binding function (fs.rs) checks its arguments, then hands [`dispatch`]
//! a job: a closure that makes the system call and returns a [`Done`] or a
//! [`Failure`], plain Rust data. Called without a request (the sync form),
//! the job runs at once and the caller returns or throws its result. Called
//! with one (an FSReqCallback, or kUsePromises for a promise), the job runs
//! at once too when it can't block, as a call on a regular file can't, and
//! on a small pool of threads when it can (a pipe, a FIFO, a terminal, or a
//! large transfer), and either way its result waits for the loop's poll
//! phase. There the loop (engine/mod.rs) takes each finished request in
//! turn and calls its `oncomplete`, or settles its promise, as Node's
//! FSReqCallback and FSReqPromise do (node_file.cc): never during the call
//! that made it, and with ticks and microtasks drained after each.
//!
//! libuv runs every fs request on its thread pool, which on this machine
//! made node's readFile of 1,000 small files about three times slower than
//! its readFileSync of them. Node doesn't promise when, or in what order,
//! requests complete, so running the ones that can't block in place is
//! still Node as a script sees it, and the pool is left for those that
//! would stall the loop.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::buffer::{Encoding, decode};
use super::{string, type_error};

/// What a finished call gives back.
pub enum Done {
    Undefined,
    Number(f64),
    /// A path or name as UTF-8 (mkdir's first directory made).
    Text(Vec<u8>),
    /// Bytes in an encoding, a Buffer for "buffer": readlink, realpath,
    /// mkdtemp.
    Encoded(Vec<u8>, Encoding),
    /// readdir's names, and with withFileTypes their types.
    Names(Vec<Vec<u8>>, Option<Vec<i32>>, Encoding),
    /// The 18 stat fields, as BigInts or not.
    Stats(super::fs::StatFields, bool),
    /// The 8 statfs fields.
    StatFs([i64; 8], bool),
    /// Bytes read, for the buffers the request named.
    Read(Vec<u8>),
}

/// A failed call, as Node's UVException reports it.
pub struct Failure {
    pub errno: i32,
    pub syscall: &'static str,
    pub path: Option<Vec<u8>>,
    pub dest: Option<Vec<u8>>,
}

impl Failure {
    pub fn new(errno: i32, syscall: &'static str) -> Failure {
        Failure {
            errno,
            syscall,
            path: None,
            dest: None,
        }
    }

    pub fn path(errno: i32, syscall: &'static str, path: &[u8]) -> Failure {
        Failure {
            path: Some(path.to_vec()),
            ..Failure::new(errno, syscall)
        }
    }

    pub fn paths(errno: i32, syscall: &'static str, path: &[u8], dest: &[u8]) -> Failure {
        Failure {
            path: Some(path.to_vec()),
            dest: Some(dest.to_vec()),
            ..Failure::new(errno, syscall)
        }
    }
}

pub type Outcome = Result<Done, Failure>;
type Job = Box<dyn FnOnce() -> Outcome + Send>;

/// Where bytes a read returned go: the views of the request, in order.
pub struct Dest(pub Vec<v8::Global<v8::ArrayBufferView>>);

/// Whether a job can block, so it has to run off the loop's thread.
pub enum Blocking {
    No,
    Yes,
}

/// What made the request and how it is told: an FSReqCallback's
/// `oncomplete`, or a promise.
enum Target {
    Callback(v8::Global<v8::Object>),
    Promise(v8::Global<v8::PromiseResolver>),
}

struct Request {
    target: Target,
    dest: Option<Dest>,
}

/// The pool's size, libuv's default (UV_THREADPOOL_SIZE).
const POOL_THREADS: usize = 4;

/// The isolate's requests: those waiting, those done and not yet delivered,
/// and the pool, started the first time a job needs it.
pub struct Io {
    next: u64,
    waiting: HashMap<u64, Request>,
    ready: VecDeque<(u64, Outcome)>,
    results: Sender<(u64, Outcome)>,
    finished: Receiver<(u64, Outcome)>,
    pool: Option<Sender<(u64, Job)>>,
    in_pool: usize,
}

impl Default for Io {
    fn default() -> Io {
        let (results, finished) = channel();
        Io {
            next: 1,
            waiting: HashMap::new(),
            ready: VecDeque::new(),
            results,
            finished,
            pool: None,
            in_pool: 0,
        }
    }
}

fn io<'a>(scope: &'a mut v8::PinScope) -> &'a mut Io {
    if scope.get_slot::<Io>().is_none() {
        scope.set_slot(Io::default());
    }
    scope.get_slot_mut::<Io>().unwrap()
}

impl Io {
    fn pool(&mut self) -> &Sender<(u64, Job)> {
        let results = self.results.clone();
        self.pool.get_or_insert_with(|| {
            let (jobs, queue) = channel::<(u64, Job)>();
            let queue = Arc::new(Mutex::new(queue));
            for n in 0..POOL_THREADS {
                let queue = queue.clone();
                let results = results.clone();
                let _ = std::thread::Builder::new()
                    .name(format!("opal-io-{n}"))
                    .spawn(move || {
                        loop {
                            let next = queue.lock().ok().and_then(|q| q.recv().ok());
                            let Some((id, job)) = next else { return };
                            if results.send((id, job())).is_err() {
                                return;
                            }
                        }
                    });
            }
            jobs
        })
    }
}

/// Runs `job` as the call asked. With no request at `index`, returns its
/// outcome for the caller to return or throw. With one, queues it, sets
/// the return value to the promise when the request is kUsePromises, and
/// returns None.
pub fn dispatch<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    request: v8::Local<'s, v8::Value>,
    rv: &mut v8::ReturnValue<'s, v8::Value>,
    blocking: Blocking,
    dest: Option<Dest>,
    job: impl FnOnce() -> Outcome + Send + 'static,
) -> Option<Outcome> {
    let target = if let Ok(object) = v8::Local::<v8::Object>::try_from(request) {
        Target::Callback(v8::Global::new(scope, object))
    } else if request.is_symbol() {
        let resolver = v8::PromiseResolver::new(scope)?;
        rv.set(resolver.get_promise(scope).into());
        Target::Promise(v8::Global::new(scope, resolver))
    } else {
        return Some(job());
    };
    let state = io(scope);
    let id = state.next;
    state.next += 1;
    state.waiting.insert(id, Request { target, dest });
    match blocking {
        Blocking::No => state.ready.push_back((id, job())),
        Blocking::Yes => {
            state.in_pool += 1;
            let _ = state.pool().send((id, Box::new(job)));
        }
    }
    None
}

/// How many requests are waiting: the loop stays alive while any is.
pub fn pending(scope: &mut v8::PinScope) -> usize {
    scope
        .get_slot::<Io>()
        .map_or(0, |state| state.waiting.len())
}

/// Collects what the pool has finished, waiting up to `timeout` (None:
/// until one finishes) when nothing is ready to deliver yet and the pool
/// is still working.
pub fn poll(scope: &mut v8::PinScope, timeout: Option<Duration>) {
    let Some(state) = scope.get_slot_mut::<Io>() else {
        if let Some(timeout) = timeout {
            std::thread::sleep(timeout);
        }
        return;
    };
    if state.ready.is_empty() && state.in_pool > 0 && timeout != Some(Duration::ZERO) {
        let first = match timeout {
            Some(timeout) => state.finished.recv_timeout(timeout).ok(),
            None => state.finished.recv().ok(),
        };
        if let Some(first) = first {
            state.in_pool -= 1;
            state.ready.push_back(first);
        }
    } else if state.ready.is_empty()
        && state.in_pool == 0
        && let Some(timeout) = timeout
    {
        std::thread::sleep(timeout);
    }
    while let Ok(next) = state.finished.try_recv() {
        state.in_pool -= 1;
        state.ready.push_back(next);
    }
}

/// Whether a finished request is waiting to be delivered, so the loop must
/// not wait.
pub fn ready(scope: &mut v8::PinScope) -> bool {
    scope
        .get_slot::<Io>()
        .is_some_and(|state| !state.ready.is_empty())
}

/// How many finished requests are waiting to be delivered.
pub fn ready_count(scope: &mut v8::PinScope) -> usize {
    scope.get_slot::<Io>().map_or(0, |state| state.ready.len())
}

/// The next finished request's delivery: for a callback, the function to
/// call, what to call it on, and its arguments, for the loop to call as it
/// calls every callback; a promise is settled here, and gives None too.
#[allow(clippy::type_complexity)]
pub fn next<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> Option<
    Option<(
        v8::Local<'s, v8::Function>,
        v8::Local<'s, v8::Value>,
        Vec<v8::Local<'s, v8::Value>>,
    )>,
> {
    let state = scope.get_slot_mut::<Io>()?;
    let (id, outcome) = state.ready.pop_front()?;
    let request = state.waiting.remove(&id)?;
    let promise = matches!(request.target, Target::Promise(_));
    let result = outcome.map(|done| value(scope, done, request.dest.as_ref(), !promise));
    match request.target {
        Target::Promise(resolver) => {
            let resolver = v8::Local::new(scope, resolver);
            match result {
                Ok(value) => resolver.resolve(scope, value),
                Err(failure) => {
                    let error = exception(scope, &failure);
                    resolver.reject(scope, error)
                }
            };
            Some(None)
        }
        Target::Callback(object) => {
            let object = v8::Local::new(scope, object);
            let key = string(scope, "oncomplete");
            let function = object
                .get(scope, key.into())
                .and_then(|f| v8::Local::<v8::Function>::try_from(f).ok());
            let Some(function) = function else {
                return Some(None);
            };
            // FSReqCallback::Resolve passes the value only when there is
            // one: `(null)` for a call that returns nothing.
            let args = match result {
                Ok(value) if value.is_undefined() => vec![v8::null(scope).into()],
                Ok(value) => vec![v8::null(scope).into(), value],
                Err(failure) => vec![exception(scope, &failure)],
            };
            Some(Some((function, object.into(), args)))
        }
    }
}

/// Node's UVException for a failure: `CODE: message, syscall 'path' ->
/// 'dest'`, with errno, code, syscall, path and dest in that order.
pub fn exception<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    failure: &Failure,
) -> v8::Local<'s, v8::Value> {
    let (code, description) = super::fs::uv_error(failure.errno);
    let path = failure.path.as_deref().map(String::from_utf8_lossy);
    let dest = failure.dest.as_deref().map(String::from_utf8_lossy);
    let mut text = format!("{code}: {description}, {}", failure.syscall);
    if let Some(path) = &path {
        text.push_str(&format!(" '{path}'"));
    }
    if let Some(dest) = &dest {
        text.push_str(&format!(" -> '{dest}'"));
    }
    let message = string(scope, &text);
    let exception = v8::Exception::error(scope, message);
    let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
    let set = |scope: &mut v8::PinScope<'s, '_>, key: &str, value: v8::Local<'s, v8::Value>| {
        let key = string(scope, key);
        object.set(scope, key.into(), value);
    };
    let value = v8::Integer::new(scope, failure.errno).into();
    set(scope, "errno", value);
    let value = string(scope, code).into();
    set(scope, "code", value);
    let value = string(scope, failure.syscall).into();
    set(scope, "syscall", value);
    if let Some(path) = &path {
        let value = string(scope, path).into();
        set(scope, "path", value);
    }
    if let Some(dest) = &dest {
        let value = string(scope, dest).into();
        set(scope, "dest", value);
    }
    exception
}

/// Copies bytes read into the request's views, in order, as readv fills
/// them; what doesn't fit, which a read never returns, is dropped.
fn fill(scope: &mut v8::PinScope, dest: &Dest, mut bytes: &[u8]) {
    for view in &dest.0 {
        if bytes.is_empty() {
            return;
        }
        let view = v8::Local::new(scope, view);
        let Some(target) = super::buffer::bytes(scope, view.into()) else {
            continue;
        };
        let n = target.len().min(bytes.len());
        target[..n].copy_from_slice(&bytes[..n]);
        bytes = &bytes[n..];
    }
}

/// A finished call's value. Stats go in the binding's shared arrays for a
/// callback, which reads them at once, as FillGlobalStatsArray does, and
/// in arrays of their own for a promise, which may be read after another
/// call has filled the shared ones, as FSReqPromise's are.
pub fn value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    done: Done,
    dest: Option<&Dest>,
    shared: bool,
) -> v8::Local<'s, v8::Value> {
    match done {
        Done::Undefined => v8::undefined(scope).into(),
        Done::Number(n) => v8::Number::new(scope, n).into(),
        Done::Text(text) => string(scope, &String::from_utf8_lossy(&text)).into(),
        Done::Encoded(bytes, encoding) => encoded(scope, &bytes, encoding),
        Done::Names(names, types, encoding) => {
            let names: Vec<_> = names
                .iter()
                .map(|name| encoded(scope, name, encoding))
                .collect();
            let names = v8::Array::new_with_elements(scope, &names);
            match types {
                None => names.into(),
                Some(types) => {
                    let types: Vec<_> = types
                        .iter()
                        .map(|&kind| v8::Integer::new(scope, kind).into())
                        .collect();
                    let types = v8::Array::new_with_elements(scope, &types);
                    v8::Array::new_with_elements(scope, &[names.into(), types.into()]).into()
                }
            }
        }
        Done::Stats(fields, bigint) => {
            super::fs::stats_array(scope, &fields, bigint, false, shared)
        }
        Done::StatFs(fields, bigint) => {
            super::fs::stats_array(scope, &fields, bigint, true, shared)
        }
        Done::Read(bytes) => {
            if let Some(dest) = dest {
                fill(scope, dest, &bytes);
            }
            v8::Number::new(scope, bytes.len() as f64).into()
        }
    }
}

/// `bytes` in `encoding`, a Buffer for "buffer": StringBytes::Encode.
pub fn encoded<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    bytes: &[u8],
    encoding: Encoding,
) -> v8::Local<'s, v8::Value> {
    match encoding {
        Encoding::Buffer => super::fs::new_buffer(scope, bytes),
        encoding => match decode(scope, bytes, encoding) {
            Some(text) => text.into(),
            None => {
                type_error(scope, "the result is too large for a string");
                v8::undefined(scope).into()
            }
        },
    }
}
