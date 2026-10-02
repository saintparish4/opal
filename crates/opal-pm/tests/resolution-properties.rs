//! Property tests for the resolver, over generated registries.
//!
//! `semver-properties.rs` covers the range algebra in isolation. This covers
//! the thing built on top of it: given a whole registry and a manifest, does
//! every version the resolver picks actually satisfy the range that asked for
//! it? That is the invariant a silent wrong-version install breaks, and it is
//! not implied by a correct `max_satisfying` — the resolver *reuses* an
//! already-selected version whenever one satisfies, and a reuse rule that is
//! slightly too eager produces exactly this bug while every semver unit test
//! still passes.
//!
//! The registry here is in-memory, but it is not a stub of the thing under
//! test: packuments are built as JSON and handed to the real
//! `Packument::parse`, so generated input travels the same path a registry
//! response does. Only the transport is replaced, and resolution has no
//! business touching it — `tarball` is `unreachable!` for that reason.
//!
//! Generated plans routinely contain dependency cycles, since an edge may name
//! any package including its own. Nothing special handles them; that these
//! tests terminate at all is the evidence, with two explicit cases at the
//! bottom to say so out loud.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use opal_pm::link::{self, PlanOptions};
use opal_pm::manifest::Manifest;
use opal_pm::platform::Platform;
use opal_pm::registry::{Packument, Registry, RegistryError};
use opal_pm::resolve::{self, PackageId, Resolution, ResolveError, ResolveOptions};
use opal_pm::semver::{Range, Version};
use proptest::prelude::*;
use serde_json::json;

/// Real enough for `Packument::parse`; nothing here ever downloads.
const INTEGRITY: &str = "sha512-Zm9vYmFy";

/// A package no plan ever publishes, so an edge naming it is a 404.
const ABSENT: &str = "pkg-absent";

/// One generated dependency: which package it names, which of that package's
/// published versions the range is built around, which range shape to build,
/// and whether it is declared optionally.
#[derive(Clone, Copy, Debug)]
struct Edge {
    package: usize,
    version: usize,
    shape: u8,
    optional: bool,
}

#[derive(Clone, Debug)]
struct Plan {
    /// The versions each package publishes, sorted and deduplicated.
    versions: Vec<Vec<Version>>,
    /// What each published version declares, in the order `versions` flattens.
    dependencies: Vec<Vec<Edge>>,
    /// Which published version each package's `latest` tag points at, as an
    /// index. Not always the highest: a release published without moving the
    /// tag is the case the resolver has to pass over.
    latest: Vec<usize>,
    /// Whether each published version is deprecated, flattened like
    /// `dependencies`.
    deprecated: Vec<bool>,
    root: Vec<Edge>,
}

struct Universe {
    packuments: BTreeMap<String, Arc<Packument>>,
    root: Manifest,
    /// Every name some manifest declared as an `optionalDependency`.
    optional: BTreeSet<String>,
}

impl Registry for Universe {
    fn packument(&self, name: &str) -> Result<Arc<Packument>, RegistryError> {
        self.packuments
            .get(name)
            .map(Arc::clone)
            .ok_or_else(|| RegistryError::NotFound(name.to_string()))
    }

    fn tarball(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        unreachable!("resolution downloaded {url}, which it must never do");
    }
}

impl Universe {
    /// `None` when the plan asks for something that genuinely does not exist,
    /// which is a legitimate outcome rather than a property violation. Any
    /// other error means the generator drifted away from what it claims to
    /// produce, so it fails loudly instead of being skipped.
    fn resolution(&self) -> Option<Resolution> {
        match resolve::resolve(self, &self.root, &ResolveOptions::default()) {
            Ok(resolution) => Some(resolution),
            Err(ResolveError::NoMatchingVersion { .. } | ResolveError::Registry(_)) => None,
            Err(error) => panic!("unexpected resolve failure: {error}"),
        }
    }
}

fn name(package: usize) -> String {
    format!("pkg-{package}")
}

