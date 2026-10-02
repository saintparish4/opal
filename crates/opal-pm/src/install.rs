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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::link::{self, Fetched, FetchedPackage, Layout, LinkError, LinkReport};
use crate::lockfile::{self, LockfileError};
use crate::locks::InstallLock;
use crate::manifest::{DEPENDENCY_SCRIPTS, Manifest, ManifestError, PROJECT_SCRIPTS};
use crate::package::{PackageError, PackageStore};
use crate::parallel;
use crate::platform::Platform;
use crate::progress::{Progress, Stage};
use crate::projects::{ProjectError, ProjectIndex};
use crate::registry::{Registry, RegistryError};
use crate::resolve::{self, PackageId, Resolution, ResolveError, ResolveOptions, ResolvedPackage};

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
    #[error(
        "there is no opal.lock to install from, and --frozen-lockfile was requested; \
         run `opal install` without it once to create opal.lock, then commit it"
    )]
    LockfileMissing,
    #[error(
        "{}",
        rejected
            .iter()
            .map(|(id, reason)| format!("{id} {reason}"))
            .collect::<Vec<_>>()
            .join("; ")
    )]
    UnsupportedPlatform { rejected: Vec<(PackageId, String)> },
}

#[derive(Clone, Debug)]
pub struct InstallOptions {
    /// Whether the project's own `devDependencies` are linked. Resolution
    /// always covers them — see [`resolve::requirements_match`] — so this only
    /// decides what the tree contains, never what the lockfile says.
    pub include_development: bool,
    /// CI mode: refuse to re-resolve, so a stale lockfile fails the build
    /// instead of being quietly rewritten.
    pub frozen_lockfile: bool,
    /// The host to install for. Overridable so a test can install a tree for a
    /// platform it is not running on.
    pub platform: Platform,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            include_development: true,
            frozen_lockfile: false,
            platform: Platform::host(),
        }
    }
}

/// Wall time per phase.
///
/// One total hides which phase cost what, and the phases fail for unrelated
/// reasons: resolution is round-trip bound, fetching is bandwidth bound, and
/// linking is filesystem bound. A ten-minute install that is nine minutes of
/// linking and one of resolving needs a different fix from the reverse, and
/// the summary line is where someone looks first.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub resolve: Duration,
    pub fetch: Duration,
    pub link: Duration,
}

#[derive(Debug, Default)]
pub struct InstallReport {
    pub packages: usize,
    pub resolved: bool,
    pub fetched: usize,
    pub already_stored: usize,
    pub link: LinkReport,
    pub skipped: Vec<(String, String)>,
    /// Resolved, recorded in the lockfile, and not installed here because this
    /// host cannot run them.
    pub platform_skipped: Vec<(PackageId, String)>,
    /// An older lockfile was replaced by re-resolving.
    pub lockfile_upgraded: bool,
    pub timings: Timings,
    /// Packages in this tree the registry marks deprecated, with the message
    /// its publisher left. Only populated on a run that resolved: the message
    /// lives in the packument, and `opal.lock` does not carry it, so a run
    /// answered entirely by the lockfile has nothing to report.
    pub deprecated: Vec<(PackageId, String)>,
    /// Packages in this tree with install scripts, which opal does not run.
    /// Populated on every run, including one `opal.lock` answered, because
    /// the consequence (a native addon that was never built) persists until
    /// something runs them.
    pub scripts_not_run: Vec<(PackageId, UnrunScripts)>,
    /// The project's own lifecycle scripts, which npm runs on every install
    /// of it (`patch-package` and `husky` live here), and opal does not.
    pub project_scripts_not_run: Option<UnrunScripts>,
}

/// Lifecycle scripts that exist and were not run.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnrunScripts {
    /// The lifecycle events declared, in the order npm runs them.
    pub events: Vec<&'static str>,
    /// No `install` or `preinstall`, but a `binding.gyp` at the root, for
    /// which npm supplies `node-gyp rebuild` as the install script. Native
    /// addons commonly rely on exactly this and declare nothing.
    pub implicit_node_gyp: bool,
}

