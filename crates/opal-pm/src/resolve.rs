//! Semver resolution: `package.json` plus the registry, into a resolved graph.
//!
//! The algorithm is npm's in spirit: walk requirements breadth-first, and for
//! each one reuse an already-selected version if it satisfies, otherwise fetch
//! the version npm itself would pick for the range ([`pick`]). Reuse is what
//! keeps the tree small; the layout planner is what copes with the conflicts
//! reuse cannot avoid.
//!
//! Determinism is a requirement, not a nicety — a lockfile that differs between
//! two runs over the same inputs is a lockfile nobody can review. Every
//! collection here is ordered, and the work queue is drained in sorted order.
//!
//! Only the waiting is concurrent. Before each breadth-first level is
//! resolved, the packuments it is about to ask for are fetched several at a
//! time; the level is then resolved one request after another, exactly as if
//! nothing had been fetched ahead. The order selections are made in is the
//! result, so that order never depends on which response arrived first.
//!
//! A resolve that replaces a lockfile starts from it ([`Seed`]): a request
//! the lockfile already answered gets that answer again, and only what the
//! manifest newly asks for is answered by the registry.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;

use crate::integrity::Integrity;
use crate::manifest::{DependencyClass, Manifest, Spec};
use crate::parallel;
use crate::progress::{Progress, Silent};
use crate::registry::{Packument, Registry, RegistryError, VersionMetadata};
use crate::semver::{Range, Version};

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error("no version of {name} satisfies {spec} (registry offers {available})")]
    NoMatchingVersion {
        name: String,
        spec: String,
        available: usize,
    },
    #[error(
        "{name}@{spec} is not a supported dependency specifier (v1 resolves the public registry only)"
    )]
    UnsupportedSpec { name: String, spec: String },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PackageId {
    pub name: String,
    pub version: Version,
}

impl PackageId {
    pub fn new(name: impl Into<String>, version: Version) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

/// One resolved edge: what was asked for, and what it resolved to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedEdge {
    /// The name this is required under, which an alias makes different from
    /// the package's own name — `string-width-cjs` for `string-width`. It is
    /// the directory the linker places it in.
    pub name: String,
    /// The package actually installed. Equal to `name` unless aliased.
    pub package: String,
    pub spec: String,
    pub version: Version,
    /// Whether the dependent tolerates this being absent. Carried so the
    /// planner can tell npm's two platform outcomes apart: a mismatched
    /// optional is skipped, a mismatched requirement is an error.
    pub optional: bool,
}

impl ResolvedEdge {
    pub fn id(&self) -> PackageId {
        PackageId::new(self.package.clone(), self.version.clone())
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedPackage {
    pub id: PackageId,
    pub tarball: String,
    pub integrity: Integrity,
    /// Sorted by dependency name.
    pub dependencies: Vec<ResolvedEdge>,
    /// `os` and `cpu` as published. Recorded rather than applied here: a
    /// lockfile that resolved away another platform's native binary would be
    /// wrong the moment it was committed and installed on that platform.
    pub os: Vec<String>,
    pub cpu: Vec<String>,
}

/// What the root project asked for, kept so a `package.json` edit can be
/// detected without re-resolving.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RequirementRecord {
    pub class: DependencyClass,
    /// The name the project requires, which an alias makes different from the
    /// package installed under it.
    pub name: String,
    /// The package actually installed. Equal to `name` unless aliased.
    pub package: String,
    pub spec: String,
    /// What this requirement resolved to, or `None` for an optional that was
    /// skipped. Recorded rather than re-derived: the layout is planned from
    /// the root edges, and deriving them from the resolved set by version
    /// order picks a transitive dependency's higher major over the version the
    /// project actually asked for.
    pub version: Option<Version>,
}

/// A root-level placement: the package, and the name the project calls it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Root {
    pub name: String,
    pub id: PackageId,
    pub optional: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolution {
    pub requirements: Vec<RequirementRecord>,
    pub packages: BTreeMap<PackageId, ResolvedPackage>,
    /// Optional dependencies that do not exist or have no matching version.
    /// Recorded so `opal install` can say why something is absent.
    pub skipped: Vec<(String, String)>,
}

impl Resolution {
    pub fn package(&self, id: &PackageId) -> Option<&ResolvedPackage> {
        self.packages.get(id)
    }

