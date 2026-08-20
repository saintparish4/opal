//! Content-addressed store: every object is named by the BLAKE3 hash of its own
//! bytes.
//!
//! On-disk layout:
//!
//! ```text
//! <root>/CAS_VERSION            layout version, so a future change is loud
//! <root>/objects/ab/cd/abcd...  object, sharded by the first four hex chars
//! <root>/tmp/obj-<pid>-...tmp   in-flight writes, and orphans left by kills
//! ```
//!
//! Two shard levels of two hex characters give 65,536 buckets, which keeps
//! directory sizes reasonable at millions of objects — a flat or single-level
//! layout degrades on ext4 and HFS+ well before that. The filename is the full
//! hex digest rather than the usual git-style remainder, so an object carries
//! its own key: [`Cas::audit`] needs no side table to check content against key.
//!
//! Writes never touch the final path directly. Bytes go to `tmp/`, get fsynced,
//! get re-hashed from disk, and only then are renamed into `objects/`. A process
//! killed at any point in that sequence leaves an orphaned temp file, never a
//! corrupt object.

pub mod gc;

use std::fs::{self, File};
use std::io::{self, Read, Write as _};
use std::path::{Path, PathBuf};

use crate::atomic::{TempFile, write_atomic};
use crate::fault::{self, FaultPoint};
use crate::hash::{ContentHash, ContentHasher, HASH_HEX_LEN};

/// Bumped when the on-disk layout changes in a way older builds misread.
pub const LAYOUT_VERSION: u32 = 1;

const VERSION_FILE: &str = "CAS_VERSION";
const OBJECTS_DIR: &str = "objects";
const TMP_DIR: &str = "tmp";
const COPY_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CasError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("object {hash} is not in the store")]
    Missing { hash: ContentHash },

    /// The store contains an object whose content does not hash to its key.
    /// Atomic writes are designed to make this unreachable; if it ever fires,
    /// something outside Opal has written into the store.
    #[error("object {hash} contains content hashing to {actual}")]
    Corrupt {
        hash: ContentHash,
        actual: ContentHash,
    },

    /// The bytes read back from the temp file did not hash to what was written.
    #[error("write verification failed: hashed {expected} while writing, {actual} on readback")]
    WriteVerification {
        expected: ContentHash,
        actual: ContentHash,
    },

    #[error("cache at {path} has layout version {found}, this build understands {LAYOUT_VERSION}")]
    LayoutVersion { path: PathBuf, found: String },
}

impl CasError {
    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

type Result<T> = std::result::Result<T, CasError>;

/// A content-addressed store rooted at a directory.
///
/// Cloning is cheap and every method takes `&self`: several tools share one
/// store concurrently, and correctness comes from atomic renames rather than
/// from exclusive access.
#[derive(Clone, Debug)]
pub struct Cas {
    root: PathBuf,
    objects: PathBuf,
    tmp: PathBuf,
}

impl Cas {
    /// Opens (creating if needed) a store at `root`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let store = Self {
            objects: root.join(OBJECTS_DIR),
            tmp: root.join(TMP_DIR),
            root,
        };

        for dir in [&store.root, &store.objects, &store.tmp] {
            fs::create_dir_all(dir).map_err(|source| CasError::io(dir, source))?;
        }
        store.check_layout_version()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn temp_dir(&self) -> &Path {
        &self.tmp
    }

    /// Where an object lives, whether or not it exists.
    pub fn object_path(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.to_hex();
        self.objects.join(&hex[0..2]).join(&hex[2..4]).join(&hex)
    }

    pub fn contains(&self, hash: &ContentHash) -> bool {
        self.object_path(hash).is_file()
    }

    /// Stores bytes, returning their hash.
    pub fn put(&self, bytes: &[u8]) -> Result<ContentHash> {
        self.put_reader(bytes)
    }

    /// Stores a file's contents without loading the whole file into memory.
    pub fn put_file(&self, path: &Path) -> Result<ContentHash> {
        let file = File::open(path).map_err(|source| CasError::io(path, source))?;
        self.put_reader(file)
    }