impl UnrunScripts {
    /// What npm would run, out of `lifecycle`, for the package in `directory`.
    fn find(manifest: &Manifest, directory: &Path, lifecycle: &[&'static str]) -> Option<Self> {
        let events: Vec<&'static str> = lifecycle
            .iter()
            .copied()
            .filter(|event| manifest.lifecycle_scripts.contains_key(event))
            .collect();
        let declares_install = events.contains(&"install") || events.contains(&"preinstall");
        let implicit_node_gyp = !declares_install
            && !manifest.gypfile_opt_out
            && directory.join("binding.gyp").is_file();
        (!events.is_empty() || implicit_node_gyp).then_some(Self {
            events,
            implicit_node_gyp,
        })
    }
}

pub fn install(
    project_root: &Path,
    registry: &dyn Registry,
    store: &PackageStore,
    projects: &ProjectIndex,
    options: &InstallOptions,
    progress: &dyn Progress,
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

    let mut report = InstallReport::default();
    let lockfile_path = lockfile::path_in(project_root);
    // A lockfile this build is too new to read is replaced by re-resolving,
    // which is only ever silent outside CI: under --frozen-lockfile the error
    // propagates, because a build that promised not to rewrite the lockfile
    // must not rewrite it to upgrade it either.
    let existing = match lockfile::read(&lockfile_path) {
        Ok(existing) => existing,
        Err(error) if error.is_outdated_version() && !options.frozen_lockfile => {
            report.lockfile_upgraded = true;
            None
        }
        Err(error) => return Err(error.into()),
    };
    let lockfile_present = existing.is_some();
    let reusable = existing.filter(|resolution| resolve::requirements_match(resolution, &manifest));

    let resolution = match reusable {
        Some(resolution) => resolution,
        None => {
            // Under --frozen-lockfile an older lockfile has already failed above,
            // so no lockfile here means there was no file at all, which calls for
            // a different remedy than a stale one.
            if options.frozen_lockfile {
                return Err(if lockfile_present {
                    InstallError::LockfileOutdated
                } else {
                    InstallError::LockfileMissing
                });
            }
            progress.stage(Stage::Resolving);
            let started = Instant::now();
            // Always resolves devDependencies, whatever this install links, so
            // one lockfile serves a dev install and a production one.
            let resolved = resolve::resolve(
                registry,
                &manifest,
                &ResolveOptions {
                    include_development: true,
                    ..ResolveOptions::default()
                },
            )?;
            lockfile::write(&lockfile_path, &resolved)?;
            report.timings.resolve = started.elapsed();
            report.resolved = true;
            resolved
        }
    };

    // Recorded as soon as there is a lockfile worth marking from — before
    // fetching, not after linking. A killed install's already-fetched packages
    // then stay live for the retry instead of being collected in between.
    projects.record(project_root)?;

    // Planned before anything is fetched, so a package the tree does not
    // contain — another platform's native binary, a devDependency under
    // --production — is never downloaded either.
    let plan = link::plan(
        &resolution,
        &link::PlanOptions {
            platform: options.platform.clone(),
            include_development: options.include_development,
        },
    );
    report.packages = plan.layout.len();

    progress.stage(Stage::Fetching {
        packages: report.packages,
    });
    // A dependency the host cannot run, that nothing declared optional, is
    // npm's EBADPLATFORM: linking it produces a tree that fails at run time
    // instead of an install that fails now.
    if !plan.platform_rejected.is_empty() {
        return Err(InstallError::UnsupportedPlatform {
            rejected: plan.platform_rejected,
        });
    }

    let started = Instant::now();
    let fetched = fetch_all(
        registry,
        store,
        &resolution,
        &plan.layout,
        &mut report,
        progress,
    )?;
    report.timings.fetch = started.elapsed();

    progress.stage(Stage::Linking {
        packages: report.packages,
    });
    let started = Instant::now();
    report.link = link::reconcile(project_root, &plan.layout, &fetched, store.cas())?;
    report.timings.link = started.elapsed();
    report.skipped = resolution.skipped.clone();
    report.platform_skipped = plan.platform_skipped;

    // Reads whatever metadata is already local and asks for nothing. After a
    // resolve that is every packument it just fetched; on a lockfile-reuse run
    // it is whatever the on-disk cache still holds, which is why this survives
    // an install the lockfile answered entirely.
    report.deprecated = deprecations(registry, &plan.layout);
    report.scripts_not_run = scripts_not_run(project_root, &plan.layout);
    report.project_scripts_not_run = UnrunScripts::find(&manifest, project_root, &PROJECT_SCRIPTS);

    progress.finished();
    Ok(report)
}

/// What the registry says about the packages actually being installed.
fn deprecations(registry: &dyn Registry, layout: &Layout) -> Vec<(PackageId, String)> {
    let mut found: Vec<(PackageId, String)> = Vec::new();
    for id in layout.values() {
        if found.iter().any(|(seen, _)| seen == id) {
            continue;
        }
        // Absent metadata is not a reason to fail an install that has already
        // succeeded — it just means there is nothing to say about it.
        let Some(packument) = registry.cached_packument(&id.name) else {
            continue;
        };
        if let Some(message) = packument
            .version(&id.version)
            .and_then(|metadata| metadata.deprecated.clone())
        {
            found.push((id.clone(), message));
        }
    }
    found.sort();
    found
}

/// Every package in the linked tree with install scripts.
///
/// Read from each package's materialized directory, the same place a script
/// runner will discover them, so neither the package index nor the lockfile
/// has to carry it.
fn scripts_not_run(project_root: &Path, layout: &Layout) -> Vec<(PackageId, UnrunScripts)> {
    let mut seen: BTreeSet<&PackageId> = BTreeSet::new();
    let mut found = Vec::new();
    for (path, id) in layout {
        if !seen.insert(id) {
            continue;
        }
        let directory = project_root.join(path.as_str());
        // The install has already succeeded; a manifest that cannot be read
        // is not a reason to fail it, and has no scripts to report.
        let Ok(manifest) = Manifest::read(&directory.join("package.json")) else {
            continue;
        };
        if let Some(scripts) = UnrunScripts::find(&manifest, &directory, &DEPENDENCY_SCRIPTS) {
            found.push((id.clone(), scripts));
        }
    }
    found.sort();
    found
}

/// What one fetching thread got done.
#[derive(Default)]
struct Batch {
    packages: Vec<(PackageId, FetchedPackage)>,
    downloaded: usize,
    already_stored: usize,
}

/// Ensures the contents of everything the planned tree names are in the store.
///
/// Packages are fetched and ingested several at a time: each is independent
/// of the others, and every write underneath is content-addressed or atomic,
/// so two threads ingesting at once cannot disturb each other. The result is
/// keyed by package, so it is the same whichever download finished first.
fn fetch_all(
    registry: &dyn Registry,
    store: &PackageStore,
    resolution: &Resolution,
    layout: &Layout,
    report: &mut InstallReport,
    progress: &dyn Progress,
) -> Result<Fetched, InstallError> {
    // Once per package, however many places the layout puts it.
    let wanted: BTreeMap<&PackageId, &ResolvedPackage> = layout
        .values()
        .filter_map(|id| Some((id, resolution.package(id)?)))
        .collect();
    let wanted: Vec<(&PackageId, &ResolvedPackage)> = wanted.into_iter().collect();

    let batches = parallel::each(
        &wanted,
        parallel::REQUESTS.min(wanted.len()),
        Batch::default,
        |(id, package), batch: &mut Batch| {
            let stored = store.lookup(&package.integrity)?;
            let from_store = stored.is_some();
            let index_hash = match stored {
                Some(hash) => {
                    batch.already_stored += 1;
                    hash
                }
                None => {
                    let tarball = registry.tarball(&package.tarball)?;
                    let hash = store.ingest(&id.name, &id.version, &package.integrity, &tarball)?;
                    batch.downloaded += 1;
                    hash
                }
            };
            progress.fetched(id, from_store);
            let index = store.read_index(&index_hash)?;
            batch
                .packages
                .push(((*id).clone(), FetchedPackage { index_hash, index }));
            Ok::<(), InstallError>(())
        },
    )?;

    let mut fetched: Fetched = BTreeMap::new();
    for batch in batches {
        report.fetched += batch.downloaded;
        report.already_stored += batch.already_stored;
        fetched.extend(batch.packages);
    }
    Ok(fetched)
}