    /// Root-level edges, resolved, each with the name it is required under.
    pub fn roots(&self) -> Vec<Root> {
        self.roots_for(true)
    }

    /// Root-level edges, with the project's own `devDependencies` dropped when
    /// they are not being installed. A dev package that another root also
    /// depends on is still reached, through that root.
    pub fn roots_for(&self, include_development: bool) -> Vec<Root> {
        let mut roots: Vec<Root> = self
            .requirements
            .iter()
            .filter(|requirement| {
                include_development || requirement.class != DependencyClass::Development
            })
            .filter_map(|requirement| {
                Some(Root {
                    name: requirement.name.clone(),
                    id: self.root_of(requirement)?,
                    optional: requirement.class.tolerates_absence(),
                })
            })
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }

    fn root_of(&self, requirement: &RequirementRecord) -> Option<PackageId> {
        if let Some(version) = &requirement.version {
            return Some(PackageId::new(requirement.package.clone(), version.clone()));
        }
        // No recorded version: a resolution built by hand, or one parsed from a
        // lockfile written before the field existed. The spec still narrows it
        // correctly for every range — only a dist-tag is unrecoverable here,
        // because the tag maps to a version through the packument, not through
        // anything the lockfile holds.
        let versions = self
            .packages
            .keys()
            .filter(|id| id.name == requirement.package)
            .map(|id| &id.version);
        match Range::parse(&requirement.spec) {
            Ok(range) => range.max_satisfying(versions),
            Err(_) => versions.max(),
        }
        .map(|version| PackageId::new(requirement.package.clone(), version.clone()))
    }
}

#[derive(Clone, Debug)]
pub struct ResolveOptions {
    /// The root project's `devDependencies` are installed; a dependency's are
    /// never installed, which is what keeps a tree from exploding.
    pub include_development: bool,
    /// How many packuments to fetch at once ahead of each breadth-first level.
    /// One fetches nothing ahead, which exists so a test can hold the two to
    /// the same answer.
    pub concurrent_requests: usize,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            include_development: true,
            concurrent_requests: parallel::REQUESTS,
        }
    }
}

/// What a resolve starts from besides the manifest.
///
/// Without one, every requirement is answered by the registry as it stands
/// today, so a lockfile re-resolved after any edit to `package.json` comes
/// back with every package the registry has moved since it was written.
/// Adding one dependency to a five-week-old Next.js lockfile changed 63 of
/// its 435 packages that way. npm and bun both keep what is locked instead.
#[derive(Default)]
pub struct Seed<'a> {
    /// The lockfile being replaced. A requirement it answered is answered
    /// the same way again, and one it never saw takes a version it holds
    /// before the registry is asked for a new one.
    pub locked: Option<&'a Resolution>,
    /// Root requirements the caller has already resolved, by the name the
    /// project requires them under. `opal add` names a package to get what
    /// the registry has now, so these are never answered from `locked`, and
    /// every other requirement on that package moves to the same version
    /// wherever its range allows, so naming a package does not leave a
    /// second copy of it behind.
    pub pinned: BTreeMap<String, Version>,
}

pub fn resolve(
    registry: &dyn Registry,
    root: &Manifest,
    options: &ResolveOptions,
) -> Result<Resolution, ResolveError> {
    resolve_reporting(registry, root, options, &Silent)
}

/// [`resolve`], saying how far it has got. What is reported never feeds back
/// into a decision, so the resolution is the same one `resolve` returns.
pub fn resolve_reporting(
    registry: &dyn Registry,
    root: &Manifest,
    options: &ResolveOptions,
    progress: &dyn Progress,
) -> Result<Resolution, ResolveError> {
    resolve_from(registry, root, options, &Seed::default(), progress)
}

