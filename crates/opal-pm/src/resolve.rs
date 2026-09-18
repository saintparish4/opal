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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use crate::integrity::Integrity;
use crate::manifest::{DependencyClass, Manifest, Spec};
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
        root_versions: BTreeMap::new(),
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
    /// (name, spec) -> the version that root requirement resolved to. Keyed by
    /// spec too, because a package can be both a dependency and a
    /// devDependency at different ranges.
    root_versions: BTreeMap<(String, String), Version>,
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
        let mut queue: VecDeque<Request> = VecDeque::new();
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
            queue.push_back(request);
        }
        requirements
            .sort_by(|left, right| (left.class, &left.name).cmp(&(right.class, &right.name)));

        while let Some(request) = queue.pop_front() {
            let Some(version) = self.select(&request)? else {
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

            // A package's dependencies are expanded once, however many paths
            // reach it — this is also what terminates dependency cycles.
            if !self.expanded.insert(id.clone()) {
                continue;
            }
            for requirement in self.dependencies_of(&id)? {
                queue.push_back(Request::new(
                    Some(id.clone()),
                    requirement.name,
                    requirement.spec,
                    requirement.class.tolerates_absence(),
                ));
            }
        }

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

        // Reuse before fetching: an already-selected version that satisfies the
        // range keeps the tree flat and the install small.
        if let Some(range) = &range
            && let Some(versions) = self.selected.get(&request.package)
            && let Some(reused) = range.max_satisfying(versions.iter())
        {
            return Ok(Some(reused.clone()));
        }

        let packument = match self.registry.packument(&request.package) {
            Ok(packument) => packument,
            Err(RegistryError::NotFound(name)) if request.optional => {
                self.skipped.push((name, "not in the registry".to_string()));
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };

        let chosen = match (&range, &request.spec) {
            (Some(range), _) => pick(&packument, range),
            (None, Spec::Tag(tag)) => packument
                .dist_tags
                .get(tag)
                .and_then(|version| packument.version(version)),
            (None, _) => None,
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

        let version = metadata.version.clone();
        let id = PackageId::new(request.package.clone(), version.clone());
        self.packages
            .entry(id.clone())
            .or_insert_with(|| ResolvedPackage {
                id,
                tarball: metadata.tarball,
                integrity: metadata.integrity,
                dependencies: Vec::new(),
                os: metadata.manifest.os,
                cpu: metadata.manifest.cpu,
            });
        self.selected
            .entry(request.package.clone())
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
