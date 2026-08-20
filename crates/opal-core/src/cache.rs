//! Where the shared cache lives.
//!
//! One directory holds the CAS and the memo records for every project on the
//! machine. That is deliberate: the graphs, and later the packages, are
//! content-addressed, so two checkouts of the same project — or two different
//! projects sharing a dependency — reuse the same objects.

use std::path::{Path, PathBuf};

use crate::cas::{Cas, CasError};
use crate::graph::memo::{GraphCache, MemoError};

/// Overrides cache location. Tests and CI set this; users rarely need to.
pub const CACHE_DIR_ENV: &str = "OPAL_CACHE_DIR";

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cannot locate a cache directory: set {CACHE_DIR_ENV} or HOME")]
    NoHome,
    #[error(transparent)]
    Cas(#[from] CasError),
    #[error(transparent)]
    Memo(#[from] MemoError),
}

#[derive(Clone, Debug)]
pub struct CacheRoot {
    path: PathBuf,
}

impl CacheRoot {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$OPAL_CACHE_DIR`, else the platform cache directory.
    pub fn discover() -> Result<Self, CacheError> {
        if let Some(explicit) = std::env::var_os(CACHE_DIR_ENV) {
            return Ok(Self::at(explicit));
        }
        let home = std::env::var_os("HOME").ok_or(CacheError::NoHome)?;
        let home = PathBuf::from(home);

        #[cfg(target_os = "macos")]
        let base = home.join("Library").join("Caches");
        #[cfg(not(target_os = "macos"))]
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".cache"));

        Ok(Self::at(base.join("opal")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn cas_dir(&self) -> PathBuf {
        self.path.join("cas")
    }

    pub fn records_dir(&self) -> PathBuf {
        self.path.join("memo")
    }

    pub fn open_cas(&self) -> Result<Cas, CacheError> {
        Ok(Cas::open(self.cas_dir())?)
    }

    /// Opens the store and the memo index together — the handle tools share.
    pub fn open(&self) -> Result<GraphCache, CacheError> {
        Ok(GraphCache::new(self.open_cas()?, self.records_dir())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_is_stable() {
        let root = CacheRoot::at("/cache/opal");
        assert_eq!(root.cas_dir(), PathBuf::from("/cache/opal/cas"));
        assert_eq!(root.records_dir(), PathBuf::from("/cache/opal/memo"));
    }

    #[test]
    fn test_open_creates_both_halves() {
        let dir = tempfile::tempdir().unwrap();
        let root = CacheRoot::at(dir.path());
        let cache = root.open().unwrap();
        assert!(root.cas_dir().is_dir());
        assert!(root.records_dir().is_dir());
        assert!(cache.live_outputs().unwrap().is_empty());
    }
}