/// [`resolve_reporting`], keeping what `seed` already settled.
///
/// The seed is an input like the manifest is: the same manifest, seed, and
/// registry give the same resolution, and a resolution seeded with itself
/// under an unchanged manifest comes back as it was.
pub fn resolve_from(
    registry: &dyn Registry,
    root: &Manifest,
    options: &ResolveOptions,
    seed: &Seed<'_>,
    progress: &dyn Progress,
) -> Result<Resolution, ResolveError> {
    Resolver {
        registry,
        progress,
        selected: BTreeMap::new(),
        packages: BTreeMap::new(),
        expanded: BTreeSet::new(),
        skipped: Vec::new(),
        root_versions: BTreeMap::new(),
        refused: BTreeMap::new(),
        kept: Kept::of(seed, root, options),
        pinned: &seed.pinned,
    }
    .run(root, options)
}

/// The version a package named on the command line installs at.
///
/// Asked of the registry and never of a lockfile. A bare name means the
/// `latest` tag, as it does to npm, deprecated or not: the user named the
/// package, so the tag is the answer and the warning is theirs to read.
pub fn named(
    registry: &dyn Registry,
    name: &str,
    spec: Option<&Spec>,
) -> Result<Version, ResolveError> {
    let package = match spec {
        Some(Spec::Alias { package, .. }) => package.as_str(),
        _ => name,
    };
    let packument = registry.packument(package)?;
    let tagged = |tag: &str| {
        packument
            .dist_tags
            .get(tag)
            .and_then(|version| packument.version(version))
    };
    let chosen = match spec {
        Some(Spec::Range(range) | Spec::Alias { range, .. }) => pick(&packument, range),
        Some(Spec::Tag(tag)) => tagged(tag),
        Some(Spec::Unsupported(spec)) => {
            return Err(ResolveError::UnsupportedSpec {
                name: name.to_string(),
                spec: spec.clone(),
            });
        }
        None => tagged("latest").or_else(|| {
            packument
                .versions()
                .rev()
                .find_map(|version| packument.version(version))
        }),
    };
    chosen
        .map(|metadata| metadata.version)
        .ok_or_else(|| ResolveError::NoMatchingVersion {
            name: name.to_string(),
            spec: spec.map_or_else(|| "latest".to_string(), Spec::to_string),
            available: packument.published(),
        })
}

/// What a replaced lockfile settled, as the resolver consults it.
#[derive(Default)]
struct Kept<'a> {
    /// Each root requirement the manifest still declares as the lockfile
    /// recorded it, as (name, spec), with the version it resolved to.
    roots: BTreeMap<(String, String), Version>,
    packages: Option<&'a BTreeMap<PackageId, ResolvedPackage>>,
    /// Every locked version of each package, for a request the lockfile
    /// never saw. All of them, including one only a removed requirement led
    /// to: npm and bun both answer a new request from whatever the lockfile
    /// holds, and a version nothing asks for again is simply not selected.
    versions: BTreeMap<&'a str, BTreeSet<&'a Version>>,
    /// package -> the version a pinned root resolved it to.
    named: BTreeMap<String, Version>,
}

impl<'a> Kept<'a> {
    fn of(seed: &Seed<'a>, root: &Manifest, options: &ResolveOptions) -> Self {
        let mut kept = Self::default();
        for requirement in root.installable(options.include_development) {
            let Some(version) = seed.pinned.get(&requirement.name) else {
                continue;
            };
            let package = match &requirement.spec {
                Spec::Alias { package, .. } => package,
                _ => &requirement.name,
            };
            // Two names pinned onto one package at different versions is an
            // alias beside the package itself; the higher is the one other
            // requirements are moved to.
            kept.named
                .entry(package.clone())
                .and_modify(|named| *named = version.clone().max(named.clone()))
                .or_insert_with(|| version.clone());
        }

        let Some(locked) = seed.locked else {
            return kept;
        };
        kept.packages = Some(&locked.packages);
        for id in locked.packages.keys() {
            kept.versions
                .entry(id.name.as_str())
                .or_default()
                .insert(&id.version);
        }
        for requirement in root.installable(options.include_development) {
            let spec = requirement.spec.to_string();
            let recorded = locked
                .requirements
                .iter()
                .find(|record| record.name == requirement.name && record.spec == spec);
            if let Some(RequirementRecord {
                version: Some(version),
                ..
            }) = recorded
            {
                kept.roots
                    .insert((requirement.name.clone(), spec), version.clone());
            }
        }
        kept
    }

