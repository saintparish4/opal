//! Which projects use this cache.
//!
//! The cache is global and projects live anywhere, so garbage collection has no
//! way to find the lockfiles that keep objects alive unless installs leave a
//! trail. Every successful `opal install` records its project root here, and
//! `opal cache gc` walks the trail to build its mark set.
//!
//! Entries are a hint, not a source of truth: one that no longer names a project
//! with a lockfile is dropped on the next collection, and a project missing from
//! the index only risks its packages being re-downloaded (its `node_modules`
//! files are hardlinks and survive their CAS entry being unlinked).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use opal_core::atomic::write_atomic;
use opal_core::hash::HashBuilder;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl ProjectError {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProjectIndex {
    directory: PathBuf,
}

impl ProjectIndex {
    pub fn new(directory: impl Into<PathBuf>) -> Result<Self, ProjectError> {
        let directory = directory.into();
        std::fs::create_dir_all(&directory)
            .map_err(|source| ProjectError::io(&directory, source))?;
        Ok(Self { directory })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Notes that this project's lockfile keeps objects alive. Idempotent.
    pub fn record(&self, project_root: &Path) -> Result<(), ProjectError> {
        let canonical = canonical(project_root);
        let path = self.entry_path(&canonical);
        write_atomic(&path, format!("{}\n", canonical.display()).as_bytes(), None)
            .map_err(|source| ProjectError::io(path, source))
    }

    pub fn forget(&self, project_root: &Path) -> Result<(), ProjectError> {
        let path = self.entry_path(&canonical(project_root));
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(ProjectError::io(path, source)),
        }
    }

    /// Every recorded project root, deduplicated and sorted.
    pub fn known(&self) -> Result<Vec<PathBuf>, ProjectError> {
        let entries = match std::fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(ProjectError::io(&self.directory, source)),
        };

        let mut roots = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(|source| ProjectError::io(&self.directory, source))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            // An unreadable entry is a hint we cannot use, not a failure: the
            // worst case is a re-download.
            if let Ok(text) = std::fs::read_to_string(&path) {
                let recorded = text.trim();
                if !recorded.is_empty() {
                    roots.insert(PathBuf::from(recorded));
                }
            }
        }
        Ok(roots.into_iter().collect())
    }

    fn entry_path(&self, project_root: &Path) -> PathBuf {
        let key = HashBuilder::new("opal.pm.project.v1")
            .push_str(&project_root.to_string_lossy())
            .finish()
            .to_hex();
        self.directory.join(key)
    }
}

/// Resolves symlinks where possible so one project has one entry.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_records_are_idempotent_and_readable() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let index = ProjectIndex::new(directory.path().join("projects")).unwrap();

        assert!(index.known().unwrap().is_empty());
        index.record(&project).unwrap();
        index.record(&project).unwrap();

        let known = index.known().unwrap();
        assert_eq!(known.len(), 1);
        assert_eq!(known[0], std::fs::canonicalize(&project).unwrap());
    }

    #[test]
    fn test_forget_removes_an_entry() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let index = ProjectIndex::new(directory.path().join("projects")).unwrap();

        index.record(&project).unwrap();
        index.forget(&project).unwrap();
        assert!(index.known().unwrap().is_empty());
        // Forgetting something unknown is not an error.
        index.forget(&project).unwrap();
    }
}
