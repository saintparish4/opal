//! Package contents in the content-addressed store.
//!
//! A tarball is never unpacked to disk as a unit. Each file inside it becomes
//! its own CAS object, and the package is represented by an *index* — a sorted
//! map of relative path to content hash — which is itself a CAS object. Two
//! packages that ship an identical file share one object on disk, which is the
//! cross-package deduplication PRD §4.3 is after, and it means the linker can
//! materialize a package with hardlinks alone.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use opal_core::atomic::write_atomic;
use opal_core::cas::{Cas, CasError};
use opal_core::fault::{self, FaultPoint};
use opal_core::hash::{ContentHash, HashBuilder};
use opal_core::path::NormalizedPath;
use serde::{Deserialize, Serialize};

use crate::integrity::{Integrity, IntegrityError};
use crate::locks::CacheLock;
use crate::parallel;
use crate::semver::Version;

/// Tarball downloaded, integrity not yet checked.
pub const FAULT_BEFORE_VERIFY: FaultPoint = FaultPoint::new("pm-before-verify");

/// A tar entry at or under this size is read into memory so its hash is
/// known before anything is written. Above it, streaming wins: the peak
/// cost of buffering stops being worth one avoided fsync.
const BUFFERED_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

/// How many buffered bytes wait to be stored before the decoder stops and
/// stores them. This, not the package's size, is what an ingest holds in
/// memory — and sixteen packages are being ingested at once.
const BATCH_BYTES: u64 = BUFFERED_ENTRY_BYTES;

/// Which ceiling a tarball ran into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ceiling {
    EntryBytes,
    UnpackedBytes,
    Entries,
}

impl std::fmt::Display for Ceiling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::EntryBytes => "single-file byte",
            Self::UnpackedBytes => "total unpacked byte",
            Self::Entries => "file count",
        })
    }
}

/// Ceilings on what one tarball may unpack to.
///
/// A gzip stream can describe far more output than it costs to download, so
/// without these an install is a disk-filling primitive that anyone who can
/// publish a package holds. The defaults sit where no real package reaches
/// them — the largest things on npm are native toolchains in the low hundreds
/// of megabytes — and a package that does hit one fails loudly rather than
/// filling the store on the way to failing anyway.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub entry_bytes: u64,
    pub unpacked_bytes: u64,
    pub entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            entry_bytes: 512 * 1024 * 1024,
            unpacked_bytes: 1024 * 1024 * 1024,
            entries: 200_000,
        }
    }
}
/// Some of the tarball's files are in the CAS, the index is not yet written.
pub const FAULT_MID_EXTRACT: FaultPoint = FaultPoint::new("pm-mid-extract");

/// Bumped when the index shape changes.
pub const INDEX_FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Cas(#[from] CasError),
    #[error(transparent)]
    Integrity(#[from] IntegrityError),
    #[error("{name}@{version}: tarball is not readable: {source}")]
    Tarball {
        name: String,
        version: Version,
        #[source]
        source: std::io::Error,
    },
    #[error("{name}@{version}: tarball contains no files")]
    Empty { name: String, version: Version },
    #[error("{name}@{version}: tarball exceeds the {what} limit of {limit}")]
    TooLarge {
        name: String,
        version: Version,
        what: Ceiling,
        limit: u64,
    },
    #[error("package index {hash} is unreadable: {source}")]
    Index {
        hash: ContentHash,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "package index {found} has format v{version}, this build understands v{INDEX_FORMAT_VERSION}"
    )]
    IndexVersion { found: ContentHash, version: u32 },
}

impl PackageError {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PackageFile {
    pub hash: ContentHash,
    /// Only the execute bit survives. Everything else in a tar mode is noise
    /// that would fragment the store for no behavioural difference.
    #[serde(default)]
    pub executable: bool,
    pub size: u64,
}

/// What a package *is*, as far as Opal is concerned.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PackageIndex {
    pub format: u32,
    pub name: String,
    pub version: Version,
    pub files: BTreeMap<NormalizedPath, PackageFile>,
}

impl PackageIndex {
    /// Canonical bytes. `BTreeMap` ordering makes this deterministic, so the
    /// same tarball always yields the same index hash.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("package indexes contain only serializable values")
    }

    pub fn file(&self, path: &str) -> Option<&PackageFile> {
        self.files.get(&NormalizedPath::new(path))
    }
}

