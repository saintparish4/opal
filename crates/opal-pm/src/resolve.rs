//! Semver resolution: `package.json` plus the registry, into a resolved graph.
//!
//! The algorithm is npm's in spirit: walk requirements breadth-first, and for
//! each one reuse an already-selected version if it satisfies, otherwise take
//! the highest version the range allows. Reuse is what keeps the tree small;
//! the layout planner is what copes with the conflicts reuse cannot avoid.
//!
//! Determinism is a requirement, not a nicety — a lockfile that differs between
//! two runs over the same inputs is a lockfile nobody can review. Every
//! collection here is ordered, and the work queue is drained in sorted order.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use crate::integrity::Integrity;
use crate::manifest::{DependencyClass, Manifest, Spec};
use crate::registry::{Registry, RegistryError};
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
    pub name: String,
    pub spec: String,
    pub version: Version,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedPackage {
    pub id: PackageId,
    pub tarball: String,
    pub integrity: Integrity,
    /// Sorted by dependency name.
    pub dependencies: Vec<ResolvedEdge>,
}

/// What the root project asked for, kept so a `package.json` edit can be
/// detected without re-resolving.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RequirementRecord {
    pub class: DependencyClass,
    pub name: String,
    pub spec: String,
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

    /// Root-level edges, resolved.
    pub fn roots(&self) -> Vec<PackageId> {
        let mut roots: Vec<PackageId> = self
            .requirements
            .iter()
            .filter_map(|requirement| {
                self.packages
                    .keys()
                    .filter(|id| id.name == requirement.name)
                    .max()
                    .cloned()
            })
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }
}

#[derive(Clone, Debug)]
pub struct ResolveOptions {
    /// The root project's `devDependencies` are installed; a dependency's are
    /// never installed, which is what keeps a tree from exploding.
    pub include_development: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            include_development: true,
        }
    }
}

pub fn resolve(
    registry: &dyn Registry,
    root: &Manifest,
    options: &ResolveOptions,
) -> Result<Resolution, ResolveError> {
    Resolver {
        registry,
        selected: BTreeMap::new(),
        packages: BTreeMap::new(),
        expanded: BTreeSet::new(),
        skipped: Vec::new(),
    }
    .run(root, options)
}

struct Resolver<'a> {
    registry: &'a dyn Registry,
    /// name -> versions chosen so far, highest last.
    selected: BTreeMap<String, BTreeSet<Version>>,
    packages: BTreeMap<PackageId, ResolvedPackage>,
    expanded: BTreeSet<PackageId>,
    skipped: Vec<(String, String)>,
}

/// One unit of work: resolve `name @ spec`, requested by `parent`.
struct Request {
    parent: Option<PackageId>,
    name: String,
    spec: Spec,
    optional: bool,
}

impl<'a> Resolver<'a> {
    fn run(
        mut self,
        root: &Manifest,
        options: &ResolveOptions,
    ) -> Result<Resolution, ResolveError> {
        let mut queue: VecDeque<Request> = VecDeque::new();
        let mut requirements = Vec::new();

        for requirement in root.installable(options.include_development) {
            requirements.push(RequirementRecord {
                class: requirement.class,
                name: requirement.name.clone(),
                spec: requirement.spec.to_string(),
            });
            queue.push_back(Request {
                parent: None,
                name: requirement.name.clone(),
                spec: requirement.spec.clone(),
                optional: requirement.class.tolerates_absence(),
            });
        }
        requirements
            .sort_by(|left, right| (left.class, &left.name).cmp(&(right.class, &right.name)));

        while let Some(request) = queue.pop_front() {
            let Some(version) = self.select(&request)? else {
                continue;
            };
            let id = PackageId::new(request.name.clone(), version);

            if let Some(parent) = &request.parent {
                let edge = ResolvedEdge {
                    name: id.name.clone(),
                    spec: request.spec.to_string(),
                    version: id.version.clone(),
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

            // A package's dependencies are expanded once, however many paths
            // reach it — this is also what terminates dependency cycles.
            if !self.expanded.insert(id.clone()) {
                continue;
            }
            for requirement in self.dependencies_of(&id)? {
                queue.push_back(Request {
                    parent: Some(id.clone()),
                    optional: requirement.class.tolerates_absence(),
                    name: requirement.name,
                    spec: requirement.spec,
                });
            }
        }

        Ok(Resolution {
            requirements,
            packages: self.packages,
            skipped: self.skipped,
        })
    }

    /// Chooses a version, recording the package if it is new.
    fn select(&mut self, request: &Request) -> Result<Option<Version>, ResolveError> {
        let range = match &request.spec {
            Spec::Range(range) => Some(range.clone()),
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

        // Reuse before fetching: an already-selected version that satisfies the
        // range keeps the tree flat and the install small.
        if let Some(range) = &range
            && let Some(versions) = self.selected.get(&request.name)
            && let Some(reused) = range.max_satisfying(versions.iter())
        {
            return Ok(Some(reused.clone()));
        }

        let packument = match self.registry.packument(&request.name) {
            Ok(packument) => packument,
            Err(RegistryError::NotFound(name)) if request.optional => {
                self.skipped.push((name, "not in the registry".to_string()));
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };

        let chosen = match (&range, &request.spec) {
            (Some(range), _) => range.max_satisfying(packument.versions.keys()).cloned(),
            (None, Spec::Tag(tag)) => packument.dist_tags.get(tag).cloned(),
            (None, _) => None,
        };
        let Some(version) = chosen else {
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
                available: packument.versions.len(),
            });
        };

        let id = PackageId::new(request.name.clone(), version.clone());
        self.packages.entry(id.clone()).or_insert_with(|| {
            let metadata = packument
                .version(&version)
                .expect("the chosen version came from this packument");
            ResolvedPackage {
                id,
                tarball: metadata.tarball.clone(),
                integrity: metadata.integrity.clone(),
                dependencies: Vec::new(),
            }
        });
        self.selected
            .entry(request.name.clone())
            .or_default()
            .insert(version.clone());
        Ok(Some(version))
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

/// Whether a lockfile still describes what `package.json` asks for.
pub fn requirements_match(
    resolution: &Resolution,
    manifest: &Manifest,
    include_development: bool,
) -> bool {
    let declared: Vec<RequirementRecord> = {
        let mut declared: Vec<RequirementRecord> = manifest
            .installable(include_development)
            .map(|requirement| RequirementRecord {
                class: requirement.class,
                name: requirement.name.clone(),
                spec: requirement.spec.to_string(),
            })
            .collect();
        declared.sort_by(|left, right| (left.class, &left.name).cmp(&(right.class, &right.name)));
        declared
    };
    declared == resolution.requirements
}

/// A range that was satisfied by reuse rather than by a fresh fetch.
pub fn satisfied_by(range: &Range, version: &Version) -> bool {
    range.satisfies(version)
}
