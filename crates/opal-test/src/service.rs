//! Transforms on request, for workers in the middle of a `require`.
//!
//! Each worker opens a second connection for these, and opal answers each one
//! on that connection's own thread, so workers transform in parallel. A
//! result is kept in memory for the run and on disk across runs, under a hash
//! of the source, its extension and the options it was transformed with; a
//! file edited back to an earlier state finds its old result.
//!
//! A result is kept as the reply's bytes and sent as they are: a hit costs a
//! read and a write, not a parse and a print. The source map in it is one
//! string of numbers, `line,column,sourceLine,sourceColumn` repeated, which
//! the worker decodes only for a file a stack trace passes through.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Numbers each staging file this process writes. Workers ask for the same
/// file at once on different threads, so a staging name has to be unique
/// within the process, not just across processes.
static STAGING: AtomicU64 = AtomicU64::new(0);

use serde_json::{Value, json};

use crate::transform;
use crate::tsconfig::TsconfigCache;

pub struct TransformService {
    /// The opal binary's own hash: any change to opal, and so possibly to
    /// what it emits, starts from an empty cache. Worked out on the first
    /// request, so a run that transforms nothing doesn't pay for it.
    identity: OnceLock<blake3::Hash>,
    tsconfig: TsconfigCache,
    memory: Mutex<HashMap<blake3::Hash, Arc<Vec<u8>>>>,
    disk: Option<PathBuf>,
}

impl TransformService {
    pub fn new(cache_dir: &Path) -> Self {
        let disk = cache_dir.join("test-transform");
        TransformService {
            identity: OnceLock::new(),
            tsconfig: TsconfigCache::default(),
            memory: Mutex::new(HashMap::new()),
            disk: std::fs::create_dir_all(&disk).ok().map(|()| disk),
        }
    }

    /// The reply to one request, as JSON bytes: `{code, mappings}` or
    /// `{error}`.
    pub fn answer(&self, request: &Value) -> Arc<Vec<u8>> {
        let error = |message: String| {
            Arc::new(serde_json::to_vec(&json!({ "error": message })).expect("serializable"))
        };
        let Some(path) = request["path"].as_str().map(Path::new) else {
            return error("a transform request names no path".to_string());
        };
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(failure) => return error(format!("{}: {failure}", path.display())),
        };
        let options = self.tsconfig.options_for(path);
        let mut hasher = blake3::Hasher::new();
        let identity = self.identity.get_or_init(|| {
            std::env::current_exe()
                .and_then(std::fs::read)
                .map(|bytes| blake3::hash(&bytes))
                .unwrap_or_else(|_| blake3::hash(env!("CARGO_PKG_VERSION").as_bytes()))
        });
        hasher.update(identity.as_bytes());
        hasher.update(format!("{options:?}").as_bytes());
        hasher.update(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .as_bytes(),
        );
        hasher.update(&[0]);
        hasher.update(source.as_bytes());
        let key = hasher.finalize();
        if let Some(hit) = self.memory.lock().expect("lock").get(&key) {
            return Arc::clone(hit);
        }
        let file = self
            .disk
            .as_ref()
            .map(|dir| dir.join(format!("{}.json", key.to_hex())));
        // A whole reply is one JSON object; anything else (a file another tool
        // left half written) is done again rather than served.
        if let Some(cached) = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .filter(|bytes| bytes.first() == Some(&b'{') && bytes.last() == Some(&b'}'))
        {
            let cached = Arc::new(cached);
            self.memory
                .lock()
                .expect("lock")
                .insert(key, Arc::clone(&cached));
            return cached;
        }
        let reply = match transform::transform(path, &source, &options) {
            Ok(output) => {
                let mut mappings = String::with_capacity(output.mappings.len() * 12);
                for (index, mapping) in output.mappings.iter().enumerate() {
                    for (part, number) in mapping.iter().enumerate() {
                        if index > 0 || part > 0 {
                            mappings.push(',');
                        }
                        mappings.push_str(&number.to_string());
                    }
                }
                Arc::new(
                    serde_json::to_vec(&json!({ "code": output.code, "mappings": mappings }))
                        .expect("serializable"),
                )
            }
            // Errors aren't cached: the file's next edit is likely the fix.
            Err(failure) => return error(failure),
        };
        if let Some(file) = file {
            let staging = file.with_extension(format!(
                "tmp{}-{}",
                std::process::id(),
                STAGING.fetch_add(1, Ordering::Relaxed)
            ));
            if std::fs::write(&staging, reply.as_slice()).is_ok() {
                let _ = std::fs::rename(&staging, &file);
            }
        }
        self.memory
            .lock()
            .expect("lock")
            .insert(key, Arc::clone(&reply));
        reply
    }
}
