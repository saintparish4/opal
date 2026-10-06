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
//!
//! `opal add` and `opal remove` are this pipeline with an edit to
//! `package.json` in front ([`change`]). The edit is made in memory, resolved,
//! and only then written, manifest first and lockfile second: a kill between
//! the two leaves a manifest the lockfile does not match yet, which the next
//! `opal install` resolves, keeping everything the lockfile already settled.
//! The other order would leave a lockfile for a dependency the manifest never
//! gained, and the next install would quietly drop it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use opal_core::atomic::{self, replace_atomic};
use opal_core::fault::FaultPoint;

use crate::edit::{self, AddRequest, Addition, EditError, Group};
use crate::link::{self, Fetched, FetchedPackage, Layout, LinkError, LinkReport};
use crate::lockfile::{self, LockfileError};
use crate::locks::InstallLock;
use crate::manifest::{
    DEPENDENCY_SCRIPTS, DependencyClass, Manifest, ManifestError, PROJECT_SCRIPTS, Spec,
};
use crate::package::{PackageError, PackageStore};
use crate::parallel;
use crate::platform::Platform;
use crate::progress::{Progress, Stage};
use crate::projects::{ProjectError, ProjectIndex};
use crate::registry::{BodyObserver, Registry, RegistryError};
use crate::resolve::{
    self, PackageId, Resolution, ResolveError, ResolveOptions, ResolvedPackage, Seed,
};
use crate::semver::Version;

/// `package.json` is written and about to be renamed into place; `opal.lock`
/// still describes the manifest it replaces.
pub const FAULT_BEFORE_MANIFEST_RENAME: FaultPoint = FaultPoint::new("pm-before-manifest-rename");

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
    #[error("{path}: {source}")]
    Edit {
        path: PathBuf,
        #[source]
        source: EditError,
    },
    #[error("adding or removing a dependency rewrites opal.lock, which --frozen-lockfile forbids")]
    FrozenChange,
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

/// An edit to `package.json`, carried out by the install that follows it.
#[derive(Clone, Debug)]
pub enum Change {
    /// Each request is resolved against the registry as it stands, whatever
    /// `opal.lock` holds for that name, and written to `group`. `None` keeps
    /// a package in the group that already lists it.
    Add {
        requests: Vec<AddRequest>,
        group: Option<Group>,
        /// Record the resolved version itself instead of a caret range on it.
        exact: bool,
    },
    /// Each name is taken out of every group that lists it.
    Remove { names: Vec<String> },
}

/// A [`Change`] applied to the manifest's text, not yet written anywhere.
struct Edited {
    text: String,
    /// What each added name resolved to, which the resolve is then held to:
    /// the range written was computed from that version, and a registry that
    /// published in between must not make the two disagree.
    pinned: BTreeMap<String, Version>,
    removed: Vec<String>,
}

impl Edited {
    fn of(
        change: &Change,
        text: &str,
        path: &Path,
        registry: &dyn Registry,
    ) -> Result<Self, InstallError> {
        let refused = |source| InstallError::Edit {
            path: path.to_path_buf(),
            source,
        };
        match change {
            Change::Add {
                requests,
                group,
                exact,
            } => {
                let mut pinned = BTreeMap::new();
                let mut additions = Vec::with_capacity(requests.len());
                for request in requests {
                    let typed = request.spec.as_deref().map(Spec::parse);
                    let version = resolve::named(registry, &request.name, typed.as_ref())?;
                    additions.push(Addition {
                        name: request.name.clone(),
                        spec: edit::saved_spec(typed.as_ref(), &version, *exact),
                        group: *group,
                    });
                    pinned.insert(request.name.clone(), version);
                }
                Ok(Self {
                    text: edit::add(text, &additions).map_err(refused)?,
                    pinned,
                    removed: Vec::new(),
                })
            }
            Change::Remove { names } => Ok(Self {
                text: edit::remove(text, names).map_err(refused)?,
                pinned: BTreeMap::new(),
                removed: names.clone(),
            }),
        }
    }
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
    /// The project's own dependencies this run put in `node_modules`, as the
    /// name the project requires and what was installed under it, runtime
    /// dependencies first. Empty when the tree was already in place; after a
    /// killed install, the ones the re-run had to supply.
    pub added_direct: Vec<(String, PackageId)>,
    /// What `opal add` was asked for: each name as the project requires it,
    /// and what it resolved to. Unlike `added_direct`, listed whether or not
    /// the linker had anything to do, since a package already in the tree as
    /// someone else's dependency is still a new dependency of the project.
    pub requested: Vec<(String, PackageId)>,
    /// The names `opal remove` took out of `package.json`.
    pub removed_direct: Vec<String>,
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
    run(
        project_root,
        None,
        registry,
        store,
        projects,
        options,
        progress,
    )
}

