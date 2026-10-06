//! Where an install says what it is doing.
//!
//! A seam, not a renderer. `opal install` runs silently today — resolve, fetch
//! and link all complete with no output — so a ten-minute run and a fast one
//! look identical while they are happening, which is how the `create-next-app`
//! validation managed to look ordinary at ten minutes.
//!
//! `opal-pm` stays ignorant of terminals for the same reason `opal-core` stays
//! ignorant of tools: a library that knows about spinners cannot be driven by
//! anything that is not one. So the pipeline calls these hooks unconditionally,
//! tests pass [`Silent`], and only `opal-cli` implements one that draws.

use crate::resolve::PackageId;

/// The pipeline's phases, in the order they happen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// Reading `package.json` and asking the registry what satisfies it. Absent
    /// when a usable `opal.lock` answers instead.
    Resolving,
    /// Getting package contents into the store. The slow, network-bound one.
    Fetching { packages: usize },
    /// Reconciling `node_modules` against the plan.
    Linking { packages: usize },
}

/// Called as the install proceeds. Every method has a default, so an
/// implementation only overrides the ones it renders.
///
/// `Sync` because packages are fetched on several threads, and each one calls
/// the download hooks and [`Progress::fetched`] itself. The rest are only ever
/// called from the thread running the install.
pub trait Progress: Sync {
    fn stage(&self, _stage: Stage) {}

    /// Resolution has chosen `settled` packages out of the `known` it can see
    /// from here. `known` is a lower bound that grows as the tree is walked:
    /// a package's own dependencies are unknown until it is chosen. The last
    /// call of a resolve reports the two equal, at the size of the tree.
    fn resolving(&self, _settled: usize, _known: usize) {}

    /// A package's tarball has started arriving. `total` is its size when the
    /// server said. A download that is retried starts again, with its count
    /// back at nothing.
    fn download_started(&self, _id: &PackageId, _total: Option<u64>) {}

    /// `bytes` more of that tarball are here.
    fn downloaded(&self, _id: &PackageId, _bytes: u64) {}

    /// One package's contents are in the store, which ends any download it
    /// had. `from_store` distinguishes a cache hit from a download, which is
    /// the difference between a fast tick and a slow one.
    fn fetched(&self, _id: &PackageId, _from_store: bool) {}

    /// The last stage is done and nothing further will be reported.
    fn finished(&self) {}
}

/// Reports nothing. The default for every caller that is not a terminal.
pub struct Silent;

impl Progress for Silent {}