/// The CAS, plus the pointers from a published tarball to its index.
#[derive(Clone, Debug)]
pub struct PackageStore {
    cas: Cas,
    root: PathBuf,
    pointers: PathBuf,
    limits: Limits,
    ingest_threads: usize,
}

/// A tar entry read into memory, waiting to be stored.
struct Buffered {
    path: NormalizedPath,
    executable: bool,
    size: u64,
    bytes: Vec<u8>,
}

impl PackageStore {
    pub fn open(cas: Cas, cache_root: impl Into<PathBuf>) -> Result<Self, PackageError> {
        let root = cache_root.into();
        let pointers = root.join("packages");
        std::fs::create_dir_all(&pointers).map_err(|source| PackageError::io(&pointers, source))?;
        Ok(Self {
            cas,
            root,
            pointers,
            limits: Limits::default(),
            ingest_threads: parallel::workers(),
        })
    }

    /// How many threads store one package's files. One makes ingest a plain
    /// loop, which is what a test comparing the two needs.
    #[must_use]
    pub fn with_ingest_threads(mut self, threads: usize) -> Self {
        self.ingest_threads = threads.max(1);
        self
    }

    /// Tightens what a tarball is allowed to unpack to. The defaults are sized
    /// for the real registry; a test wanting to reach one does not have to
    /// build half a gigabyte to get there.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn cas(&self) -> &Cas {
        &self.cas
    }

    pub fn cache_root(&self) -> &Path {
        &self.root
    }

    /// Held by an install for its whole run. Concurrent installs share it; they
    /// only add content-addressed objects, which cannot conflict.
    pub fn lock_shared(&self) -> Result<CacheLock, PackageError> {
        CacheLock::shared(&self.root).map_err(|source| PackageError::io(&self.root, source))
    }

    /// Held by collection across mark *and* sweep — a mark set is only valid
    /// while nothing is writing.
    pub fn lock_exclusive(&self) -> Result<CacheLock, PackageError> {
        CacheLock::exclusive(&self.root).map_err(|source| PackageError::io(&self.root, source))
    }

    fn pointer_path(&self, integrity: &Integrity) -> PathBuf {
        let key = HashBuilder::new("opal.pm.tarball.v1")
            .push_str(&integrity.to_string())
            .finish()
            .to_hex();
        self.pointers.join(&key[0..2]).join(key)
    }

    /// The index hash for an already-ingested tarball, if the store still has
    /// both the pointer and the index it names.
    pub fn lookup(&self, integrity: &Integrity) -> Result<Option<ContentHash>, PackageError> {
        let path = self.pointer_path(integrity);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(PackageError::io(path, source)),
        };
        let Ok(hash) = ContentHash::parse_hex(text.trim()) else {
            return Ok(None);
        };
        // A pointer to a collected index is as good as no pointer: re-ingesting
        // is always correct, so the store heals itself after a GC.
        Ok(self.cas.contains(&hash).then_some(hash))
    }

    /// Every pointer in the store, as (file path, package index hash).
    ///
    /// Used by the collector to drop pointers whose index is gone.
    pub fn pointer_entries(&self) -> Result<Vec<(PathBuf, ContentHash)>, PackageError> {
        let mut entries = Vec::new();
        let shards = match std::fs::read_dir(&self.pointers) {
            Ok(shards) => shards,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
            Err(source) => return Err(PackageError::io(&self.pointers, source)),
        };

        for shard in shards {
            let shard = shard.map_err(|source| PackageError::io(&self.pointers, source))?;
            if !shard.path().is_dir() {
                continue;
            }
            for pointer in std::fs::read_dir(shard.path())
                .map_err(|source| PackageError::io(shard.path(), source))?
            {
                let pointer = pointer.map_err(|source| PackageError::io(shard.path(), source))?;
                let path = pointer.path();
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if let Ok(hash) = ContentHash::parse_hex(text.trim()) {
                    entries.push((path, hash));
                }
            }
        }
        entries.sort();
        Ok(entries)
    }

    pub fn read_index(&self, hash: &ContentHash) -> Result<PackageIndex, PackageError> {
        let bytes = self.cas.read(hash)?;
        let index: PackageIndex =
            serde_json::from_slice(&bytes).map_err(|source| PackageError::Index {
                hash: *hash,
                source,
            })?;
        if index.format != INDEX_FORMAT_VERSION {
            return Err(PackageError::IndexVersion {
                found: *hash,
                version: index.format,
            });
        }
        Ok(index)
    }

    /// Verifies a tarball, files it into the CAS, and records its index.
    ///
    /// Safe to run twice: every write underneath is content-addressed or
    /// atomic, so a killed ingest leaves at worst some already-verified objects
    /// and no pointer — and the next run redoes exactly the missing part.
    pub fn ingest(
        &self,
        name: &str,
        version: &Version,
        integrity: &Integrity,
        tarball: &[u8],
    ) -> Result<ContentHash, PackageError> {
        fault::checkpoint(FAULT_BEFORE_VERIFY);
        integrity.verify(tarball)?;

        let decoder = flate2::read::GzDecoder::new(tarball);
        let mut archive = tar::Archive::new(decoder);
        let entries = archive.entries().map_err(|source| PackageError::Tarball {
            name: name.to_string(),
            version: version.clone(),
            source,
        })?;

        let mut files = BTreeMap::new();
        let mut batch: Vec<Buffered> = Vec::new();
        let mut batch_bytes: u64 = 0;
        let mut entries_seen: usize = 0;
        let mut announced_mid_extract = false;
        let mut unpacked: u64 = 0;
        let too_large = |what: Ceiling, limit: u64| PackageError::TooLarge {
            name: name.to_string(),
            version: version.clone(),
            what,
            limit,
        };

        for entry in entries {
            let mut entry = entry.map_err(|source| PackageError::Tarball {
                name: name.to_string(),
                version: version.clone(),
                source,
            })?;
            let header = entry.header();
            if !header.entry_type().is_file() {
                // Directories are implied by the files inside them, and Opal
                // does not reproduce in-tarball symlinks or devices.
                continue;
            }
            let mode = header.mode().unwrap_or(0o644);
            let size = entry.size();
            let Some(path) = entry.path().ok().and_then(|path| strip_package_root(&path)) else {
                continue;
            };

            // Checked before reading a byte of it: the header's declared size
            // is what the reader will hand over, so refusing here costs
            // nothing and refusing later costs the whole write.
            if size > self.limits.entry_bytes {
                return Err(too_large(Ceiling::EntryBytes, self.limits.entry_bytes));
            }
            unpacked = unpacked.saturating_add(size);
            if unpacked > self.limits.unpacked_bytes {
                return Err(too_large(
                    Ceiling::UnpackedBytes,
                    self.limits.unpacked_bytes,
                ));
            }
            if entries_seen >= self.limits.entries {
                return Err(too_large(Ceiling::Entries, self.limits.entries as u64));
            }
            entries_seen += 1;
            let executable = mode & 0o111 != 0;

            // Buffered rather than streamed, so the CAS can check whether it
            // already holds these bytes before writing any. Duplicate files are
            // ordinary across an npm tree — licences, tiny shims, identical
            // `package.json` shapes — and every one it recognizes here skips an
            // fsync. Anything larger keeps streaming: the win is not worth
            // holding a 10 MB native binary in memory to get it.
            if size <= BUFFERED_ENTRY_BYTES {
                if batch_bytes + size > BATCH_BYTES {
                    self.store_batch(&mut batch, &mut files)?;
                    batch_bytes = 0;
                }
                let mut bytes = Vec::with_capacity(size as usize);
                entry
                    .read_to_end(&mut bytes)
                    .map_err(|source| PackageError::Tarball {
                        name: name.to_string(),
                        version: version.clone(),
                        source,
                    })?;
                batch_bytes += size;
                batch.push(Buffered {
                    path,
                    executable,
                    size,
                    bytes,
                });
            } else {
                // A path may appear twice in a tarball and the later entry
                // wins, so what was read before this one is recorded first.
                self.store_batch(&mut batch, &mut files)?;
                batch_bytes = 0;
                let hash = self.cas.put_reader(&mut entry)?;
                files.insert(
                    path,
                    PackageFile {
                        hash,
                        executable,
                        size,
                    },
                );
            }
            if !files.is_empty() && !announced_mid_extract {
                announced_mid_extract = true;
                fault::checkpoint(FAULT_MID_EXTRACT);
            }
        }
        self.store_batch(&mut batch, &mut files)?;
        if !files.is_empty() && !announced_mid_extract {
            fault::checkpoint(FAULT_MID_EXTRACT);
        }

        if files.is_empty() {
            return Err(PackageError::Empty {
                name: name.to_string(),
                version: version.clone(),
            });
        }

        let index = PackageIndex {
            format: INDEX_FORMAT_VERSION,
            name: name.to_string(),
            version: version.clone(),
            files,
        };
        let hash = self.cas.put(&index.to_bytes())?;

        let pointer = self.pointer_path(integrity);
        write_atomic(&pointer, format!("{hash}\n").as_bytes(), None)
            .map_err(|source| PackageError::io(pointer, source))?;
        Ok(hash)
    }

    /// Stores the buffered entries and records them, leaving `batch` empty.
    ///
    /// A package's files are stored several at a time because each store ends
    /// in an fsync, and a filesystem journal commits the fsyncs it is handed
    /// together: one thread storing `next`'s 8,520 files waits out a commit per
    /// file, where several threads share each commit. Every write underneath
    /// is content-addressed, so two threads storing at once cannot disturb
    /// each other, and the order they finish in reaches nothing: the entries
    /// are recorded in tarball order, into a sorted map.
    fn store_batch(
        &self,
        batch: &mut Vec<Buffered>,
        files: &mut BTreeMap<NormalizedPath, PackageFile>,
    ) -> Result<(), PackageError> {
        if batch.is_empty() {
            return Ok(());
        }
        let positions: Vec<usize> = (0..batch.len()).collect();
        let stored = parallel::each(
            &positions,
            self.ingest_threads.min(batch.len()),
            Vec::new,
            |position, stored: &mut Vec<(usize, ContentHash)>| {
                stored.push((*position, self.cas.put(&batch[*position].bytes)?));
                Ok::<(), PackageError>(())
            },
        )?;

        let mut hashes: Vec<(usize, ContentHash)> = stored.into_iter().flatten().collect();
        hashes.sort_unstable_by_key(|(position, _)| *position);
        for (entry, (_, hash)) in batch.drain(..).zip(hashes) {
            files.insert(
                entry.path,
                PackageFile {
                    hash,
                    executable: entry.executable,
                    size: entry.size,
                },
            );
        }
        Ok(())
    }
}