/// [`install`], after making `change` to `package.json`.
///
/// Nothing is written unless the changed manifest resolves: a mistyped name
/// or a range nothing satisfies leaves `package.json` and `opal.lock` as they
/// were.
pub fn change(
    project_root: &Path,
    change: &Change,
    registry: &dyn Registry,
    store: &PackageStore,
    projects: &ProjectIndex,
    options: &InstallOptions,
    progress: &dyn Progress,
) -> Result<InstallReport, InstallError> {
    run(
        project_root,
        Some(change),
        registry,
        store,
        projects,
        options,
        progress,
    )
}

fn read_text(path: &Path) -> Result<String, ManifestError> {
    std::fs::read_to_string(path).map_err(|source| ManifestError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn run(
    project_root: &Path,
    change: Option<&Change>,
    registry: &dyn Registry,
    store: &PackageStore,
    projects: &ProjectIndex,
    options: &InstallOptions,
    progress: &dyn Progress,
) -> Result<InstallReport, InstallError> {
    if change.is_some() && options.frozen_lockfile {
        return Err(InstallError::FrozenChange);
    }
    let manifest_path = project_root.join("package.json");
    let node_modules = project_root.join(link::NODE_MODULES);
    // Taking the lock creates `node_modules`, so a directory that is not a
    // project is turned away before it, and gains nothing.
    read_text(&manifest_path)?;

    // Held for the whole run. Two installs against one project serialize here;
    // the kernel drops it if either is killed.
    let _project_lock = InstallLock::acquire(&node_modules).map_err(|source| InstallError::Io {
        path: node_modules.clone(),
        source,
    })?;
    // A run killed between writing `opal.lock` or `package.json` and
    // renaming it into place left its temp file in the project. With the
    // lock held nothing else can be writing one, so whatever is there is
    // that. Clearing it is tidying, and never a reason for this run to fail.
    let lockfile_path = lockfile::path_in(project_root);
    let _ = atomic::remove_stale(&lockfile::temp_path(&lockfile_path));
    if let Ok(temp) = atomic::replacement_temp(&manifest_path) {
        let _ = atomic::remove_stale(&temp);
    }
    // Read under the lock, and not before it: this run may have waited on
    // another that rewrote the manifest, and an edit made to the text as it
    // was before the wait would erase that one.
    let text = read_text(&manifest_path)?;

    // And a shared lock on the cache, taken before anything is written and held
    // to the end. It does not exclude other installs — only `opal cache gc`,
    // which would otherwise be free to sweep an object between marking it and
    // this run creating it. Project lock first, then cache lock, always:
    // collection takes only the cache lock, so no cycle is possible.
    let _cache_lock = store.lock_shared()?;

    let mut report = InstallReport::default();
    // Before the change is worked out, which asks the registry: a manifest
    // opal cannot read is the first thing wrong, and the one to report.
    let manifest = Manifest::parse(&manifest_path, &text)?;
    // Naming a package asks the registry about it, which is already
    // resolving as far as anyone watching is concerned.
    let resolving = change.map(|_| {
        progress.stage(Stage::Resolving);
        Instant::now()
    });
    let edited = change
        .map(|change| Edited::of(change, &text, &manifest_path, registry))
        .transpose()?;
    let manifest = match &edited {
        Some(edited) => Manifest::parse(&manifest_path, &edited.text)?,
        None => manifest,
    };
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
    let resolution = match existing {
        // A change always resolves, even one that leaves the requirements as
        // they were: `opal add` of a package the lockfile already holds is a
        // request for the version the registry has now.
        Some(resolution)
            if edited.is_none() && resolve::requirements_match(&resolution, &manifest) =>
        {
            resolution
        }
        stale => {
            // Under --frozen-lockfile an older lockfile has already failed above,
            // so no lockfile here means there was no file at all, which calls for
            // a different remedy than a stale one.
            if options.frozen_lockfile {
                return Err(if stale.is_some() {
                    InstallError::LockfileOutdated
                } else {
                    InstallError::LockfileMissing
                });
            }
            let started = resolving.unwrap_or_else(|| {
                progress.stage(Stage::Resolving);
                Instant::now()
            });
            let seed = Seed {
                locked: stale.as_ref(),
                pinned: edited
                    .as_ref()
                    .map(|edited| edited.pinned.clone())
                    .unwrap_or_default(),
            };
            // Always resolves devDependencies, whatever this install links, so
            // one lockfile serves a dev install and a production one.
            let resolved = resolve::resolve_from(
                registry,
                &manifest,
                &ResolveOptions {
                    include_development: true,
                    ..ResolveOptions::default()
                },
                &seed,
                progress,
            )?;
            // Rendered before the manifest is written, because rendering can
            // refuse, and a manifest written ahead of a lockfile that never
            // follows is the one state this order exists to rule out.
            let rendered = lockfile::render(&resolved)?;
            if let Some(edited) = &edited
                && edited.text != text
            {
                replace_atomic(
                    &manifest_path,
                    edited.text.as_bytes(),
                    Some(FAULT_BEFORE_MANIFEST_RENAME),
                )
                .map_err(|source| InstallError::Io {
                    path: manifest_path.clone(),
                    source,
                })?;
            }
            lockfile::write_rendered(&lockfile_path, &rendered)?;
            report.timings.resolve = started.elapsed();
            report.resolved = true;
            resolved
        }
    };
    if let Some(edited) = edited {
        report.requested = edited
            .pinned
            .keys()
            .filter_map(|name| {
                let record = resolution
                    .requirements
                    .iter()
                    .find(|record| &record.name == name)?;
                let version = record.version.clone()?;
                Some((
                    name.clone(),
                    PackageId::new(record.package.clone(), version),
                ))
            })
            .collect();
        report.removed_direct = edited.removed;
    }

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
    report.added_direct = added_direct(&resolution, &plan.layout, &report.link, options);
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

/// The root requirements whose own top-level placement the linker just made.
fn added_direct(
    resolution: &Resolution,
    layout: &Layout,
    link: &LinkReport,
    options: &InstallOptions,
) -> Vec<(String, PackageId)> {
    let mut added: Vec<(String, PackageId)> = Vec::new();
    for requirement in &resolution.requirements {
        if !options.include_development && requirement.class == DependencyClass::Development {
            continue;
        }
        // One name can be required in two classes; it has one directory.
        if added.iter().any(|(name, _)| name == &requirement.name) {
            continue;
        }
        let path = link::root_slot(&requirement.name);
        if link.added_paths.binary_search(&path).is_err() {
            continue;
        }
        if let Some(id) = layout.get(&path) {
            added.push((requirement.name.clone(), id.clone()));
        }
    }
    added
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
/// One package's download, as the registry reports it, passed on under the
/// package's name: the registry knows a URL, and progress is about packages.
struct Download<'a> {
    id: &'a PackageId,
    progress: &'a dyn Progress,
}

impl BodyObserver for Download<'_> {
    fn started(&self, total: Option<u64>) {
        self.progress.download_started(self.id, total);
    }

    fn received(&self, bytes: u64) {
        self.progress.downloaded(self.id, bytes);
    }
}

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
                    let download = Download { id, progress };
                    let tarball = registry.tarball_observed(&package.tarball, &download)?;
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
