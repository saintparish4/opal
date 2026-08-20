//! Memoization: reuse a resolved graph when nothing it depended on changed.
//!
//! A memo record is a *lookup hint plus a proof*. The key (entry path + options)
//! only decides which record to read; whether that record may be used is
//! decided entirely by re-checking the [`ResolveTrace`] it carries — every file
//! it read must still hash the same, and every path it probed and found absent
//! must still be absent. Two projects that collide on a key therefore cannot
//! serve each other stale graphs; they simply miss. Two projects with identical
//! content share a hit, which is the cross-project reuse the CAS exists for.
//!
//! No timestamp is consulted anywhere in this file.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::resolver::{self, Resolution, ResolveTrace, ResolverOptions};
use super::{GraphError, GraphSnapshot, ModuleGraph, SnapshotError};
use crate::atomic::write_atomic;
use crate::cas::{Cas, CasError};
use crate::fault::FaultPoint;
use crate::hash::{ContentHash, HashBuilder};
use crate::path::NormalizedPath;

/// Bumped when the record shape changes; old records then miss rather than
/// being misread.
pub const MEMO_FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum MemoError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Cas(#[from] CasError),
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
}

impl MemoError {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Identifies which record to consult — never, on its own, whether it is valid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemoKey(ContentHash);

impl MemoKey {
    pub fn for_graph(entry: &NormalizedPath, options: &ResolverOptions) -> Self {
        let mut builder = HashBuilder::new("opal.memo.graph.v1");
        builder.push_u64(u64::from(MEMO_FORMAT_VERSION));
        builder.push_str(entry.as_str());
        builder.push_hash(&options.digest());
        Self(builder.finish())
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }
}

#[derive(Serialize, Deserialize)]
struct MemoRecord {
    version: u32,
    entry: NormalizedPath,
    trace: ResolveTrace,
    output: ContentHash,
}

/// Why a lookup did not produce a usable graph. Carried into CLI output and
/// test assertions, because "it missed" without a reason is unfalsifiable.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MissReason {
    NoRecord,
    FormatVersion(u32),
    RecordUnreadable(String),
    InputChanged(NormalizedPath),
    InputRemoved(NormalizedPath),
    /// A path the previous run probed and found absent now exists — resolution
    /// could pick differently, so the old graph cannot be trusted.
    MissingPathAppeared(NormalizedPath),
    /// The record survived but its graph was garbage-collected out of the CAS.
    OutputEvicted(ContentHash),
}

impl std::fmt::Display for MissReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRecord => write!(f, "no record"),
            Self::FormatVersion(found) => write!(f, "record format v{found}"),
            Self::RecordUnreadable(why) => write!(f, "record unreadable: {why}"),
            Self::InputChanged(path) => write!(f, "changed: {path}"),
            Self::InputRemoved(path) => write!(f, "removed: {path}"),
            Self::MissingPathAppeared(path) => write!(f, "appeared: {path}"),
            Self::OutputEvicted(hash) => write!(f, "graph {hash} evicted from the store"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CacheStatus {
    Hit,
    Miss(MissReason),
}

impl CacheStatus {
    pub fn is_hit(&self) -> bool {
        matches!(self, Self::Hit)
    }
}

impl std::fmt::Display for CacheStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hit => write!(f, "HIT"),
            Self::Miss(reason) => write!(f, "MISS ({reason})"),
        }
    }
}

/// Graph snapshots in the CAS, plus the records that point at them.
///
/// One store is shared by every tool: the test runner and the bundler asking
/// for the same graph must reach the same record, or the shared-cache bet in
/// PRD §4.2 does not pay off.
#[derive(Clone, Debug)]
pub struct GraphCache {
    cas: Cas,
    records: PathBuf,
}

impl GraphCache {
    pub fn new(cas: Cas, records: impl Into<PathBuf>) -> Result<Self, MemoError> {
        let records = records.into();
        fs::create_dir_all(&records).map_err(|source| MemoError::io(&records, source))?;
        Ok(Self { cas, records })
    }

    pub fn cas(&self) -> &Cas {
        &self.cas
    }

    pub fn record_path(&self, key: MemoKey) -> PathBuf {
        let hex = key.to_hex();
        self.records.join(&hex[0..2]).join(format!("{hex}.json"))
    }

