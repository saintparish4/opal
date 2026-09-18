//! Packument metadata, cached across runs.
//!
//! `NpmRegistry`'s in-process cache dedupes repeat lookups inside one resolve
//! and then dies with the process, so a warm package store carried no memory of
//! the metadata fetched to build it. Re-resolving a 440-package tree with every
//! tarball already present still spent **596.4s** re-fetching packuments, and
//! the benchmark puts re-resolution at 87–97% round-trip stall against a fully
//! warm store. This is the other half of that store.
//!
//! **This is HTTP freshness, not cache invalidation.** The distinction matters
//! because mtime is banned from invalidation logic outright, and a record
//! here does carry a timestamp. Nothing content-addressed is keyed on it: a
//! record holds the time *inside its own body*, nothing reads a file's mtime,
//! and packuments live outside the CAS precisely so no content-addressed
//! invariant is weakened by a document the registry is free to change under us.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opal_core::atomic::write_atomic;
use opal_core::hash::ContentHash;

/// A registry that sends no `Cache-Control` gets npm's own answer for
/// packuments, which is what the public registry sends today.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(300);

const RECORD_VERSION: u32 = 1;
const EMPTY: &str = "-";

/// One cached packument response.
#[derive(Clone, Debug)]
pub struct Record {
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub fetched: SystemTime,
    pub max_age: Duration,
}

impl Record {
    pub fn is_fresh(&self, now: SystemTime) -> bool {
        now.duration_since(self.fetched)
            .is_ok_and(|age| age < self.max_age)
    }

    fn render(&self) -> Vec<u8> {
        let fetched = self
            .fetched
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut out = format!(
            "opal-packument {RECORD_VERSION}\netag {}\nfetched {fetched}\nmax-age {}\n\n",
            self.etag.as_deref().unwrap_or(EMPTY),
            self.max_age.as_secs()
        )
        .into_bytes();
        out.extend_from_slice(&self.body);
        out
    }

    /// Malformed records read as absent rather than fatal: this is a cache, and
    /// the answer to an unreadable one is to fetch again.
    fn parse(bytes: &[u8]) -> Option<Self> {
        let split = bytes.windows(2).position(|pair| pair == b"\n\n")?;
        let (header, body) = bytes.split_at(split);
        let header = std::str::from_utf8(header).ok()?;
        let mut lines = header.lines();

        let version = lines.next()?.strip_prefix("opal-packument ")?;
        if version.trim() != RECORD_VERSION.to_string() {
            return None;
        }

        let mut etag = None;
        let mut fetched = None;
        let mut max_age = None;
        for line in lines {
            let (key, value) = line.split_once(' ')?;
            match key {
                "etag" => etag = (value != EMPTY).then(|| value.to_string()),
                "fetched" => fetched = value.parse::<u64>().ok(),
                "max-age" => max_age = value.parse::<u64>().ok(),
                _ => {}
            }
        }

        Some(Self {
            body: body.get(2..)?.to_vec(),
            etag,
            fetched: UNIX_EPOCH + Duration::from_secs(fetched?),
            max_age: Duration::from_secs(max_age?),
        })
    }
}

/// Where cached packuments live, one file per (registry, package).
#[derive(Clone, Debug)]
pub struct PackumentCache {
    root: PathBuf,
}

