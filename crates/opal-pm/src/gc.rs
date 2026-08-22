//! The mark phase for package contents.
//!
//! `opal-core`'s collector sweeps anything outside a live set, and deliberately
//! knows nothing about where that set comes from — that is this module's job.
//! An object is live when some project's `opal.lock` still names the package it
//! belongs to:
//!
//! ```text
//! opal.lock ──▶ integrity ──▶ pointer ──▶ package index ──▶ every file hash
//! ```
//!
//! Without this, the first `opal cache gc` after an install would find package
//! objects unreferenced and collect a working tree's contents.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use opal_core::cas::gc::{GcOptions, GcReport};
use opal_core::hash::ContentHash;

use crate::lockfile;
use crate::package::{PackageError, PackageStore};
use crate::projects::{ProjectError, ProjectIndex};

#[derive(Debug, thiserror::Error)]
pub enum GcError {
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error(transparent)]
    Cas(#[from] opal_core::cas::CasError),
    #[error(transparent)]
    Projects(#[from] ProjectError),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// What one `opal cache gc` did.
#[derive(Debug)]
pub struct Collection {
    pub marks: Marks,
    pub sweep: GcReport,
    pub pointers_pruned: usize,
}

/// Marks and sweeps, holding the cache lock across both.
///
/// The lock is the whole point of this function existing rather than callers
/// composing [`mark`] and `opal_core::cas::gc::collect` themselves: a mark set
/// computed while an install is running describes a cache that no longer
/// exists by the time the sweep runs.
///
/// `also_live` carries the other mark phase — graph snapshots the memo index
/// still points at. A miss in either one collects live objects.
pub fn collect(
    store: &PackageStore,
    projects: &ProjectIndex,
    extra: &[PathBuf],
    also_live: &BTreeSet<ContentHash>,
    options: &GcOptions,
) -> Result<Collection, GcError> {
    let _lock = store.lock_exclusive()?;

    let marks = mark(store, projects, extra)?;
    let mut live = marks.live.clone();
    live.extend(also_live.iter().copied());

    let sweep = opal_core::cas::gc::collect(store.cas(), &live, options)?;
    let pointers_pruned = if options.dry_run {
        0
    } else {
        prune_pointers(store)?
    };

    Ok(Collection {
        marks,
        sweep,
        pointers_pruned,
    })
}

#[derive(Debug, Default)]
pub struct Marks {
    /// Objects that must survive: package indexes and every file they name.
    pub live: BTreeSet<ContentHash>,
    pub projects: usize,
    pub packages: usize,
    /// Recorded projects that no longer exist, or no longer have a lockfile.
    /// Dropped from the index by [`mark`].
    pub forgotten: Vec<PathBuf>,
    /// Lockfile entries whose contents were never fetched into this cache.
    pub unfetched: usize,
    /// Lockfiles that exist but could not be read.
    pub unreadable: Vec<(PathBuf, String)>,
}

/// Builds the live set from every project this cache knows about.
///
/// `extra` roots are marked without being recorded — for CI, where the cache
/// outlives the checkout that filled it.
///
/// Callers that intend to sweep should use [`collect`], which holds the cache
/// lock across both phases. This is public for inspection and testing.
pub fn mark(
    store: &PackageStore,
    index: &ProjectIndex,
    extra: &[PathBuf],
) -> Result<Marks, GcError> {
    let mut marks = Marks::default();
    let known = index.known()?;

    for project in &known {
        if !mark_project(store, project, &mut marks)? {
            // The project is gone, so its entry cannot keep anything alive.
            index.forget(project)?;
            marks.forgotten.push(project.clone());
        }
    }
    for project in extra {
        mark_project(store, project, &mut marks)?;
    }
    Ok(marks)
}

/// Marks one project. Returns whether the project is still worth tracking.
fn mark_project(
    store: &PackageStore,
    project_root: &Path,
    marks: &mut Marks,
) -> Result<bool, GcError> {
    let path = lockfile::path_in(project_root);
    let resolution = match lockfile::read(&path) {
        Ok(Some(resolution)) => resolution,
        Ok(None) => return Ok(false),
        // A lockfile that exists but does not parse is not evidence that the
        // project is gone, so its entry stays and nothing is collected on its
        // account.
        Err(error) => {
            marks.unreadable.push((path, error.to_string()));
            return Ok(true);
        }
    };

    marks.projects += 1;
    for package in resolution.packages.values() {
        let Some(index_hash) = store.lookup(&package.integrity)? else {
            marks.unfetched += 1;
            continue;
        };
        marks.live.insert(index_hash);
        marks.packages += 1;

        let index = store.read_index(&index_hash)?;
        marks
            .live
            .extend(index.files.values().map(|file| file.hash));
    }
    Ok(true)
}

/// Removes pointers whose package index is no longer in the store.
///
/// Safe at any time: [`PackageStore::lookup`] already treats a dangling pointer
/// as a miss, so this only stops them accumulating.
pub fn prune_pointers(store: &PackageStore) -> Result<usize, GcError> {
    let mut pruned = 0;
    for (path, index_hash) in store.pointer_entries()? {
        if store.cas().contains(&index_hash) {
            continue;
        }
        std::fs::remove_file(&path).map_err(|source| GcError::Io { path, source })?;
        pruned += 1;
    }
    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::{Algorithm, Integrity};
    use opal_core::cas::Cas;

    fn store(directory: &Path) -> PackageStore {
        let cas = Cas::open(directory.join("cas")).unwrap();
        PackageStore::open(cas, directory).unwrap()
    }

    #[test]
    fn test_a_project_without_a_lockfile_is_forgotten() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        let index = ProjectIndex::new(directory.path().join("projects")).unwrap();
        let project = directory.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        index.record(&project).unwrap();

        let marks = mark(&store, &index, &[]).unwrap();
        assert_eq!(marks.forgotten.len(), 1);
        assert!(marks.live.is_empty());
        assert!(index.known().unwrap().is_empty());
    }

    #[test]
    fn test_prune_removes_pointers_to_collected_indexes() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());

        let tarball = {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            {
                let mut builder = tar::Builder::new(&mut encoder);
                let contents = b"module.exports = 1;\n";
                let mut header = tar::Header::new_gnu();
                header.set_size(contents.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, "package/index.js", &contents[..])
                    .unwrap();
                builder.finish().unwrap();
            }
            encoder.finish().unwrap()
        };
        let integrity = Integrity::of(Algorithm::Sha512, &tarball);
        let index_hash = store
            .ingest(
                "demo",
                &crate::semver::Version::new(1, 0, 0),
                &integrity,
                &tarball,
            )
            .unwrap();

        assert_eq!(prune_pointers(&store).unwrap(), 0);
        std::fs::remove_file(store.cas().object_path(&index_hash)).unwrap();
        assert_eq!(prune_pointers(&store).unwrap(), 1);
        assert_eq!(store.lookup(&integrity).unwrap(), None);
    }
}
