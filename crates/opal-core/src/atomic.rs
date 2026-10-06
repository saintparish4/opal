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

    /// A temp file at exactly `path`, in place of whatever is there.
    ///
    /// For a file in a directory opal does not own, whose writers a lock
    /// already serializes. A uniquely named temp file left there by a killed
    /// write stays for good, because nothing can tell it from a file the user
    /// made. One with a fixed name is recognisably opal's: the next write
    /// replaces it, and [`remove_stale`] clears it without waiting for one.
    ///
    /// What was at `path` is removed and the file created anew, never opened.
    /// Opening would follow a symlink left under that name and write through
    /// it, and a repository can commit one.
    pub fn create_at(path: PathBuf) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        remove_stale(&path)?;
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
    write_through_temp(path, None, bytes, None, before_rename)
}

/// [`write_atomic`] through the temp file `temp`, a fixed path beside `path`
/// ([`TempFile::create_at`] says when that is the right kind). The caller
/// holds whatever lock keeps two writers of `path` apart.
pub fn write_atomic_via(
    path: &Path,
    temp: &Path,
    bytes: &[u8],
    before_rename: Option<FaultPoint>,
) -> io::Result<()> {
    write_through_temp(path, Some(temp), bytes, None, before_rename)
}

/// What a fixed-name temp file adds to the name of the file it replaces,
/// where that file is not one of opal's own.
const REPLACEMENT_SUFFIX: &str = ".opal-tmp";

/// Where [`replace_atomic`] writes before renaming over `path`: beside the
/// file `path` resolves to, which for a symlink is not beside the link.
pub fn replacement_temp(path: &Path) -> io::Result<PathBuf> {
    let target = fs::canonicalize(path)?;
    let mut name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file path"))?
        .to_os_string();
    name.push(REPLACEMENT_SUFFIX);
    Ok(target.with_file_name(name))
}

/// Removes what a killed write left at a fixed temp path. Nothing being
/// there is the usual case and not an error.
pub fn remove_stale(temp: &Path) -> io::Result<()> {
    match fs::remove_file(temp) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// [`write_atomic`], for a file opal did not create and must not change the
/// nature of.
///
/// A rename swaps in a new inode, so what belonged to the old one is carried
/// across by hand: its permissions, and, where `path` is a symlink, the link
/// itself, by replacing the file it points at.
///
/// A rename also needs only the *directory* to be writable, which would let
/// this replace a file its owner marked read-only. Opening the file for
/// writing first refuses that, as a write in place would have.
///
/// The temp file is [`replacement_temp`], a fixed name, so the caller holds
/// whatever lock keeps two writers of `path` apart.
pub fn replace_atomic(
    path: &Path,
    bytes: &[u8],
    before_rename: Option<FaultPoint>,
) -> io::Result<()> {
    let target = fs::canonicalize(path)?;
    let permissions = File::options()
        .write(true)
        .open(&target)?
        .metadata()?
        .permissions();
    let temp = replacement_temp(&target)?;
    write_through_temp(
        &target,
        Some(&temp),
        bytes,
        Some(permissions),
        before_rename,
    )
}

fn write_through_temp(
    path: &Path,
    temp: Option<&Path>,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
    before_rename: Option<FaultPoint>,
) -> io::Result<()> {
    use std::io::Write as _;

    let mut temp = match temp {
        Some(temp) => TempFile::create_at(temp.to_path_buf())?,
        None => TempFile::create(path.parent().unwrap_or(Path::new(".")), "write")?,
    };
    temp.file_mut().write_all(bytes)?;
    if let Some(permissions) = permissions {
        temp.file_mut().set_permissions(permissions)?;
    }
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

    #[cfg(unix)]
    #[test]
    fn test_a_replaced_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("package.json");
        fs::write(&target, b"one").unwrap();

        for mode in [0o600, 0o664, 0o755] {
            fs::set_permissions(&target, fs::Permissions::from_mode(mode)).unwrap();
            replace_atomic(&target, b"two", None).unwrap();

            assert_eq!(fs::read(&target).unwrap(), b"two");
            let replaced = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
            assert_eq!(replaced, mode, "{mode:o} became {replaced:o}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_replacing_through_a_symlink_keeps_the_link_and_rewrites_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("shared").join("package.json");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, b"one").unwrap();
        let link = dir.path().join("package.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        replace_atomic(&link, b"two", None).unwrap();

        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"two");
        let beside_the_link: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(beside_the_link.len(), 2, "{beside_the_link:?}");
    }

    #[cfg(unix)]
    #[test]
    fn test_a_read_only_file_is_not_replaced() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("package.json");
        fs::write(&target, b"one").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444)).unwrap();
        // Root opens anything for writing, so there is nothing to refuse.
        if File::options().write(true).open(&target).is_ok() {
            return;
        }

        let error = replace_atomic(&target, b"two", None).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(&target).unwrap(), b"one");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn test_a_fixed_temp_file_takes_the_place_of_one_a_killed_write_left() {
        let dir = tempfile::tempdir().unwrap();
        let (target, temp) = (
            dir.path().join("opal.lock"),
            dir.path().join("opal.lock.tmp"),
        );
        fs::write(&target, b"one").unwrap();
        fs::write(&temp, b"half of a write that was killed").unwrap();

        write_atomic_via(&target, &temp, b"two", None).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"two");
        assert!(!temp.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_a_symlink_left_at_the_temp_path_is_replaced_and_not_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("package.json");
        let victim = dir.path().join("victim");
        fs::write(&target, b"one").unwrap();
        fs::write(&victim, b"untouched").unwrap();
        let temp = replacement_temp(&target).unwrap();
        std::os::unix::fs::symlink(&victim, &temp).unwrap();

        replace_atomic(&target, b"two", None).unwrap();

        assert_eq!(fs::read(&victim).unwrap(), b"untouched");
        assert_eq!(fs::read(&target).unwrap(), b"two");
        assert!(!fs::symlink_metadata(&target).unwrap().is_symlink());
        assert!(fs::symlink_metadata(&temp).is_err());
    }

    #[test]
    fn test_the_replacement_temp_sits_beside_the_file_under_a_name_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("package.json");
        fs::write(&target, b"one").unwrap();

        let temp = replacement_temp(&target).unwrap();

        assert_eq!(temp.file_name().unwrap(), "package.json.opal-tmp");
        assert_eq!(
            temp.parent().unwrap(),
            fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[test]
    fn test_removing_a_stale_temp_file_that_is_not_there_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("opal.lock.tmp");

        remove_stale(&temp).unwrap();
        fs::write(&temp, b"left behind").unwrap();
        remove_stale(&temp).unwrap();

        assert!(!temp.exists());
    }

    #[test]
    fn test_replacing_a_file_that_is_not_there_is_an_error() {
        let dir = tempfile::tempdir().unwrap();

        let error = replace_atomic(&dir.path().join("absent"), b"two", None).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
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