impl Plan {
    fn build(&self) -> Universe {
        let mut packuments = BTreeMap::new();
        let mut optional = BTreeSet::new();
        let mut published = 0;

        for (package, versions) in self.versions.iter().enumerate() {
            let mut entries = serde_json::Map::new();
            for version in versions {
                let edges = self
                    .dependencies
                    .get(published)
                    .map_or(&[][..], Vec::as_slice);
                let deprecated = self.deprecated.get(published).copied().unwrap_or(false);
                published += 1;
                let (required, tolerated) = self.declare(edges, &mut optional);
                let mut entry = json!({
                    "name": name(package),
                    "version": version.to_string(),
                    "dependencies": required,
                    "optionalDependencies": tolerated,
                    "dist": {
                        "tarball": format!("file:///{}-{version}.tgz", name(package)),
                        "integrity": INTEGRITY,
                    },
                });
                if deprecated {
                    entry["deprecated"] = json!("generated");
                }
                entries.insert(version.to_string(), entry);
            }

            let latest = &versions[self.latest.get(package).copied().unwrap_or(0) % versions.len()];
            let document = json!({
                "name": name(package),
                "dist-tags": { "latest": latest.to_string() },
                "versions": entries,
            });
            packuments.insert(
                name(package),
                Arc::new(Packument::parse(
                    &name(package),
                    &serde_json::to_vec(&document).expect("serializable"),
                )),
            );
        }

        let (required, tolerated) = self.declare(&self.root, &mut optional);
        let root = Manifest::from_value(&json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": required,
            "optionalDependencies": tolerated,
        }));

        Universe {
            packuments,
            root,
            optional,
        }
    }

    /// Splits one manifest's edges into its two dependency fields, and records
    /// what it tolerated the absence of. A name declared both ways *within a
    /// manifest* stays required there, but that says nothing about how another
    /// manifest declares it — which is why the accumulator only ever grows,
    /// from what actually survived as optional here.
    fn declare(
        &self,
        edges: &[Edge],
        optional: &mut BTreeSet<String>,
    ) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let mut required = BTreeMap::new();
        let mut tolerated = BTreeMap::new();

        for edge in edges {
            let (name, spec) = self.specifier(*edge);
            if edge.optional {
                tolerated.insert(name, spec);
            } else {
                tolerated.remove(&name);
                required.insert(name, spec);
            }
        }
        optional.extend(tolerated.keys().cloned());
        (required, tolerated)
    }

    fn specifier(&self, edge: Edge) -> (String, String) {
        if edge.shape == 7 {
            return (ABSENT.to_string(), "^1.0.0".to_string());
        }
        let package = edge.package % self.versions.len();
        let versions = &self.versions[package];
        let target = &versions[edge.version % versions.len()];

        let spec = match edge.shape {
            0 => format!("^{target}"),
            1 => format!("~{target}"),
            2 => target.to_string(),
            3 => format!(">={target}"),
            4 => "*".to_string(),
            5 => format!(">={target} <{}.0.0", target.major + 1),
            // Nothing is published above 2.x, so this is the unsatisfiable
            // case: an error for a dependency, a skip for an optional one.
            _ => "^99.0.0".to_string(),
        };
        (name(package), spec)
    }

    /// The same plan with nothing required, which is the shape that must never
    /// fail to resolve.
    fn all_optional(&self) -> Self {
        let optional = |edges: &Vec<Edge>| {
            edges
                .iter()
                .map(|edge| Edge {
                    optional: true,
                    ..*edge
                })
                .collect()
        };
        Self {
            versions: self.versions.clone(),
            dependencies: self.dependencies.iter().map(optional).collect(),
            latest: self.latest.clone(),
            deprecated: self.deprecated.clone(),
            root: optional(&self.root),
        }
    }
}

fn published_version() -> impl Strategy<Value = Version> {
    (
        0u64..3,
        0u64..3,
        0u64..3,
        prop::option::weighted(0.2, prop::sample::select(vec!["alpha", "rc.1"])),
    )
        .prop_map(|(major, minor, patch, tag)| {
            let text = match tag {
                Some(tag) => format!("{major}.{minor}.{patch}-{tag}"),
                None => format!("{major}.{minor}.{patch}"),
            };
            Version::parse(&text).expect("the generator only produces well-formed versions")
        })
}

