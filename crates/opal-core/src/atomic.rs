//! Atomic file writes: write a temp file, fsync it, rename it into place.
//!
//! Nothing in Opal writes a file that another run might read by writing it in
//! place. `rename(2)` within a filesystem is atomic, so a reader sees either the
//! old file or the new one, and a process killed mid-write leaves a temp file
//! behind rather than a torn one. This is the primitive behind atomic CAS
//! writes, the memo records, and `opal.lock`.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fault::{self, FaultPoint};

/// A temp file that deletes itself unless it is persisted.
///
/// Cleanup on `Drop` covers the error paths. It deliberately does *not* cover a
/// SIGKILL — that is what leaves the orphan temp files `opal cache gc` sweeps,
/// and leaving them is the whole point: an orphan is inert garbage, where a
/// partially written object at its final path would be corruption.
pub struct TempFile {
    path: PathBuf,
    file: Option<File>,
    persisted: bool,
}

impl TempFile {
    pub fn create(dir: &Path, prefix: &str) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(unique_name(prefix));
        let file = File::options().create_new(true).write(true).open(&path)?;
        Ok(Self {
            path,
            file: Some(file),
            persisted: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("temp file handle is taken only by sync_and_close")
    }

    /// Flushes to the filesystem and closes the handle.
    pub fn sync_and_close(&mut self) -> io::Result<()> {
        match self.file.take() {
            Some(file) => file.sync_all(),
            None => Ok(()),
        }
    }

    /// Renames into place. The caller must have called [`Self::sync_and_close`].
    ///
    /// `target` must be on the same filesystem as the temp directory, or the
    /// rename fails with `EXDEV` — loudly, which is the right outcome.
    pub fn persist(mut self, target: &Path) -> io::Result<()> {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&self.path, target)?;
        self.persisted = true;
        // The rename is already atomic against a killed process; the directory
        // fsync is what makes it survive a power cut as well.
        match target.parent() {
            Some(parent) => sync_dir(parent),
            None => Ok(()),
        }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.persisted {
            return;
        }
        self.file = None;
        let _ = fs::remove_file(&self.path);
    }
}

/// Writes `bytes` to `path` atomically, through a temp file in the same
/// directory (so the rename cannot cross a filesystem boundary).
pub fn write_atomic(
    path: &Path,
    bytes: &[u8],
    before_rename: Option<FaultPoint>,
) -> io::Result<()> {
    use std::io::Write as _;

    let dir = path.parent().unwrap_or(Path::new("."));
    let mut temp = TempFile::create(dir, "write")?;
    temp.file_mut().write_all(bytes)?;
    temp.sync_and_close()?;
    if let Some(point) = before_rename {
        fault::checkpoint(point);
    }
    temp.persist(path)
}

/// fsyncs a directory so a rename into it is durable.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        // Directory handles are not openable this way on Windows (v2). The
        // rename itself is still atomic; only power-loss durability differs.
        let _ = dir;
        Ok(())
    }
}

fn unique_name(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0);
    // pid + per-process counter + nanos: unique across concurrent `opal`
    // processes sharing one cache, without a random-number dependency.
    format!("{prefix}-{}-{counter}-{nanos}.tmp", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_atomic_replaces_content() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("f.txt");
        write_atomic(&target, b"one", None).unwrap();
        write_atomic(&target, b"two", None).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"two");

        let strays: Vec<PathBuf> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path != &target)
            .collect();
        assert!(strays.is_empty(), "unexpected leftovers: {strays:?}");
    }

    #[test]
    fn test_dropped_temp_file_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = {
            let temp = TempFile::create(dir.path(), "t").unwrap();
            temp.path().to_path_buf()
        };
        assert!(!path.exists());
    }

    #[test]
    fn test_unique_names_do_not_repeat() {
        let names: Vec<String> = (0..100).map(|_| unique_name("t")).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
    }
}