    /// Stores a stream: temp file, fsync, verify, rename.
    pub fn put_reader(&self, mut reader: impl Read) -> Result<ContentHash> {
        let mut temp =
            TempFile::create(&self.tmp, "obj").map_err(|e| CasError::io(&self.tmp, e))?;
        let mut hasher = ContentHasher::new();
        let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
        let mut wrote_a_chunk = false;

        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|source| CasError::io(temp.path(), source))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            temp.file_mut()
                .write_all(&buffer[..read])
                .map_err(|source| CasError::io(temp.path(), source))?;
            if !wrote_a_chunk {
                wrote_a_chunk = true;
                fault::checkpoint(FaultPoint::CasMidWrite);
            }
        }

        let hash = hasher.finish();
        temp.sync_and_close()
            .map_err(|source| CasError::io(temp.path(), source))?;

        // Read the bytes back off the filesystem before trusting them. This is
        // the step that makes a short write or a bad disk fail loudly here,
        // instead of quietly becoming an object that lies about its own hash.
        let readback = ContentHash::of_file(temp.path())
            .map_err(|source| CasError::io(temp.path(), source))?;
        if readback != hash {
            return Err(CasError::WriteVerification {
                expected: hash,
                actual: readback,
            });
        }

        let target = self.object_path(&hash);
        if target.is_file() {
            // Identical content is already stored; the temp file drops away.
            // This is the deduplication PRD §4.3 wants across packages.
            return Ok(hash);
        }

        set_read_only(temp.path()).map_err(|source| CasError::io(temp.path(), source))?;
        fault::checkpoint(FaultPoint::CasBeforeRename);
        temp.persist(&target)
            .map_err(|source| CasError::io(&target, source))?;
        Ok(hash)
    }

    /// Reads an object.
    ///
    /// Does not re-hash: objects are verified on the way in and immutable
    /// afterwards. Use [`Cas::verify`] or [`Cas::audit`] to check the store.
    pub fn read(&self, hash: &ContentHash) -> Result<Vec<u8>> {
        let path = self.object_path(hash);
        fs::read(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => CasError::Missing { hash: *hash },
            _ => CasError::io(path, source),
        })
    }

    pub fn open_object(&self, hash: &ContentHash) -> Result<File> {
        let path = self.object_path(hash);
        File::open(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => CasError::Missing { hash: *hash },
            _ => CasError::io(path, source),
        })
    }

    pub fn size_of(&self, hash: &ContentHash) -> Result<u64> {
        let path = self.object_path(hash);
        let metadata = fs::metadata(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => CasError::Missing { hash: *hash },
            _ => CasError::io(path, source),
        })?;
        Ok(metadata.len())
    }

    /// Re-hashes one object and checks it against its key.
    pub fn verify(&self, hash: &ContentHash) -> Result<()> {
        let path = self.object_path(hash);
        let actual = ContentHash::of_file(&path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => CasError::Missing { hash: *hash },
            _ => CasError::io(path, source),
        })?;
        if actual == *hash {
            Ok(())
        } else {
            Err(CasError::Corrupt {
                hash: *hash,
                actual,
            })
        }
    }

    /// Every object key in the store, sorted.
    pub fn object_hashes(&self) -> Result<Vec<ContentHash>> {
        Ok(self.scan()?.hashes)
    }

    /// Temp files currently in the store, including orphans from killed runs.
    pub fn temp_files(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in read_dir(&self.tmp)? {
            let entry = entry?;
            if entry.path().is_file() {
                paths.push(entry.path());
            }
        }
        paths.sort();
        Ok(paths)
    }

    /// Re-hashes the whole store.
    ///
    /// This is the Phase 0 exit check: after any number of interrupted writes,
    /// every entry's content must still match its hash key.
    pub fn audit(&self) -> Result<AuditReport> {
        let scan = self.scan()?;
        let mut report = AuditReport {
            objects: scan.hashes.len(),
            temp_files: self.temp_files()?.len(),
            stray_files: scan.strays,
            ..AuditReport::default()
        };

        for hash in scan.hashes {
            match self.size_of(&hash) {
                Ok(size) => report.bytes += size,
                Err(error) => {
                    report.unreadable.push((hash, error.to_string()));
                    continue;
                }
            }
            match self.verify(&hash) {
                Ok(()) => {}
                Err(CasError::Corrupt { hash, .. }) => report.corrupt.push(hash),
                Err(error) => report.unreadable.push((hash, error.to_string())),
            }
        }
        Ok(report)
    }

    fn scan(&self) -> Result<Scan> {
        let mut scan = Scan::default();
        for outer in read_dir(&self.objects)? {
            let outer = outer?;
            if !outer.path().is_dir() {
                scan.strays.push(outer.path());
                continue;
            }
            for inner in read_dir(&outer.path())? {
                let inner = inner?;
                if !inner.path().is_dir() {
                    scan.strays.push(inner.path());
                    continue;
                }
                for object in read_dir(&inner.path())? {
                    let object = object?;
                    let path = object.path();
                    match classify_object(&path, &outer.file_name(), &inner.file_name()) {
                        Some(hash) => scan.hashes.push(hash),
                        None => scan.strays.push(path),
                    }
                }
            }
        }
        scan.hashes.sort();
        scan.strays.sort();
        Ok(scan)
    }

    fn check_layout_version(&self) -> Result<()> {
        let path = self.root.join(VERSION_FILE);
        match fs::read_to_string(&path) {
            Ok(found) => {
                if found.trim().parse::<u32>() == Ok(LAYOUT_VERSION) {
                    Ok(())
                } else {
                    Err(CasError::LayoutVersion {
                        path,
                        found: found.trim().to_string(),
                    })
                }
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                write_atomic(&path, format!("{LAYOUT_VERSION}\n").as_bytes(), None)
                    .map_err(|source| CasError::io(path, source))
            }
            Err(source) => Err(CasError::io(path, source)),
        }
    }
}