    fn package(&self, id: &PackageId) -> Option<&'a ResolvedPackage> {
        self.packages?.get(id)
    }
}

struct Resolver<'a> {
    registry: &'a dyn Registry,
    progress: &'a dyn Progress,
    /// name -> versions chosen so far, highest last.
    selected: BTreeMap<String, BTreeSet<Version>>,
    packages: BTreeMap<PackageId, ResolvedPackage>,
    expanded: BTreeSet<PackageId>,
    skipped: Vec<(String, String)>,
    /// (name, spec) -> the version that root requirement resolved to. Keyed by
    /// spec too, because a package can be both a dependency and a
    /// devDependency at different ranges.
    root_versions: BTreeMap<(String, String), Version>,
    /// package -> the error a fetch ahead of its level came back with, held
    /// until the sequential pass reaches that package.
    refused: BTreeMap<String, RegistryError>,
    kept: Kept<'a>,
    pinned: &'a BTreeMap<String, Version>,
}

/// One unit of work: resolve `name @ spec`, requested by `parent`.
struct Request {
    parent: Option<PackageId>,
    /// What the dependent calls it.
    name: String,
    /// What to actually fetch. Differs from `name` only for an alias.
    package: String,
    spec: Spec,
    optional: bool,
}

impl Request {
    fn new(parent: Option<PackageId>, name: String, spec: Spec, optional: bool) -> Self {
        let package = match &spec {
            Spec::Alias { package, .. } => package.clone(),
            _ => name.clone(),
        };
        Self {
            parent,
            name,
            package,
            spec,
            optional,
        }
    }
}

impl<'a> Resolver<'a> {
    fn run(
        mut self,
        root: &Manifest,
        options: &ResolveOptions,
    ) -> Result<Resolution, ResolveError> {
        let mut level: Vec<Request> = Vec::new();
        let mut requirements = Vec::new();

        for requirement in root.installable(options.include_development) {
            let request = Request::new(
                None,
                requirement.name.clone(),
                requirement.spec.clone(),
                requirement.class.tolerates_absence(),
            );
            requirements.push(RequirementRecord {
                class: requirement.class,
                name: requirement.name.clone(),
                package: request.package.clone(),
                spec: requirement.spec.to_string(),
                version: None,
            });
            level.push(request);
        }
        requirements
            .sort_by(|left, right| (left.class, &left.name).cmp(&(right.class, &right.name)));

        // One level at a time, each in the order it was queued, with the next
        // level collected behind it: the order a single first-in, first-out
        // queue would give.
        let mut known = 0;
        while !level.is_empty() {
            let unseen = self.fetch_ahead(&level, options.concurrent_requests);
            // Each name nothing has selected yet is about to add a package.
            // A second version of a name already selected adds one too, and
            // only shows once it is chosen, hence the `max` in the loop.
            // Held to its own high-water mark so the total never steps back
            // while the count is still climbing.
            known = known.max(self.packages.len() + unseen);

            for request in std::mem::take(&mut level) {
                let selected = self.select(&request)?;
                let settled = self.packages.len();
                self.progress.resolving(settled, known.max(settled));
                let Some(version) = selected else {
                    continue;
                };
                let id = PackageId::new(request.package.clone(), version);

                if request.parent.is_none() {
                    self.root_versions.insert(
                        (request.name.clone(), request.spec.to_string()),
                        id.version.clone(),
                    );
                }

                if let Some(parent) = &request.parent {
                    let edge = ResolvedEdge {
                        name: request.name.clone(),
                        package: id.name.clone(),
                        spec: request.spec.to_string(),
                        version: id.version.clone(),
                        optional: request.optional,
                    };
                    let package = self
                        .packages
                        .get_mut(parent)
                        .expect("a parent is recorded before its dependencies are queued");
                    if !package.dependencies.contains(&edge) {
                        package.dependencies.push(edge);
                        package.dependencies.sort_by(|left, right| {
                            (&left.name, &left.spec).cmp(&(&right.name, &right.spec))
                        });
                    }
                }

                // A package's dependencies are expanded once, however many
                // paths reach it — this is also what terminates dependency
                // cycles.
                if !self.expanded.insert(id.clone()) {
                    continue;
                }
                for requirement in self.dependencies_of(&id)? {
                    level.push(Request::new(
                        Some(id.clone()),
                        requirement.name,
                        requirement.spec,
                        requirement.class.tolerates_absence(),
                    ));
                }
            }
        }

        // The estimate can end above what was found (an optional package the
        // registry doesn't have never arrives), so the last word is the count.
        let total = self.packages.len();
        self.progress.resolving(total, total);

        for requirement in &mut requirements {
            requirement.version = self
                .root_versions
                .get(&(requirement.name.clone(), requirement.spec.clone()))
                .cloned();
        }
        requirements.dedup_by(|left, right| {
            (&left.class, &left.name, &left.spec) == (&right.class, &right.name, &right.spec)
        });

        Ok(Resolution {
            requirements,
            packages: self.packages,
            skipped: self.skipped,
        })
    }

