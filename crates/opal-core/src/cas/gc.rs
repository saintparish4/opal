//! Mark-and-sweep garbage collection for the store.
//!
//! Marking is the caller's job: it passes the set of hashes something still
//! points at (the graph snapshots the memo index still references;
//! the objects named by every project's `opal.lock`). Sweeping is
//! everything else, plus temp files old enough that no live process could still
//! be writing them.
//!
//! GC has no exclusive lock, so run it when no other Opal process is
//! writing to the same cache. The temp-file age threshold is the guard that
//! keeps a concurrent write from having its in-flight temp file swept.

use std::collections::BTreeSet;
use std::fs;
use std::time::{Duration, SystemTime};

use super::{Cas, CasError};
use crate::hash::ContentHash;

/// How long an orphaned temp file must sit untouched before it is swept.
pub const DEFAULT_TEMP_MAX_AGE: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone)]
pub struct GcOptions {
    /// Report what would be removed without removing it.
    pub dry_run: bool,
    /// Temp files younger than this are left alone — another process may still
    /// be writing them.
    ///
    /// This reads the filesystem timestamp, which is *not* a cache-invalidation
    /// decision: it never determines whether content is fresh, only whether an
    /// in-flight write might still be in progress. Content hashes remain the
    /// only input to invalidation (PRD §4.2).
    pub temp_max_age: Duration,
    /// Re-hash surviving objects and remove any that fail. Off by default; this
    /// is a full read of the store.
    pub verify: bool,
}

impl Default for GcOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            temp_max_age: DEFAULT_TEMP_MAX_AGE,
            verify: false,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct GcReport {
    pub objects_scanned: usize,
    pub objects_removed: usize,
    pub bytes_reclaimed: u64,
    pub corrupt_removed: usize,
    pub temp_files_removed: usize,
    pub temp_files_kept: usize,
}

/// Removes objects outside `live`, then sweeps stale temp files.
pub fn collect(
    cas: &Cas,
    live: &BTreeSet<ContentHash>,
    options: &GcOptions,
) -> Result<GcReport, CasError> {
    let mut report = GcReport::default();

    for hash in cas.object_hashes()? {
        report.objects_scanned += 1;
        let corrupt = options.verify && matches!(cas.verify(&hash), Err(CasError::Corrupt { .. }));
        if live.contains(&hash) && !corrupt {
            continue;
        }

        let size = cas.size_of(&hash).unwrap_or(0);
        if !options.dry_run {
            remove(cas, &hash)?;
        }
        report.objects_removed += 1;
        report.bytes_reclaimed += size;
        if corrupt {
            report.corrupt_removed += 1;
        }
    }

    let now = SystemTime::now();
    for path in cas.temp_files()? {
        let age = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .unwrap_or_default();
        if age < options.temp_max_age {
            report.temp_files_kept += 1;
            continue;
        }
        if !options.dry_run {
            fs::remove_file(&path).map_err(|source| CasError::io(path, source))?;
        }
        report.temp_files_removed += 1;
    }

    Ok(report)
}

fn remove(cas: &Cas, hash: &ContentHash) -> Result<(), CasError> {
    let path = cas.object_path(hash);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        // Another process may have collected the same object first; the end
        // state is the one we wanted either way.
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(CasError::io(path, source)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atomic::TempFile;

    fn store() -> (tempfile::TempDir, Cas) {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        (dir, cas)
    }

    #[test]
    fn test_sweeps_unreferenced_objects_only() {
        let (_dir, cas) = store();
        let keep = cas.put(b"still referenced").unwrap();
        let drop = cas.put(b"nothing points here").unwrap();

        let live = BTreeSet::from([keep]);
        let report = collect(&cas, &live, &GcOptions::default()).unwrap();

        assert_eq!(report.objects_scanned, 2);
        assert_eq!(report.objects_removed, 1);
        assert!(cas.contains(&keep));
        assert!(!cas.contains(&drop));
    }

    #[test]
    fn test_dry_run_removes_nothing() {
        let (_dir, cas) = store();
        let hash = cas.put(b"unreferenced").unwrap();

        let options = GcOptions {
            dry_run: true,
            ..GcOptions::default()
        };
        let report = collect(&cas, &BTreeSet::new(), &options).unwrap();

        assert_eq!(report.objects_removed, 1);
        assert!(cas.contains(&hash));
    }

    #[test]
    fn test_keeps_recent_temp_files_and_sweeps_old_ones() {
        let (_dir, cas) = store();
        let orphan = {
            let temp = TempFile::create(cas.temp_dir(), "obj").unwrap();
            let path = temp.path().to_path_buf();
            std::mem::forget(temp); // simulate a killed process: no Drop cleanup
            path
        };
        assert!(orphan.is_file());

        let keep_all = collect(&cas, &BTreeSet::new(), &GcOptions::default()).unwrap();
        assert_eq!(keep_all.temp_files_kept, 1);
        assert!(orphan.is_file());

        let sweep = GcOptions {
            temp_max_age: Duration::ZERO,
            ..GcOptions::default()
        };
        let swept = collect(&cas, &BTreeSet::new(), &sweep).unwrap();
        assert_eq!(swept.temp_files_removed, 1);
        assert!(!orphan.exists());
    }

    #[test]
    fn test_verify_removes_corrupt_objects_even_when_live() {
        let (_dir, cas) = store();
        let hash = cas.put(b"content").unwrap();
        let path = cas.object_path(&hash);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }
        fs::write(&path, b"corrupted from outside").unwrap();

        let options = GcOptions {
            verify: true,
            ..GcOptions::default()
        };
        let report = collect(&cas, &BTreeSet::from([hash]), &options).unwrap();

        assert_eq!(report.corrupt_removed, 1);
        assert!(!cas.contains(&hash));
        assert!(cas.audit().unwrap().is_clean());
    }
}
