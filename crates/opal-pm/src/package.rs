//! Package contents in the content-addressed store.
//!
//! A tarball is never unpacked to disk as a unit. Each file inside it becomes
//! its own CAS object, and the package is represented by an *index* — a sorted
//! map of relative path to content hash — which is itself a CAS object. Two
//! packages that ship an identical file share one object on disk, which is the
//! cross-package deduplication PRD §4.3 is after, and it means the linker can
//! materialize a package with hardlinks alone.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use opal_core::atomic::write_atomic;
use opal_core::cas::{Cas, CasError};
use opal_core::fault::{self, FaultPoint};
use opal_core::hash::{ContentHash, HashBuilder};
use opal_core::path::NormalizedPath;
use serde::{Deserialize, Serialize};

use crate::integrity::{Integrity, IntegrityError};
use crate::locks::CacheLock;
use crate::semver::Version;

/// Tarball downloaded, integrity not yet checked.
pub const FAULT_BEFORE_VERIFY: FaultPoint = FaultPoint::new("pm-before-verify");
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
        })
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

            let hash = self.cas.put_reader(&mut entry)?;
            files.insert(
                path,
                PackageFile {
                    hash,
                    executable: mode & 0o111 != 0,
                    size,
                },
            );
            if files.len() == 1 {
                fault::checkpoint(FAULT_MID_EXTRACT);
            }
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