impl PackumentCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Keyed on the registry base as well as the name, so pointing `opal` at a
    /// different registry cannot serve it another one's answer.
    fn record_path(&self, base: &str, name: &str) -> PathBuf {
        let hash = ContentHash::of(format!("{base}\n{name}").as_bytes());
        let hex = hash.to_hex();
        self.root.join(&hex[0..2]).join(format!("{hex}.packument"))
    }

    pub fn get(&self, base: &str, name: &str) -> Option<Record> {
        Record::parse(&std::fs::read(self.record_path(base, name)).ok()?)
    }

    /// A cache write that fails is not an install that fails — the body is
    /// already in hand, and the only cost of losing it is fetching it again.
    pub fn put(&self, base: &str, name: &str, record: &Record) {
        let path = self.record_path(base, name);
        let _ = write_atomic(&path, &record.render(), None);
    }

    /// Drops records older than `keep`, and anything unreadable.
    ///
    /// This directory is bounded only by the number of packages ever resolved
    /// on the machine, so without a sweep it is the same monotonic leak memo
    /// records are. Nothing is lost by dropping one: the worst case is a fetch
    /// that would have been a cache hit.
    ///
    /// The age comes from the record's own `fetched` field, not from the
    /// file's mtime. That is the same distinction the record's own
    /// documentation draws, and it matters here too: a record copied or
    /// restored from a backup would carry a fresh mtime and a truthful
    /// `fetched`.
    pub fn prune(&self, keep: Duration, now: SystemTime, dry_run: bool) -> usize {
        let mut pruned = 0;
        let Ok(shards) = std::fs::read_dir(&self.root) else {
            return 0;
        };
        for shard in shards.flatten() {
            let Ok(records) = std::fs::read_dir(shard.path()) else {
                continue;
            };
            for record in records.flatten() {
                let path = record.path();
                let expired = match std::fs::read(&path).ok().as_deref().and_then(Record::parse) {
                    Some(record) => now
                        .duration_since(record.fetched)
                        .is_ok_and(|age| age > keep),
                    // Unreadable is garbage by definition: nothing will ever
                    // serve from it.
                    None => true,
                };
                if !expired {
                    continue;
                }
                if !dry_run && std::fs::remove_file(&path).is_err() {
                    continue;
                }
                pruned += 1;
            }
        }
        pruned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(body: &str) -> Record {
        Record {
            body: body.as_bytes().to_vec(),
            etag: Some("W/\"abc\"".to_string()),
            fetched: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            max_age: DEFAULT_MAX_AGE,
        }
    }

    #[test]
    fn test_a_record_round_trips_through_the_cache() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PackumentCache::new(directory.path());
        assert!(cache.get("https://registry.example", "left-pad").is_none());

        let stored = record("{\"name\":\"left-pad\"}");
        cache.put("https://registry.example", "left-pad", &stored);

        let read = cache.get("https://registry.example", "left-pad").unwrap();
        assert_eq!(read.body, stored.body);
        assert_eq!(read.etag, stored.etag);
        assert_eq!(read.fetched, stored.fetched);
        assert_eq!(read.max_age, stored.max_age);
    }

    #[test]
    fn test_another_registry_never_answers_for_this_one() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PackumentCache::new(directory.path());
        cache.put("https://one.example", "left-pad", &record("{}"));
        assert!(cache.get("https://two.example", "left-pad").is_none());
    }

    #[test]
    fn test_a_body_containing_a_blank_line_survives() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PackumentCache::new(directory.path());
        let body = "{\n\n  \"name\": \"x\"\n}";
        cache.put("https://registry.example", "x", &record(body));
        let read = cache.get("https://registry.example", "x").unwrap();
        assert_eq!(read.body, body.as_bytes());
    }

    #[test]
    fn test_freshness_is_read_from_the_record_not_the_file() {
        let stored = record("{}");
        let fresh = stored.fetched + Duration::from_secs(299);
        let stale = stored.fetched + Duration::from_secs(301);
        assert!(stored.is_fresh(fresh));
        assert!(!stored.is_fresh(stale));
        // A clock that went backwards is not freshness.
        assert!(!stored.is_fresh(stored.fetched - Duration::from_secs(1)));
    }

    #[test]
    fn test_pruning_drops_old_records_and_keeps_recent_ones() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PackumentCache::new(directory.path());
        let now = UNIX_EPOCH + Duration::from_secs(2_000_000_000);

        let mut old = record("{}");
        old.fetched = now - Duration::from_secs(60 * 60 * 24 * 30);
        cache.put("https://registry.example", "ancient", &old);

        let mut recent = record("{}");
        recent.fetched = now - Duration::from_secs(60);
        cache.put("https://registry.example", "recent", &recent);

        let keep = Duration::from_secs(60 * 60 * 24 * 7);
        assert_eq!(cache.prune(keep, now, true), 1, "dry run counts, keeps");
        assert!(cache.get("https://registry.example", "ancient").is_some());

        assert_eq!(cache.prune(keep, now, false), 1);
        assert!(cache.get("https://registry.example", "ancient").is_none());
        assert!(cache.get("https://registry.example", "recent").is_some());
    }

    #[test]
    fn test_pruning_drops_a_record_it_cannot_read() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PackumentCache::new(directory.path());
        let path = cache.record_path("https://registry.example", "x");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not a record").unwrap();

        assert_eq!(cache.prune(Duration::MAX, SystemTime::now(), false), 1);
        assert!(!path.exists());
    }

    #[test]
    fn test_a_damaged_record_reads_as_absent() {
        let directory = tempfile::tempdir().unwrap();
        let cache = PackumentCache::new(directory.path());
        let path = cache.record_path("https://registry.example", "x");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        for damaged in [
            &b"not a record at all"[..],
            b"opal-packument 99\netag -\nfetched 1\nmax-age 1\n\n{}",
            b"opal-packument 1\netag -\n\n{}",
        ] {
            std::fs::write(&path, damaged).unwrap();
            assert!(cache.get("https://registry.example", "x").is_none());
        }
    }
}