    /// Fetches, several at a time, the packuments this level is about to ask
    /// for one at a time.
    ///
    /// Nothing is decided here. A packument that arrives is kept by the
    /// registry, and an error is kept in `refused`, so the sequential pass
    /// gets the answer it would have got by asking itself, without the wait
    /// and without asking twice.
    ///
    /// Only packages nothing has selected yet are fetched. The first request
    /// for one of those always asks the registry, so this never makes a
    /// request the sequential pass would not have made.
    ///
    /// Returns how many such packages the level names, fetched ahead or not.
    fn fetch_ahead(&mut self, level: &[Request], threads: usize) -> usize {
        let wanted: BTreeSet<&str> = level
            .iter()
            .filter(|request| !matches!(request.spec, Spec::Unsupported(_)))
            .map(|request| request.package.as_str())
            .filter(|package| !self.selected.contains_key(*package))
            .collect();
        let wanted: Vec<&str> = wanted.into_iter().collect();
        let threads = threads.min(wanted.len());
        if threads < 2 {
            return wanted.len();
        }

        let registry = self.registry;
        let Ok(refused) = parallel::each(
            &wanted,
            threads,
            Vec::new,
            |package, refused: &mut Vec<(String, RegistryError)>| {
                if let Err(error) = registry.packument(package) {
                    refused.push(((*package).to_string(), error));
                }
                Ok::<(), Infallible>(())
            },
        );
        self.refused.extend(refused.into_iter().flatten());
        wanted.len()
    }

    fn packument(&mut self, package: &str) -> Result<Arc<Packument>, RegistryError> {
        match self.refused.remove(package) {
            Some(error) => Err(error),
            None => self.registry.packument(package),
        }
    }