    /// Reads a record and re-checks its trace against the current tree.
    ///
    /// A hit carries the CAS hash of the graph alongside it, so callers can
    /// report or pin the object without re-serializing what they just loaded.
    #[allow(clippy::type_complexity)]
    pub fn lookup(
        &self,
        key: MemoKey,
        root: &NormalizedPath,
    ) -> Result<(Option<(ModuleGraph, ContentHash)>, CacheStatus), MemoError> {
        let path = self.record_path(key);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok((None, CacheStatus::Miss(MissReason::NoRecord)));
            }
            Err(source) => return Err(MemoError::io(path, source)),
        };

        let record: MemoRecord = match serde_json::from_str(&text) {
            Ok(record) => record,
            Err(error) => {
                return Ok((
                    None,
                    CacheStatus::Miss(MissReason::RecordUnreadable(error.to_string())),
                ));
            }
        };
        if record.version != MEMO_FORMAT_VERSION {
            return Ok((
                None,
                CacheStatus::Miss(MissReason::FormatVersion(record.version)),
            ));
        }
        if let Some(reason) = validate(&record.trace, root) {
            return Ok((None, CacheStatus::Miss(reason)));
        }

        let bytes = match self.cas.read(&record.output) {
            Ok(bytes) => bytes,
            Err(CasError::Missing { hash }) => {
                return Ok((None, CacheStatus::Miss(MissReason::OutputEvicted(hash))));
            }
            Err(error) => return Err(error.into()),
        };
        let snapshot: GraphSnapshot = match serde_json::from_slice(&bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Ok((
                    None,
                    CacheStatus::Miss(MissReason::RecordUnreadable(error.to_string())),
                ));
            }
        };

        Ok((
            Some((ModuleGraph::from_snapshot(root, snapshot)?, record.output)),
            CacheStatus::Hit,
        ))
    }

    /// Stores a graph and the trace that justifies reusing it.
    pub fn store(
        &self,
        key: MemoKey,
        entry: &NormalizedPath,
        resolution: &Resolution,
    ) -> Result<ContentHash, MemoError> {
        let snapshot = serde_json::to_vec(&resolution.graph.to_snapshot())
            .expect("graph snapshots contain only serializable values");
        let output = self.cas.put(&snapshot)?;

        let record = MemoRecord {
            version: MEMO_FORMAT_VERSION,
            entry: entry.clone(),
            trace: resolution.trace.clone(),
            output,
        };
        let encoded =
            serde_json::to_vec(&record).expect("memo records contain only serializable values");
        let path = self.record_path(key);
        write_atomic(&path, &encoded, Some(FaultPoint::MemoBeforeRename))
            .map_err(|source| MemoError::io(path, source))?;
        Ok(output)
    }

    /// Graph objects still referenced by a record — the mark set for GC.
    pub fn live_outputs(&self) -> Result<BTreeSet<ContentHash>, MemoError> {
        let mut live = BTreeSet::new();
        for shard in read_dir(&self.records)? {
            if !shard.is_dir() {
                continue;
            }
            for record in read_dir(&shard)? {
                let Ok(text) = fs::read_to_string(&record) else {
                    continue;
                };
                // An unreadable record is not a reason to abort a GC; it just
                // marks nothing, and is itself garbage.
                if let Ok(parsed) = serde_json::from_str::<MemoRecord>(&text) {
                    live.insert(parsed.output);
                }
            }
        }
        Ok(live)
    }
}

/// Re-checks a trace. `None` means every recorded fact still holds.
fn validate(trace: &ResolveTrace, root: &NormalizedPath) -> Option<MissReason> {
    for (relative, expected) in &trace.files {
        let absolute = root.join(relative.as_str());
        match ContentHash::of_file(absolute.as_path()) {
            Ok(actual) if actual == *expected => {}
            Ok(_) => return Some(MissReason::InputChanged(relative.clone())),
            Err(_) => return Some(MissReason::InputRemoved(relative.clone())),
        }
    }
    for relative in &trace.missing {
        // Mirrors the resolver's only question — "is there a file here?" — so a
        // directory appearing where a file was probed is correctly not a miss.
        if root.join(relative.as_str()).as_path().is_file() {
            return Some(MissReason::MissingPathAppeared(relative.clone()));
        }
    }
    None
}

fn read_dir(path: &Path) -> Result<Vec<PathBuf>, MemoError> {
    let mut paths = Vec::new();
    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries {
                paths.push(entry.map_err(|source| MemoError::io(path, source))?.path());
            }
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(MemoError::io(path, source)),
    }
    paths.sort();
    Ok(paths)
}

/// The one call every tool makes: a resolved graph, plus whether it came from
/// cache.
pub struct CachedResolution {
    pub graph: ModuleGraph,
    pub status: CacheStatus,
    pub key: MemoKey,
    pub output: ContentHash,
}

pub fn resolve_cached(
    cache: &GraphCache,
    root: &NormalizedPath,
    entry: &NormalizedPath,
    options: &ResolverOptions,
) -> Result<CachedResolution, GraphError> {
    let key = MemoKey::for_graph(entry, options);
    let (cached, status) = cache.lookup(key, root).map_err(GraphError::Memo)?;

    if let Some((graph, output)) = cached {
        return Ok(CachedResolution {
            graph,
            status,
            key,
            output,
        });
    }

    let resolution = resolver::resolve(root, entry, options)?;
    let output = cache
        .store(key, entry, &resolution)
        .map_err(GraphError::Memo)?;
    Ok(CachedResolution {
        graph: resolution.graph,
        status,
        key,
        output,
    })
}