fn plan() -> impl Strategy<Value = Plan> {
    prop::collection::vec(prop::collection::vec(published_version(), 1..=4), 1..=5)
        .prop_map(|mut versions| {
            // `Edge::version` indexes into this, so it has to be settled before
            // any edge is generated against it.
            for published in &mut versions {
                published.sort();
                published.dedup();
            }
            versions
        })
        .prop_flat_map(|versions| {
            let packages = versions.len();
            let published: usize = versions.iter().map(Vec::len).sum();
            let edge = (0..packages, 0..4usize, 0u8..8, any::<bool>()).prop_map(
                |(package, version, shape, optional)| Edge {
                    package,
                    version,
                    shape,
                    optional,
                },
            );
            (
                Just(versions),
                prop::collection::vec(prop::collection::vec(edge.clone(), 0..=3), published),
                prop::collection::vec(0..4usize, packages),
                prop::collection::vec(prop::bool::weighted(0.3), published),
                prop::collection::vec(edge, 1..=3),
            )
        })
        .prop_map(|(versions, dependencies, latest, deprecated, root)| Plan {
            versions,
            dependencies,
            latest,
            deprecated,
            root,
        })
}

proptest! {
    /// The one that matters: nothing is installed at a version the manifest
    /// that asked for it does not allow.
    #[test]
    fn test_every_resolved_edge_satisfies_the_range_that_requested_it(plan in plan()) {
        let universe = plan.build();
        let Some(resolution) = universe.resolution() else { return Ok(()); };

        for package in resolution.packages.values() {
            for edge in &package.dependencies {
                let range = Range::parse(&edge.spec).expect("every generated spec is a range");
                prop_assert!(
                    range.satisfies(&edge.version),
                    "{} asked for {}@{} and got {}",
                    package.id, edge.name, edge.spec, edge.version
                );
            }
        }
    }

    /// A resolution that names a package it did not resolve is a lockfile the
    /// linker cannot materialize.
    #[test]
    fn test_the_resolution_is_closed_over_everything_it_names(plan in plan()) {
        let universe = plan.build();
        let Some(resolution) = universe.resolution() else { return Ok(()); };

        for package in resolution.packages.values() {
            for edge in &package.dependencies {
                let id = PackageId::new(edge.name.clone(), edge.version.clone());
                prop_assert!(resolution.package(&id).is_some(), "{id} is named but not resolved");
            }
        }
        for root in resolution.roots() {
            prop_assert!(
                resolution.package(&root.id).is_some(),
                "root {} is not resolved",
                root.id
            );
        }
    }

    #[test]
    fn test_every_root_requirement_is_met_or_recorded_as_skipped(plan in plan()) {
        let universe = plan.build();
        let Some(resolution) = universe.resolution() else { return Ok(()); };

        for requirement in &resolution.requirements {
            let range = Range::parse(&requirement.spec).expect("every generated spec is a range");
            let met = resolution
                .packages
                .keys()
                .any(|id| id.name == requirement.name && range.satisfies(&id.version));
            let skipped = resolution
                .skipped
                .iter()
                .any(|(name, _)| *name == requirement.name);

            prop_assert!(
                met || skipped,
                "{}@{} is neither resolved nor skipped",
                requirement.name, requirement.spec
            );
        }
    }

    /// Absence is only ever tolerated where the manifest said it could be.
    /// The invariant `test_every_root_requirement_is_met_or_recorded_as_skipped`
    /// is one step short of: *some* resolved version satisfying the root's
    /// range is not the same as the root resolving *to* one. Deriving the root
    /// by version order satisfies the weaker property and still hoists a
    /// transitive dependency's higher major into the project's own slot.
    #[test]
    fn test_every_root_resolves_to_a_version_its_own_spec_allows(plan in plan()) {
        let universe = plan.build();
        let Some(resolution) = universe.resolution() else { return Ok(()); };

        for requirement in &resolution.requirements {
            let Some(version) = &requirement.version else { continue };
            let range = Range::parse(&requirement.spec).expect("every generated spec is a range");
            prop_assert!(
                range.satisfies(version),
                "root {}@{} resolved to {}",
                requirement.name, requirement.spec, version
            );
        }

        for root in resolution.roots() {
            let allowed = resolution
                .requirements
                .iter()
                .filter(|requirement| requirement.name == root.name)
                .any(|requirement| {
                    Range::parse(&requirement.spec)
                        .expect("every generated spec is a range")
                        .satisfies(&root.id.version)
                });
            prop_assert!(allowed, "{} is a root no root requirement allows", root.id);
        }
    }

    /// A resolved package the layout never places is one that was downloaded
    /// and thrown away — and, if it was the version a root asked for, one the
    /// project cannot import.
    #[test]
    fn test_the_layout_places_every_package_the_resolution_keeps(plan in plan()) {
        let universe = plan.build();
        let Some(resolution) = universe.resolution() else { return Ok(()); };

        let planned = link::plan(&resolution, &PlanOptions {
            platform: Platform::new("linux", "x64"),
            include_development: true,
        });
        prop_assert!(planned.platform_skipped.is_empty(), "no generated package constrains a platform");

        let placed: BTreeSet<&PackageId> = planned.layout.values().collect();
        for id in resolution.packages.keys() {
            prop_assert!(placed.contains(id), "{id} was resolved but never placed");
        }
    }

    #[test]
    fn test_only_optional_dependencies_are_ever_skipped(plan in plan()) {
        let universe = plan.build();
        let Some(resolution) = universe.resolution() else { return Ok(()); };

        for (name, reason) in &resolution.skipped {
            prop_assert!(
                universe.optional.contains(name),
                "{name} was skipped ({reason}) but nothing declared it optional"
            );
        }
    }

    /// Nothing an optional dependency can do should fail an install — not a
    /// missing package, not an unsatisfiable range, not either one reached
    /// through another optional package.
    #[test]
    fn test_a_plan_of_only_optional_dependencies_always_resolves(plan in plan()) {
        let universe = plan.all_optional().build();
        let resolved = resolve::resolve(&universe, &universe.root, &ResolveOptions::default());
        prop_assert!(resolved.is_ok(), "{:?}", resolved.err().map(|error| error.to_string()));
    }

    /// A lockfile that differs between two runs over the same inputs is a
    /// lockfile nobody can review.
    #[test]
    fn test_resolving_the_same_registry_twice_gives_the_same_answer(plan in plan()) {
        prop_assert_eq!(plan.build().resolution(), plan.build().resolution());
    }

    /// `pick`, stated the way npm-pick-manifest states it: sort the satisfying
    /// versions by (is `latest` and not deprecated, is not deprecated,
    /// version) and take the top. `pick` gets there by walking down and
    /// stopping early, which is where an off-by-one in the order would hide.
    #[test]
    fn test_pick_takes_latest_then_the_undeprecated_then_the_highest(
        plan in plan(),
        package in 0..5usize,
        version in 0..4usize,
        shape in 0u8..7,
    ) {
        let universe = plan.build();
        let (name, spec) = plan.specifier(Edge { package, version, shape, optional: false });
        let packument = &universe.packuments[&name];
        let range = Range::parse(&spec).expect("every generated spec is a range");
        let latest = packument.dist_tags.get("latest");

        let expected = packument
            .versions()
            .filter(|candidate| range.satisfies(candidate))
            .filter_map(|candidate| packument.version(candidate))
            .max_by_key(|metadata| {
                let undeprecated = metadata.deprecated.is_none();
                (
                    undeprecated && Some(&metadata.version) == latest,
                    undeprecated,
                    metadata.version.clone(),
                )
            })
            .map(|metadata| metadata.version);
        let picked = resolve::pick(packument, &range).map(|metadata| metadata.version);
        prop_assert_eq!(picked, expected, "{}@{} with latest {:?}", name, spec, latest);
    }
}

/// Two packages that depend on each other. npm publishes these, and the
/// resolver has to terminate on them rather than recurse forever.
#[test]
fn test_a_dependency_cycle_terminates() {
    let edge = |package| Edge {
        package,
        version: 0,
        shape: 0,
        optional: false,
    };
    let plan = Plan {
        versions: vec![vec![Version::new(1, 0, 0)], vec![Version::new(1, 0, 0)]],
        dependencies: vec![vec![edge(1)], vec![edge(0)]],
        latest: vec![0, 0],
        deprecated: vec![false, false],
        root: vec![edge(0)],
    };

    let resolution = plan.build().resolution().expect("a cycle still resolves");
    assert_eq!(resolution.packages.len(), 2);
}

#[test]
fn test_a_package_that_depends_on_itself_terminates() {
    let edge = Edge {
        package: 0,
        version: 0,
        shape: 0,
        optional: false,
    };
    let plan = Plan {
        versions: vec![vec![Version::new(1, 0, 0)]],
        dependencies: vec![vec![edge]],
        latest: vec![0],
        deprecated: vec![false],
        root: vec![edge],
    };

    let resolution = plan
        .build()
        .resolution()
        .expect("a self-cycle still resolves");
    assert_eq!(resolution.packages.len(), 1);
}