    /// Chooses a version, recording the package if it is new.
    fn select(&mut self, request: &Request) -> Result<Option<Version>, ResolveError> {
        let range = match &request.spec {
            Spec::Range(range) => Some(range.clone()),
            // An alias narrows the *target*, so from here it is an ordinary
            // range against an ordinary packument — only the name differs.
            Spec::Alias { range, .. } => Some(range.clone()),
            Spec::Tag(_) => None,
            Spec::Unsupported(spec) => {
                if request.optional {
                    self.skipped.push((
                        request.name.clone(),
                        format!("unsupported specifier {spec:?}"),
                    ));
                    return Ok(None);
                }
                return Err(ResolveError::UnsupportedSpec {
                    name: request.name.clone(),
                    spec: spec.clone(),
                });
            }
        };

        // An answer already given comes before reuse. Reuse would move an
        // edge the lockfile settled onto whichever satisfying version this
        // run selected first, and a lockfile seeded with itself would stop
        // coming back the same.
        let mut packument = None;
        if let Some(version) = self.answered(request, range.as_ref()) {
            let selected = self
                .selected
                .get(&request.package)
                .is_some_and(|versions| versions.contains(&version));
            if selected {
                return Ok(Some(version));
            }
            let Some(offered) = self.packument_for(request)? else {
                return Ok(None);
            };
            // A version the registry has withdrawn is not kept: its edges
            // would read as none, and its dependencies would drop out of the
            // tree without a word. The request is answered afresh instead.
            if let Some(metadata) = offered.version(&version) {
                return Ok(Some(self.record(request, metadata)));
            }
            packument = Some(offered);
        }

        // Reuse before fetching: an already-selected version that satisfies the
        // range keeps the tree flat and the install small.
        if let Some(range) = &range
            && let Some(versions) = self.selected.get(&request.package)
            && let Some(reused) = range.max_satisfying(versions.iter())
        {
            return Ok(Some(reused.clone()));
        }

        let packument = match packument {
            Some(packument) => packument,
            None => match self.packument_for(request)? {
                Some(packument) => packument,
                None => return Ok(None),
            },
        };

        // What reuse would have found had the lockfile's packages been
        // selected already. They are selected level by level like everything
        // else, so a new request can arrive before the version it should
        // share.
        let kept = range.as_ref().and_then(|range| {
            self.kept
                .versions
                .get(request.package.as_str())?
                .iter()
                .rev()
                .filter(|version| range.satisfies(version))
                .find_map(|version| packument.version(version))
        });

        let chosen = match (kept, &range, &request.spec) {
            (Some(kept), _, _) => Some(kept),
            (None, Some(range), _) => pick(&packument, range),
            (None, None, Spec::Tag(tag)) => packument
                .dist_tags
                .get(tag)
                .and_then(|version| packument.version(version)),
            (None, None, _) => None,
        };
        let Some(metadata) = chosen else {
            if request.optional {
                self.skipped.push((
                    request.name.clone(),
                    format!("no version satisfies {}", request.spec),
                ));
                return Ok(None);
            }
            return Err(ResolveError::NoMatchingVersion {
                name: request.name.clone(),
                spec: request.spec.to_string(),
                available: packument.published(),
            });
        };

        Ok(Some(self.record(request, metadata)))
    }

    /// The version this request was already given: by the caller for a root
    /// it resolved itself, or by the lockfile being replaced for a request
    /// that lockfile answered and the manifest still makes.
    fn answered(&self, request: &Request, range: Option<&Range>) -> Option<Version> {
        if request.parent.is_none()
            && let Some(pinned) = self.pinned.get(&request.name)
        {
            return Some(pinned.clone());
        }
        // Ahead of the lockfile's own answer: the package was named to get
        // this version, and a dependent left on the old one would keep a
        // second copy in the tree for no reason its range gives.
        if let Some(range) = range
            && let Some(named) = self.kept.named.get(&request.package)
            && range.satisfies(named)
        {
            return Some(named.clone());
        }
        let spec = request.spec.to_string();
        match &request.parent {
            None => self.kept.roots.get(&(request.name.clone(), spec)).cloned(),
            Some(parent) => self
                .kept
                .package(parent)?
                .dependencies
                .iter()
                .find(|edge| edge.name == request.name && edge.spec == spec)
                .map(|edge| edge.version.clone()),
        }
    }

