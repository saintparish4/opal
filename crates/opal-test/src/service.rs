//! Transforms on request, for workers in the middle of a `require`.
//!
//! Each worker opens a second connection for these, and opal answers each one
//! on that connection's own thread, so workers transform in parallel. A
//! result is kept in memory for the run and on disk across runs, under a hash
//! of the source, its extension and the options it was transformed with; a
//! file edited back to an earlier state finds its old result.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};

use crate::transform;
use crate::tsconfig::TsconfigCache;

pub struct TransformService {
    /// The opal binary's own hash: any change to opal, and so possibly to
    /// what it emits, starts from an empty cache. Worked out on the first
    /// request, so a run that transforms nothing doesn't pay for it.
    identity: OnceLock<blake3::Hash>,
    tsconfig: TsconfigCache,
    memory: Mutex<HashMap<blake3::Hash, Arc<Value>>>,
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

    /// The reply to one request: `{code, mappings}` or `{error}`.
    pub fn answer(&self, request: &Value) -> Value {
        let Some(path) = request["path"].as_str().map(Path::new) else {
            return json!({ "error": "a transform request names no path" });
        };
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => return json!({ "error": format!("{}: {error}", path.display()) }),
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
            return (**hit).clone();
        }
        let file = self
            .disk
            .as_ref()
            .map(|dir| dir.join(format!("{}.json", key.to_hex())));
        if let Some(cached) = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        {
            self.memory
                .lock()
                .expect("lock")
                .insert(key, Arc::new(cached.clone()));
            return cached;
        }
        let reply = match transform::transform(path, &source, &options) {
            Ok(output) => json!({ "code": output.code, "mappings": output.mappings }),
            // Errors aren't cached: the file's next edit is likely the fix.
            Err(error) => return json!({ "error": error }),
        };
        if let Some(file) = file {
            let staging = file.with_extension(format!("tmp{}", std::process::id()));
            if std::fs::write(&staging, serde_json::to_vec(&reply).expect("serializable")).is_ok() {
                let _ = std::fs::rename(&staging, &file);
            }
        }
        self.memory
            .lock()
            .expect("lock")
            .insert(key, Arc::new(reply.clone()));
        reply
    }
}