#[derive(Default)]
struct Scan {
    hashes: Vec<ContentHash>,
    strays: Vec<PathBuf>,
}

/// What [`Cas::audit`] found.
#[derive(Debug, Default)]
pub struct AuditReport {
    pub objects: usize,
    pub bytes: u64,
    /// Objects whose content does not hash to their key.
    pub corrupt: Vec<ContentHash>,
    pub unreadable: Vec<(ContentHash, String)>,
    /// Files under `objects/` that are not validly named objects.
    pub stray_files: Vec<PathBuf>,
    /// Orphaned in-flight writes. Expected after a kill; swept by `gc`.
    pub temp_files: usize,
}

impl AuditReport {
    /// Whether the store is trustworthy. Orphaned temp files do not count
    /// against it — they are the designed outcome of an interrupted write.
    pub fn is_clean(&self) -> bool {
        self.corrupt.is_empty() && self.unreadable.is_empty() && self.stray_files.is_empty()
    }
}

/// Returns the hash if `path` is a validly named object in the right shard.
fn classify_object(
    path: &Path,
    outer: &std::ffi::OsStr,
    inner: &std::ffi::OsStr,
) -> Option<ContentHash> {
    if !path.is_file() {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    if name.len() != HASH_HEX_LEN {
        return None;
    }
    let hash = ContentHash::parse_hex(name).ok()?;
    // An object filed under the wrong shard would be invisible to lookups, so
    // it counts as a stray rather than a member of the store.
    (outer.to_str() == Some(&name[0..2]) && inner.to_str() == Some(&name[2..4])).then_some(hash)
}

fn read_dir(path: &Path) -> Result<impl Iterator<Item = Result<fs::DirEntry>> + use<>> {
    let entries = fs::read_dir(path).map_err(|source| CasError::io(path, source))?;
    let path = path.to_path_buf();
    Ok(entries.map(move |entry| entry.map_err(|source| CasError::io(&path, source))))
}

fn set_read_only(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // Objects are immutable by construction; making them read-only means an
        // accidental in-place write fails instead of silently invalidating the
        // key. It also protects the hardlinks Phase 1 fans out into
        // node_modules, which share these inodes.
        fs::set_permissions(path, fs::Permissions::from_mode(0o444))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Cas) {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        (dir, cas)
    }

    #[test]
    fn test_put_then_read_round_trips() {
        let (_dir, cas) = store();
        let hash = cas.put(b"export default 1;").unwrap();
        assert_eq!(hash, ContentHash::of(b"export default 1;"));
        assert!(cas.contains(&hash));
        assert_eq!(cas.read(&hash).unwrap(), b"export default 1;");
        cas.verify(&hash).unwrap();
    }

    #[test]
    fn test_identical_content_is_stored_once() {
        let (_dir, cas) = store();
        let first = cas.put(b"same").unwrap();
        let second = cas.put(b"same").unwrap();
        assert_eq!(first, second);
        assert_eq!(cas.object_hashes().unwrap().len(), 1);
        assert!(cas.temp_files().unwrap().is_empty());
    }

    #[test]
    fn test_object_is_sharded_by_key_prefix() {
        let (_dir, cas) = store();
        let hash = cas.put(b"shard me").unwrap();
        let hex = hash.to_hex();
        let path = cas.object_path(&hash);
        assert!(path.ends_with(format!("{}/{}/{hex}", &hex[0..2], &hex[2..4])));
        assert!(path.is_file());
    }

    #[test]
    fn test_missing_object_reports_missing() {
        let (_dir, cas) = store();
        let absent = ContentHash::of(b"never stored");
        assert!(matches!(cas.read(&absent), Err(CasError::Missing { .. })));
        assert!(matches!(cas.verify(&absent), Err(CasError::Missing { .. })));
    }

    #[test]
    fn test_put_file_streams_large_input() {
        let (dir, cas) = store();
        let source = dir.path().join("big.bin");
        let payload = vec![3u8; COPY_BUFFER_BYTES * 3 + 17];
        fs::write(&source, &payload).unwrap();

        let hash = cas.put_file(&source).unwrap();
        assert_eq!(hash, ContentHash::of(&payload));
        assert_eq!(cas.size_of(&hash).unwrap(), payload.len() as u64);
    }

    #[test]
    fn test_empty_input_is_a_valid_object() {
        let (_dir, cas) = store();
        let hash = cas.put(b"").unwrap();
        assert_eq!(cas.read(&hash).unwrap(), Vec::<u8>::new());
        assert!(cas.audit().unwrap().is_clean());
    }

    #[test]
    fn test_audit_flags_tampered_object() {
        let (_dir, cas) = store();
        let hash = cas.put(b"honest content").unwrap();
        let path = cas.object_path(&hash);

        // Simulate outside tampering: objects are read-only, so widen first.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }
        fs::write(&path, b"tampered").unwrap();

        let report = cas.audit().unwrap();
        assert!(!report.is_clean());
        assert_eq!(report.corrupt, vec![hash]);
        assert!(matches!(cas.verify(&hash), Err(CasError::Corrupt { .. })));
    }

    #[test]
    fn test_audit_flags_misfiled_object() {
        let (_dir, cas) = store();
        let hash = cas.put(b"filed wrong").unwrap();
        let hex = hash.to_hex();
        let wrong = cas.root().join(OBJECTS_DIR).join("00").join("00");
        fs::create_dir_all(&wrong).unwrap();
        fs::copy(cas.object_path(&hash), wrong.join(&hex)).unwrap();

        let report = cas.audit().unwrap();
        assert!(!report.is_clean());
        assert_eq!(report.stray_files.len(), 1);
    }

    #[test]
    fn test_layout_version_mismatch_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        Cas::open(dir.path()).unwrap();
        fs::write(dir.path().join(VERSION_FILE), "999\n").unwrap();
        assert!(matches!(
            Cas::open(dir.path()),
            Err(CasError::LayoutVersion { .. })
        ));
    }
}