    /// The packument a request is answered from, or `None` once an optional
    /// dependency the registry does not have is recorded as skipped.
    fn packument_for(&mut self, request: &Request) -> Result<Option<Arc<Packument>>, ResolveError> {
        match self.packument(&request.package) {
            Ok(packument) => Ok(Some(packument)),
            Err(RegistryError::NotFound(name)) if request.optional => {
                self.skipped.push((name, "not in the registry".to_string()));
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn record(&mut self, request: &Request, metadata: VersionMetadata) -> Version {
        let version = metadata.version.clone();
        let id = PackageId::new(request.package.clone(), version.clone());
        let kept = &self.kept;
        self.packages
            .entry(id.clone())
            .or_insert_with(|| match kept.package(&id) {
                // The lockfile's record, not the registry's, so the integrity
                // a download is checked against stays the one that was locked.
                Some(locked) => ResolvedPackage {
                    dependencies: Vec::new(),
                    ..(*locked).clone()
                },
                None => ResolvedPackage {
                    id,
                    tarball: metadata.tarball,
                    integrity: metadata.integrity,
                    dependencies: Vec::new(),
                    os: metadata.manifest.os,
                    cpu: metadata.manifest.cpu,
                },
            });
        self.selected
            .entry(request.package.clone())
            .or_default()
            .insert(version.clone());
        version
    }

    fn dependencies_of(
        &self,
        id: &PackageId,
    ) -> Result<Vec<crate::manifest::Requirement>, ResolveError> {
        let packument = self.registry.packument(&id.name)?;
        let Some(metadata) = packument.version(&id.version) else {
            return Ok(Vec::new());
        };
        Ok(metadata
            .manifest
            .installable(false)
            .cloned()
            .collect::<Vec<_>>())
    }
}

/// The version of `packument` that npm would install for `range`, among those
/// that can be installed at all.
///
/// This is `npm-pick-manifest`'s order without its engines check:
///
/// 1. the `latest` dist-tag, if it satisfies the range and isn't deprecated;
/// 2. otherwise the highest satisfying version that isn't deprecated;
/// 3. otherwise the highest satisfying version.
///
/// Taking `latest` first matters on real trees. A maintainer who publishes
/// without moving the tag hasn't offered that release to ranges yet, and npm
/// won't install it. get-intrinsic 1.3.1 is the live case: it sits past a
/// `latest` of 1.3.0, pulls in three more packages, and is in nearly every
/// express-shaped tree.
///
/// Two of npm's rules are left out on purpose. Engines needs the running
/// Node's version, which `opal-pm` has no business knowing before the runtime
/// exists. For a bare `*`, npm takes `latest` even when `latest` is a
/// prerelease, and that's a version no range allows as opal reads semver.
pub fn pick(packument: &Packument, range: &Range) -> Option<VersionMetadata> {
    if let Some(latest) = packument.dist_tags.get("latest")
        && range.satisfies(latest)
        && let Some(metadata) = packument.version(latest)
        && metadata.deprecated.is_none()
    {
        return Some(metadata);
    }

    // Highest first. A packument lists versions whose bodies can't be
    // installed from (no `dist`, no tarball, no usable integrity), and bodies
    // are parsed on demand, so each candidate costs a parse. Stopping at the
    // first installable version that isn't deprecated keeps that to one parse
    // in the usual case. Only a range whose every version is deprecated walks
    // all of them.
    let mut highest_deprecated = None;
    for candidate in packument
        .versions()
        .rev()
        .filter(|candidate| range.satisfies(candidate))
    {
        let Some(metadata) = packument.version(candidate) else {
            continue;
        };
        if metadata.deprecated.is_none() {
            return Some(metadata);
        }
        highest_deprecated.get_or_insert(metadata);
    }
    highest_deprecated
}

/// Whether a lockfile still describes what `package.json` asks for.
///
/// Always compares against the *whole* manifest, dev dependencies included: the
/// lockfile is the resolution of everything the project declares, and
/// `--production` is a filter applied when the tree is planned. Comparing a
/// dev-filtered manifest against a dev-complete lockfile never matches, which
/// is what made `--production` rewrite the lockfile and `--production
/// --frozen-lockfile` fail against a perfectly good one.
pub fn requirements_match(resolution: &Resolution, manifest: &Manifest) -> bool {
    let mut declared: Vec<(DependencyClass, &str, String)> = manifest
        .installable(true)
        .map(|requirement| {
            (
                requirement.class,
                requirement.name.as_str(),
                requirement.spec.to_string(),
            )
        })
        .collect();
    declared.dedup_by(|left, right| left == right);
    declared.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));

    let recorded: Vec<(DependencyClass, &str, String)> = resolution
        .requirements
        .iter()
        .map(|requirement| {
            (
                requirement.class,
                requirement.name.as_str(),
                requirement.spec.clone(),
            )
        })
        .collect();
    declared == recorded
}

/// A range that was satisfied by reuse rather than by a fresh fetch.
pub fn satisfied_by(range: &Range, version: &Version) -> bool {
    range.satisfies(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(version, deprecated)` pairs, published in that order, with `latest`
    /// pointing wherever it's told to.
    fn packument(versions: &[(&str, bool)], latest: Option<&str>) -> Packument {
        let entries: serde_json::Map<String, serde_json::Value> = versions
            .iter()
            .map(|(version, deprecated)| {
                let mut entry = serde_json::json!({
                    "version": version,
                    "dist": {
                        "tarball": format!("https://example.invalid/demo-{version}.tgz"),
                        "integrity": "sha512-Zm9vYmFy",
                    },
                });
                if *deprecated {
                    entry["deprecated"] = "do not use".into();
                }
                (version.to_string(), entry)
            })
            .collect();
        let mut document = serde_json::json!({ "versions": entries });
        if let Some(latest) = latest {
            document["dist-tags"] = serde_json::json!({ "latest": latest });
        }
        Packument::parse("demo", &serde_json::to_vec(&document).unwrap())
    }

    fn picked(packument: &Packument, range: &str) -> Option<String> {
        pick(packument, &Range::parse(range).unwrap()).map(|metadata| metadata.version.to_string())
    }

    #[test]
    fn test_latest_wins_over_a_newer_untagged_release() {
        let packument = packument(&[("1.3.0", false), ("1.3.1", false)], Some("1.3.0"));
        assert_eq!(picked(&packument, "^1.3.0").as_deref(), Some("1.3.0"));
        // Asked for exactly, the untagged release is still reachable.
        assert_eq!(picked(&packument, "1.3.1").as_deref(), Some("1.3.1"));
    }

    #[test]
    fn test_latest_outside_the_range_falls_back_to_the_highest() {
        let packument = packument(
            &[("1.0.0", false), ("1.2.0", false), ("2.0.0", false)],
            Some("2.0.0"),
        );
        assert_eq!(picked(&packument, "^1.0.0").as_deref(), Some("1.2.0"));
    }

    #[test]
    fn test_a_deprecated_latest_is_passed_over() {
        let packument = packument(
            &[("1.0.0", false), ("1.1.0", false), ("1.2.0", true)],
            Some("1.2.0"),
        );
        assert_eq!(picked(&packument, "^1.0.0").as_deref(), Some("1.1.0"));
    }

    #[test]
    fn test_a_deprecated_version_loses_to_a_lower_one_that_is_not() {
        let packument = packument(
            &[("1.0.0", false), ("1.1.0", true), ("2.0.0", false)],
            Some("2.0.0"),
        );
        assert_eq!(picked(&packument, "^1.0.0").as_deref(), Some("1.0.0"));
    }

    #[test]
    fn test_when_everything_is_deprecated_the_highest_still_installs() {
        // A whole package deprecated (`request`, `har-validator`) is still
        // installable, and npm installs it.
        let packument = packument(&[("2.0.0", true), ("2.1.0", true)], Some("2.1.0"));
        assert_eq!(picked(&packument, "^2.0.0").as_deref(), Some("2.1.0"));
    }

    #[test]
    fn test_a_prerelease_latest_only_counts_where_the_range_allows_prereleases() {
        let packument = packument(
            &[("1.0.0", false), ("2.0.0-rc.1", false)],
            Some("2.0.0-rc.1"),
        );
        assert_eq!(picked(&packument, "*").as_deref(), Some("1.0.0"));
        assert_eq!(
            picked(&packument, "^2.0.0-rc.0").as_deref(),
            Some("2.0.0-rc.1")
        );
    }

    #[test]
    fn test_no_latest_tag_means_the_highest() {
        let packument = packument(&[("1.0.0", false), ("1.1.0", false)], None);
        assert_eq!(picked(&packument, "^1.0.0").as_deref(), Some("1.1.0"));
    }

    #[test]
    fn test_an_uninstallable_latest_is_skipped_not_fatal() {
        let document = serde_json::json!({
            "dist-tags": { "latest": "1.1.0" },
            "versions": {
                "1.0.0": { "dist": { "tarball": "https://example.invalid/a.tgz", "integrity": "sha512-Zm9vYmFy" } },
                "1.1.0": { "version": "1.1.0" },
            }
        });
        let packument = Packument::parse("demo", &serde_json::to_vec(&document).unwrap());
        assert_eq!(picked(&packument, "^1.0.0").as_deref(), Some("1.0.0"));
    }

    #[test]
    fn test_nothing_satisfying_picks_nothing() {
        let packument = packument(&[("1.0.0", true)], Some("1.0.0"));
        assert_eq!(picked(&packument, "^2.0.0"), None);
    }
}
