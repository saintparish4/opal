//! The install pipeline.
//!
//! ```text
//! package.json -> resolution -> opal.lock -> download -> verify -> CAS -> node_modules
//! ```
//!
//! The order matters and matches PRD §4.3.1: the lockfile is written from the
//! resolution *before* anything is downloaded, so a crash during download leaves
//! a valid lockfile describing what should be there. Every later stage is keyed
//! by content, so re-running skips whatever is already done — a re-run with an
//! unchanged lockfile goes straight to linking.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::link::{self, Fetched, FetchedPackage, LinkError, LinkReport};
use crate::lockfile::{self, LockfileError};
use crate::locks::InstallLock;
use crate::manifest::{Manifest, ManifestError};
use crate::package::{PackageError, PackageStore};
use crate::projects::{ProjectError, ProjectIndex};
use crate::registry::{Registry, RegistryError};
use crate::resolve::{self, Resolution, ResolveError, ResolveOptions};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Lockfile(#[from] LockfileError),
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error(transparent)]
    Link(#[from] LinkError),
    #[error(transparent)]
    Projects(#[from] ProjectError),
    #[error("opal.lock does not match package.json, and --frozen-lockfile was requested")]
    LockfileOutdated,
}

#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub include_development: bool,
    /// CI mode: refuse to re-resolve, so a stale lockfile fails the build
    /// instead of being quietly rewritten.
    pub frozen_lockfile: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            include_development: true,
            frozen_lockfile: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct InstallReport {
    pub packages: usize,
    pub resolved: bool,
    pub fetched: usize,
    pub already_stored: usize,
    pub link: LinkReport,
    pub skipped: Vec<(String, String)>,
}

pub fn install(
    project_root: &Path,
    registry: &dyn Registry,
    store: &PackageStore,
    projects: &ProjectIndex,
    options: &InstallOptions,
) -> Result<InstallReport, InstallError> {
    let manifest = Manifest::read(&project_root.join("package.json"))?;
    let node_modules = project_root.join(link::NODE_MODULES);

    // Held for the whole run. Two installs against one project serialize here;
    // the kernel drops it if either is killed.
    let _project_lock = InstallLock::acquire(&node_modules).map_err(|source| InstallError::Io {
        path: node_modules.clone(),
        source,
    })?;

    // And a shared lock on the cache, taken before anything is written and held
    // to the end. It does not exclude other installs — only `opal cache gc`,
    // which would otherwise be free to sweep an object between marking it and
    // this run creating it. Project lock first, then cache lock, always:
    // collection takes only the cache lock, so no cycle is possible.
    let _cache_lock = store.lock_shared()?;

    let lockfile_path = lockfile::path_in(project_root);
    let existing = lockfile::read(&lockfile_path)?;
    let reusable = existing.filter(|resolution| {
        resolve::requirements_match(resolution, &manifest, options.include_development)
    });

    let mut report = InstallReport::default();
    let resolution = match reusable {
        Some(resolution) => resolution,
        None => {
            if options.frozen_lockfile {
                return Err(InstallError::LockfileOutdated);
            }
            let resolved = resolve::resolve(
                registry,
                &manifest,
                &ResolveOptions {
                    include_development: options.include_development,
                },
            )?;
            lockfile::write(&lockfile_path, &resolved)?;
            report.resolved = true;
            resolved
        }
    };

    // Recorded as soon as there is a lockfile worth marking from — before
    // fetching, not after linking. A killed install's already-fetched packages
    // then stay live for the retry instead of being collected in between.
    projects.record(project_root)?;

    let fetched = fetch_all(registry, store, &resolution, &mut report)?;
    let layout = link::plan(&resolution);
    report.packages = layout.len();
    report.link = link::reconcile(project_root, &layout, &fetched, store.cas())?;
    report.skipped = resolution.skipped.clone();
    Ok(report)
}

/// Ensures every resolved package's contents are in the store.
fn fetch_all(
    registry: &dyn Registry,
    store: &PackageStore,
    resolution: &Resolution,
    report: &mut InstallReport,
) -> Result<Fetched, InstallError> {
    let mut fetched: Fetched = BTreeMap::new();

    for (id, package) in &resolution.packages {
        let index_hash = match store.lookup(&package.integrity)? {
            Some(hash) => {
                report.already_stored += 1;
                hash
            }
            None => {
                let tarball = registry.tarball(&package.tarball)?;
                let hash = store.ingest(&id.name, &id.version, &package.integrity, &tarball)?;
                report.fetched += 1;
                hash
            }
        };
        let index = store.read_index(&index_hash)?;
        fetched.insert(id.clone(), FetchedPackage { index_hash, index });
    }
    Ok(fetched)
}
