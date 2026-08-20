//! The two advisory locks Opal takes.
//!
//! **Per project** (`node_modules/.opal-lock`, exclusive): two `opal install`
//! runs against one project serialize rather than interleave writes.
//!
//! **Per cache** (`<cache>/.opal-cache-lock`, shared by installs, exclusive by
//! collection): an install holds it shared for its whole run, so any number of
//! installs proceed together — they only ever add content-addressed objects,
//! which cannot conflict — while `opal cache gc` takes it exclusively and
//! therefore runs only when nothing is writing. Without it, a collection can
//! mark, then sweep an object an install created in between.
//!
//! Lock ordering is always project first, then cache, and collection takes only
//! the cache lock. Nothing can therefore wait on a lock while holding one the
//! other side needs.
//!
//! `flock` is the right primitive for both: it is advisory (so a stale file is
//! never a problem), it has a shared mode, and the kernel releases it when the
//! process dies — so a SIGKILLed install leaves nothing to clean up and no
//! stale-lock detection to write.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// Lives inside `node_modules`, so it travels with the tree it guards.
pub const INSTALL_LOCK_NAME: &str = ".opal-lock";
/// Lives at the cache root, guarding every object in it.
pub const CACHE_LOCK_NAME: &str = ".opal-cache-lock";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Shared,
    Exclusive,
}

/// A held `flock`. Released when dropped, or when the process dies.
pub struct FileLock {
    _file: File,
    path: PathBuf,
    mode: Mode,
}

impl FileLock {
    /// Blocks until the lock is available.
    pub fn acquire(path: &Path, mode: Mode) -> io::Result<Self> {
        Self::open(path, mode, true)
            .map(|lock| lock.expect("a blocking acquire either succeeds or fails"))
    }

    /// Returns `None` rather than waiting when the lock is held against us.
    pub fn try_acquire(path: &Path, mode: Mode) -> io::Result<Option<Self>> {
        Self::open(path, mode, false)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    fn open(path: &Path, mode: Mode, blocking: bool) -> io::Result<Option<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Opened for writing even in shared mode: the file is a handle, never a
        // channel, and nothing ever reads its contents.
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;

        match flock(&file, mode, blocking)? {
            true => Ok(Some(Self {
                _file: file,
                path: path.to_path_buf(),
                mode,
            })),
            false => Ok(None),
        }
    }
}

/// Serializes installs against one project.
pub struct InstallLock(FileLock);

impl InstallLock {
    pub fn acquire(node_modules: &Path) -> io::Result<Self> {
        FileLock::acquire(&node_modules.join(INSTALL_LOCK_NAME), Mode::Exclusive).map(Self)
    }

    pub fn try_acquire(node_modules: &Path) -> io::Result<Option<Self>> {
        Ok(
            FileLock::try_acquire(&node_modules.join(INSTALL_LOCK_NAME), Mode::Exclusive)?
                .map(Self),
        )
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }
}

/// Keeps collection and writing apart.
pub struct CacheLock(FileLock);

impl CacheLock {
    /// Taken by an install, for its whole run. Concurrent installs share it.
    pub fn shared(cache_root: &Path) -> io::Result<Self> {
        FileLock::acquire(&cache_root.join(CACHE_LOCK_NAME), Mode::Shared).map(Self)
    }

    /// Taken by collection, across mark *and* sweep. A mark set is only valid
    /// as long as nothing is writing.
    pub fn exclusive(cache_root: &Path) -> io::Result<Self> {
        FileLock::acquire(&cache_root.join(CACHE_LOCK_NAME), Mode::Exclusive).map(Self)
    }

    pub fn try_exclusive(cache_root: &Path) -> io::Result<Option<Self>> {
        Ok(FileLock::try_acquire(&cache_root.join(CACHE_LOCK_NAME), Mode::Exclusive)?.map(Self))
    }

    pub fn try_shared(cache_root: &Path) -> io::Result<Option<Self>> {
        Ok(FileLock::try_acquire(&cache_root.join(CACHE_LOCK_NAME), Mode::Shared)?.map(Self))
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }

    pub fn mode(&self) -> Mode {
        self.0.mode()
    }
}

#[cfg(unix)]
fn flock(file: &File, mode: Mode, blocking: bool) -> io::Result<bool> {
    use std::os::unix::io::AsRawFd;

    let operation = match mode {
        Mode::Shared => libc::LOCK_SH,
        Mode::Exclusive => libc::LOCK_EX,
    } | if blocking { 0 } else { libc::LOCK_NB };

    // SAFETY: `file` owns a valid descriptor for the duration of the call, and
    // `flock` only associates a lock with it. The lock is released when the
    // descriptor closes, which is what makes a killed process safe.
    let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK => Ok(false),
        _ => Err(error),
    }
}

#[cfg(not(unix))]
fn flock(file: &File, _mode: Mode, _blocking: bool) -> io::Result<bool> {
    // Native Windows is v2. `LockFileEx` is the equivalent, and it has both
    // modes, so this abstraction survives the port.
    let _ = file;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_installs_against_one_project_serialize() {
        let directory = tempfile::tempdir().unwrap();
        let node_modules = directory.path().join("node_modules");

        let held = InstallLock::acquire(&node_modules).unwrap();
        assert!(held.path().is_file());

        // flock is per open file description, so a second open from this
        // process contends exactly as another process would.
        assert!(InstallLock::try_acquire(&node_modules).unwrap().is_none());

        drop(held);
        assert!(InstallLock::try_acquire(&node_modules).unwrap().is_some());
    }

    #[test]
    fn test_installs_share_the_cache_lock_with_each_other() {
        let directory = tempfile::tempdir().unwrap();
        let first = CacheLock::shared(directory.path()).unwrap();
        let second = CacheLock::try_shared(directory.path()).unwrap();
        assert!(second.is_some(), "two installs must be able to run at once");
        assert_eq!(first.mode(), Mode::Shared);
        assert!(first.path().is_file());
        drop((first, second));
    }

    #[test]
    fn test_collection_excludes_installs_and_the_reverse() {
        let directory = tempfile::tempdir().unwrap();

        let installing = CacheLock::shared(directory.path()).unwrap();
        assert!(
            CacheLock::try_exclusive(directory.path())
                .unwrap()
                .is_none(),
            "collection must not run while an install holds the cache"
        );
        drop(installing);

        let collecting = CacheLock::exclusive(directory.path()).unwrap();
        assert!(
            CacheLock::try_shared(directory.path()).unwrap().is_none(),
            "an install must not start while collection holds the cache"
        );
        drop(collecting);

        assert!(
            CacheLock::try_exclusive(directory.path())
                .unwrap()
                .is_some()
        );
    }
}