/// Drops the tarball's single root directory and rejects anything that would
/// escape the package.
///
/// npm tarballs wrap everything in `package/`, but the name is not guaranteed,
/// so the first component is dropped whatever it is — which is what npm does.
/// Absolute paths and `..` are refused: these paths become link targets under
/// `node_modules`, so a crafted tarball must not be able to name a file outside
/// it.
fn strip_package_root(path: &Path) -> Option<NormalizedPath> {
    let mut components = path.components();
    // The dropped root must itself be an ordinary directory name. An absolute
    // path would otherwise have its leading `/` consumed as "the root" and the
    // rest let through.
    if !matches!(components.next(), Some(Component::Normal(_))) {
        return None;
    }

    let mut relative = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if relative.as_os_str().is_empty() {
        return None;
    }
    NormalizedPath::from_native(&relative).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tarball(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            for (path, contents, mode) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(contents.len() as u64);
                header.set_mode(*mode);
                header.set_cksum();
                builder.append_data(&mut header, path, *contents).unwrap();
            }
            builder.finish().unwrap();
        }
        encoder.finish().unwrap()
    }

    fn store() -> (tempfile::TempDir, PackageStore) {
        let directory = tempfile::tempdir().unwrap();
        let cas = Cas::open(directory.path().join("cas")).unwrap();
        let store = PackageStore::open(cas, directory.path()).unwrap();
        (directory, store)
    }

    #[test]
    fn test_ingest_files_every_entry_and_records_modes() {
        let (_directory, store) = store();
        let bytes = tarball(&[
            ("package/index.js", b"module.exports = 1;\n", 0o644),
            ("package/bin/cli.js", b"#!/usr/bin/env node\n", 0o755),
        ]);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);

        let hash = store
            .ingest("demo", &Version::new(1, 0, 0), &integrity, &bytes)
            .unwrap();
        let index = store.read_index(&hash).unwrap();

        assert_eq!(index.name, "demo");
        assert_eq!(index.files.len(), 2);
        assert!(!index.file("index.js").unwrap().executable);
        assert!(index.file("bin/cli.js").unwrap().executable);
        assert_eq!(
            store
                .cas()
                .read(&index.file("index.js").unwrap().hash)
                .unwrap(),
            b"module.exports = 1;\n"
        );
    }

    /// A header claiming far more than it carries. A gzip stream can describe
    /// gigabytes for a few kilobytes on the wire, which is the whole shape of
    /// the attack — so the refusal has to come off the header, before a byte
    /// of it is written anywhere.
    fn lying_tarball(declared: u64) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(declared);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "package/bomb.bin", std::io::empty())
                .unwrap();
            builder.finish().unwrap();
        }
        encoder.finish().unwrap()
    }

    #[test]
    fn test_an_entry_larger_than_the_ceiling_is_refused() {
        let (_directory, store) = store();
        let store = store.with_limits(Limits {
            entry_bytes: 1024,
            ..Limits::default()
        });
        let bytes = lying_tarball(4096);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);

        let error = store
            .ingest("bomb", &Version::new(1, 0, 0), &integrity, &bytes)
            .expect_err("a file bigger than the ceiling is not a package");

        assert!(matches!(error, PackageError::TooLarge { .. }), "{error}");
        // Nothing was written on the way to refusing.
        assert_eq!(store.lookup(&integrity).unwrap(), None);
    }

    #[test]
    fn test_entries_that_only_add_up_to_too_much_are_refused() {
        let (_directory, store) = store();
        // Each file is individually fine; three of them are not.
        let store = store.with_limits(Limits {
            unpacked_bytes: 20,
            ..Limits::default()
        });
        let bytes = tarball(&[
            ("package/a", b"0123456789", 0o644),
            ("package/b", b"0123456789", 0o644),
            ("package/c", b"0123456789", 0o644),
        ]);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);

        let error = store
            .ingest("bomb", &Version::new(1, 0, 0), &integrity, &bytes)
            .expect_err("the total is what fills a disk");
        assert!(matches!(error, PackageError::TooLarge { .. }), "{error}");
    }

    #[test]
    fn test_a_tarball_of_too_many_files_is_refused() {
        let (_directory, store) = store();
        let store = store.with_limits(Limits {
            entries: 2,
            ..Limits::default()
        });
        let bytes = tarball(&[
            ("package/a", b"x", 0o644),
            ("package/b", b"x", 0o644),
            ("package/c", b"x", 0o644),
        ]);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);

        let error = store
            .ingest("bomb", &Version::new(1, 0, 0), &integrity, &bytes)
            .expect_err("one object per entry is the cost being bounded");
        assert!(matches!(error, PackageError::TooLarge { .. }), "{error}");
    }

    #[test]
    fn test_an_ordinary_package_is_nowhere_near_the_ceilings() {
        let (_directory, store) = store();
        let bytes = tarball(&[("package/index.js", b"module.exports = 1;\n", 0o644)]);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);
        assert!(
            store
                .ingest("demo", &Version::new(1, 0, 0), &integrity, &bytes)
                .is_ok()
        );
    }

    #[test]
    fn test_ingest_is_deterministic_and_pointer_backed() {
        let (_directory, store) = store();
        let bytes = tarball(&[("package/index.js", b"same", 0o644)]);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);

        assert_eq!(store.lookup(&integrity).unwrap(), None);
        let first = store
            .ingest("demo", &Version::new(1, 0, 0), &integrity, &bytes)
            .unwrap();
        assert_eq!(store.lookup(&integrity).unwrap(), Some(first));

        let second = store
            .ingest("demo", &Version::new(1, 0, 0), &integrity, &bytes)
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn test_ingest_on_many_threads_matches_one_thread() {
        // Big enough to be stored as more than one batch.
        let blocks: Vec<Vec<u8>> = (0..4u8).map(|fill| vec![fill; 3 * 1024 * 1024]).collect();
        let small: Vec<(String, Vec<u8>)> = (0..200)
            .map(|n| {
                (
                    format!("package/lib/{n}.js"),
                    format!("export default {n};\n").into_bytes(),
                )
            })
            .collect();
        let mut files: Vec<(&str, &[u8], u32)> = vec![
            ("package/a.bin", &blocks[0], 0o644),
            ("package/b.bin", &blocks[1], 0o755),
        ];
        files.extend(
            small
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice(), 0o644)),
        );
        files.push(("package/c.bin", &blocks[2], 0o644));
        files.push(("package/d.bin", &blocks[3], 0o644));
        let bytes = tarball(&files);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);
        let version = Version::new(1, 0, 0);

        let (_one_directory, one) = store();
        let (_many_directory, many) = store();
        let serial = one
            .with_ingest_threads(1)
            .ingest("demo", &version, &integrity, &bytes)
            .unwrap();
        let threaded = many.with_ingest_threads(8);
        let parallel = threaded
            .ingest("demo", &version, &integrity, &bytes)
            .unwrap();

        assert_eq!(serial, parallel);
        let index = threaded.read_index(&parallel).unwrap();
        assert_eq!(index.files.len(), 204);
        assert!(index.file("b.bin").unwrap().executable);
        assert!(threaded.cas().audit().unwrap().is_clean());
    }

    #[test]
    fn test_the_later_of_two_entries_for_one_path_wins_on_any_thread_count() {
        let mut files: Vec<(&str, &[u8], u32)> = Vec::new();
        for _ in 0..50 {
            files.push(("package/index.js", b"early", 0o644));
            files.push(("package/other.js", b"other", 0o644));
        }
        files.push(("package/index.js", b"late", 0o755));
        let bytes = tarball(&files);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);

        for threads in [1, 8] {
            let (_directory, store) = store();
            let store = store.with_ingest_threads(threads);
            let hash = store
                .ingest("demo", &Version::new(1, 0, 0), &integrity, &bytes)
                .unwrap();
            let index = store.read_index(&hash).unwrap();
            let file = index.file("index.js").unwrap();
            assert_eq!(store.cas().read(&file.hash).unwrap(), b"late");
            assert!(file.executable);
        }
    }

    #[test]
    fn test_identical_files_in_different_packages_share_one_object() {
        let (_directory, store) = store();
        let shared: &[u8] = b"the same license text\n";
        let first = tarball(&[
            ("package/LICENSE", shared, 0o644),
            ("package/a.js", b"a", 0o644),
        ]);
        let second = tarball(&[
            ("package/LICENSE", shared, 0o644),
            ("package/b.js", b"b", 0o644),
        ]);

        let first_hash = store
            .ingest(
                "first",
                &Version::new(1, 0, 0),
                &Integrity::of(crate::integrity::Algorithm::Sha512, &first),
                &first,
            )
            .unwrap();
        let second_hash = store
            .ingest(
                "second",
                &Version::new(1, 0, 0),
                &Integrity::of(crate::integrity::Algorithm::Sha512, &second),
                &second,
            )
            .unwrap();

        let first_index = store.read_index(&first_hash).unwrap();
        let second_index = store.read_index(&second_hash).unwrap();
        assert_eq!(
            first_index.file("LICENSE").unwrap().hash,
            second_index.file("LICENSE").unwrap().hash
        );
        // Two packages, five distinct files, four unique objects plus the two
        // indexes.
        assert_eq!(store.cas().object_hashes().unwrap().len(), 5);
    }

    #[test]
    fn test_refuses_a_tarball_whose_integrity_does_not_match() {
        let (_directory, store) = store();
        let bytes = tarball(&[("package/index.js", b"real", 0o644)]);
        let wrong = Integrity::of(crate::integrity::Algorithm::Sha512, b"something else");

        assert!(matches!(
            store.ingest("demo", &Version::new(1, 0, 0), &wrong, &bytes),
            Err(PackageError::Integrity(_))
        ));
        // Nothing from an unverified tarball reaches the store.
        assert!(store.cas().object_hashes().unwrap().is_empty());
    }

    #[test]
    fn test_rejects_paths_that_escape_the_package() {
        assert_eq!(
            strip_package_root(Path::new("package/lib/a.js")).map(|path| path.into_string()),
            Some("lib/a.js".to_string())
        );
        assert!(strip_package_root(Path::new("package/../../etc/passwd")).is_none());
        assert!(strip_package_root(Path::new("/etc/passwd")).is_none());
        assert!(strip_package_root(Path::new("package")).is_none());
    }

    #[test]
    fn test_empty_tarball_is_an_error_not_an_empty_package() {
        let (_directory, store) = store();
        let bytes = tarball(&[]);
        let integrity = Integrity::of(crate::integrity::Algorithm::Sha512, &bytes);
        assert!(matches!(
            store.ingest("demo", &Version::new(1, 0, 0), &integrity, &bytes),
            Err(PackageError::Empty { .. })
        ));
    }
}
