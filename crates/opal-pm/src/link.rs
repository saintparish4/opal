//! Planning a `node_modules` tree, and reconciling the one on disk with it.
//!
//! **Layout decision** (`build_guide.md` Phase 1.4 asks for this to be explicit):
//! Opal writes a *hoisted* `node_modules` — the npm shape, not pnpm's — built
//! entirely from hardlinks, with symlinks used only for `.bin` shims. Each
//! package is placed as shallowly as it can go; a version conflict nests under
//! the package that needs it.
//!
//! The trade against pnpm's symlinked store layout is deliberate. Hoisting
//! allows phantom dependencies (code can `require` a package it never declared)
//! which pnpm's layout prevents. In exchange, v1 gets a tree that every Node
//! version and every tool understands, with no symlink in the dependency path —
//! which matters because native Windows (v2) needs elevated permissions or
//! Developer Mode for symlinks. Revisit when v2 lands.
//!
//! **Reconciler, not a sequence.** Every package directory carries a
//! `.opal-package` marker naming what it holds, written *last*. A package whose
//! files are present but whose marker is missing is therefore indistinguishable
//! from one that was never installed — which is exactly right, because a killed
//! run leaves precisely that. `reconcile` diffs the plan against the markers and
//! applies only the difference, so re-running converges with no resume logic.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use opal_core::atomic::write_atomic;
use opal_core::cas::{Cas, CasError};
use opal_core::fault::{self, FaultPoint};
use opal_core::hash::ContentHash;
use opal_core::path::NormalizedPath;

use crate::manifest::Manifest;
use crate::package::{PackageError, PackageIndex};
use crate::platform::Platform;
use crate::resolve::{PackageId, Resolution};

/// Some of a package's files are linked; its marker is not written.
pub const FAULT_MID_LINK: FaultPoint = FaultPoint::new("pm-mid-link");
/// One package is fully materialized; the next has not started.
pub const FAULT_BETWEEN_PACKAGES: FaultPoint = FaultPoint::new("pm-between-packages");

/// Written last in a package directory; its presence means "complete".
pub const MARKER_FILE: &str = ".opal-package";
pub const NODE_MODULES: &str = "node_modules";
pub const BIN_DIR: &str = ".bin";

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Cas(#[from] CasError),
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error("{id} is in the layout but its contents were never fetched")]
    NotFetched { id: PackageId },
}

impl LinkError {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Package directories, relative to the project root, and what belongs in each.
pub type Layout = BTreeMap<NormalizedPath, PackageId>;

/// A package's contents, ready to be materialized.
#[derive(Clone, Debug)]
pub struct FetchedPackage {
    pub index_hash: ContentHash,
    pub index: PackageIndex,
}

pub type Fetched = BTreeMap<PackageId, FetchedPackage>;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LinkReport {
    pub added: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub files_linked: usize,
    pub files_copied: usize,
    pub bins: usize,
    /// Set once, when the filesystem turns out not to support hardlinking the
    /// store into the project at all. Reported rather than printed: `opal-pm`
    /// is a library, and the caller decides whether a terminal hears about it.
    pub hardlink_fallback: Option<String>,
}

/// What a failed `hard_link` means for the rest of the run.
enum LinkFailure {
    /// Cross-device: the store and the project are on different filesystems,
    /// so no file will ever link. A project under `/mnt/c` on WSL2 — a v1
    /// target — is exactly this, and retrying it 20,000 times is 20,000
    /// syscalls spent proving the same thing.
    Impossible(String),
    /// This file, this time: the inode is at its link limit, or permissions
    /// disagree. The next file is worth attempting.
    ThisFile,
}

fn classify_link_failure(error: &std::io::Error) -> LinkFailure {
    match error.raw_os_error() {
        Some(libc::EXDEV) => LinkFailure::Impossible(
            "the store and this project are on different filesystems, so every file was copied              instead of hardlinked"
                .to_string(),
        ),
        // Some filesystems (and some FUSE mounts) refuse hardlinks outright
        // rather than reporting a cross-device link.
        Some(libc::EPERM) | Some(libc::ENOSYS) => LinkFailure::Impossible(
            "this filesystem does not permit hardlinks, so every file was copied instead"
                .to_string(),
        ),
        _ => LinkFailure::ThisFile,
    }
}

#[derive(Clone, Debug)]
pub struct PlanOptions {
    /// The host a tree is being planned for. A value rather than a `cfg!` so
    /// the lockfile stays platform-independent and a test can plan for a host
    /// it is not running on.
    pub platform: Platform,
    /// Whether the project's own `devDependencies` are part of this tree.
    pub include_development: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            platform: Platform::host(),
            include_development: true,
        }
    }
}

/// A planned tree, and what was left out of it.
#[derive(Debug, Default)]
pub struct Plan {
    pub layout: Layout,
    /// Packages this host cannot run, with the constraint that excluded them.
    /// Reported rather than printed: `opal-pm` is a library and has no
    /// business knowing about a terminal.
    pub platform_skipped: Vec<(PackageId, String)>,
}

/// Places every resolved package this host can run, hoisting as far as each
/// one can go.
pub fn plan(resolution: &Resolution, options: &PlanOptions) -> Plan {
    let root = NormalizedPath::new(".");
    let mut plan = Plan::default();
    let mut visited: BTreeSet<(NormalizedPath, PackageId)> = BTreeSet::new();
    let mut queue: VecDeque<(NormalizedPath, PackageId)> = resolution
        .roots_for(options.include_development)
        .into_iter()
        .map(|id| (root.clone(), id))
        .collect();

    while let Some((owner, id)) = queue.pop_front() {
        // A package's dependencies are expanded once per placement, which also
        // terminates dependency cycles.
        if !visited.insert((owner.clone(), id.clone())) {
            continue;
        }

        let package = resolution.package(&id);
        // A package that cannot run here is not placed, and neither is
        // anything it alone depends on — that subtree is unreachable now.
        if let Some(package) = package
            && !options.platform.supports(&package.os, &package.cpu)
        {
            let reason = options.platform.rejection(&package.os, &package.cpu);
            if !plan
                .platform_skipped
                .iter()
                .any(|(skipped, _)| *skipped == id)
            {
                plan.platform_skipped.push((id.clone(), reason));
            }
            continue;
        }

        let directory = placement(&owner, &id, &plan.layout);
        plan.layout.insert(directory.clone(), id.clone());

        let Some(package) = package else {
            continue;
        };
        for edge in &package.dependencies {
            queue.push_back((
                directory.clone(),
                PackageId::new(edge.name.clone(), edge.version.clone()),
            ));
        }
    }
    plan.platform_skipped.sort();
    plan
}

/// The shallowest directory whose `node_modules` slot for this name is free, or
/// already holds this exact version.
fn placement(owner: &NormalizedPath, id: &PackageId, layout: &Layout) -> NormalizedPath {
    for candidate in owning_directories(owner) {
        let slot = slot_for(&candidate, &id.name);
        match layout.get(&slot) {
            None => return slot,
            Some(existing) if existing == id => return slot,
            // Taken by another version: try one level deeper.
            Some(_) => {}
        }
    }
    slot_for(owner, &id.name)
}

fn slot_for(directory: &NormalizedPath, name: &str) -> NormalizedPath {
    directory.join(NODE_MODULES).join(name)
}

/// Every directory that could host this package, from the project root down to
/// the package that depends on it.
fn owning_directories(owner: &NormalizedPath) -> Vec<NormalizedPath> {
    let mut chain = vec![owner.clone()];
    let mut current = owner.clone();
    // Package directories look like `…/node_modules/<name>`; the owner above one
    // is whatever sits before the last `node_modules` segment.
    while let Some(parent) = strip_last_package(&current) {
        chain.push(parent.clone());
        current = parent;
    }
    chain.reverse();
    chain
}

fn strip_last_package(path: &NormalizedPath) -> Option<NormalizedPath> {
    let text = path.as_str();
    let marker = format!("{NODE_MODULES}/");
    let index = text.rfind(&marker)?;
    Some(NormalizedPath::new(&text[..index]))
}

/// Diffs the planned tree against what is on disk and applies the difference.
pub fn reconcile(
    project_root: &Path,
    layout: &Layout,
    fetched: &Fetched,
    cas: &Cas,
) -> Result<LinkReport, LinkError> {
    let mut report = LinkReport::default();
    let actual = scan(project_root)?;

    let mut stale: Vec<NormalizedPath> = Vec::new();
    for (path, marker) in &actual {
        let wanted = layout.get(path).and_then(|id| {
            fetched
                .get(id)
                .map(|package| (id.clone(), package.index_hash))
        });
        match wanted {
            Some((id, index_hash))
                if id.name == marker.name
                    && id.version.to_string() == marker.version
                    && index_hash == marker.index_hash => {}
            _ => stale.push(path.clone()),
        }
    }

    // Shallowest first, so removing a parent takes its nested packages with it
    // and the deeper entries become no-ops.
    stale.sort();
    let mut removed_roots: Vec<NormalizedPath> = Vec::new();
    for path in stale {
        if removed_roots.iter().any(|root| path.starts_with(root)) {
            continue;
        }
        let absolute = project_root.join(path.as_str());
        if absolute.exists() {
            std::fs::remove_dir_all(&absolute)
                .map_err(|source| LinkError::io(&absolute, source))?;
        }
        removed_roots.push(path);
        report.removed += 1;
    }

    for (path, id) in layout {
        let intact = actual.get(path).is_some_and(|marker| {
            !removed_roots.iter().any(|root| path.starts_with(root))
                && marker.name == id.name
                && marker.version == id.version.to_string()
        });
        if intact {
            report.unchanged += 1;
            continue;
        }
        let package = fetched
            .get(id)
            .ok_or_else(|| LinkError::NotFetched { id: id.clone() })?;
        materialize(project_root, path, package, cas, &mut report)?;
        report.added += 1;
        fault::checkpoint(FAULT_BETWEEN_PACKAGES);
    }

    write_bin_directories(project_root, layout, fetched, cas, &mut report)?;
    Ok(report)
}

/// Writes one package's files, then its marker.
fn materialize(
    project_root: &Path,
    relative: &NormalizedPath,
    package: &FetchedPackage,
    cas: &Cas,
    report: &mut LinkReport,
) -> Result<(), LinkError> {
    let directory = project_root.join(relative.as_str());
    // Start from nothing: an interrupted previous attempt may have left a
    // partial tree here, and a package is cheap to rebuild.
    if directory.exists() {
        std::fs::remove_dir_all(&directory).map_err(|source| LinkError::io(&directory, source))?;
    }

    let mut linked_one = false;
    for (path, file) in &package.index.files {
        let destination = directory.join(path.as_str());
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LinkError::io(parent, source))?;
        }
        let source = cas.object_path(&file.hash);

        if file.executable {
            // CAS objects are read-only and shared by inode, so a file that
            // needs its execute bit gets a private copy instead of a hardlink.
            copy_with_mode(&source, &destination, 0o555)?;
            report.files_copied += 1;
        } else if report.hardlink_fallback.is_some() {
            // Already established that this filesystem cannot do it.
            copy_with_mode(&source, &destination, 0o444)?;
            report.files_copied += 1;
        } else {
            match std::fs::hard_link(&source, &destination) {
                Ok(()) => report.files_linked += 1,
                Err(error) => {
                    if let LinkFailure::Impossible(reason) = classify_link_failure(&error) {
                        report.hardlink_fallback = Some(reason);
                    }
                    copy_with_mode(&source, &destination, 0o444)?;
                    report.files_copied += 1;
                }
            }
        }

        if !linked_one {
            linked_one = true;
            fault::checkpoint(FAULT_MID_LINK);
        }
    }

    let marker = format!(
        "{} {} {}\n",
        package.index.name, package.index.version, package.index_hash
    );
    let marker_path = directory.join(MARKER_FILE);
    write_atomic(&marker_path, marker.as_bytes(), None)
        .map_err(|source| LinkError::io(marker_path, source))?;
    Ok(())
}

fn copy_with_mode(source: &Path, destination: &Path, mode: u32) -> Result<(), LinkError> {
    std::fs::copy(source, destination).map_err(|error| LinkError::io(destination, error))?;
    set_mode(destination, mode)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), LinkError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|source| LinkError::io(path, source))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// The one place a symlink is created.
///
/// Native Windows (v2) needs a junction or a `.cmd` shim here instead, which is
/// why every caller goes through this function rather than reaching for
/// `std::os::unix::fs::symlink`.
fn symlink(target: &Path, link: &Path) -> Result<(), LinkError> {
    if link.exists() || std::fs::symlink_metadata(link).is_ok() {
        std::fs::remove_file(link).map_err(|source| LinkError::io(link, source))?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|source| LinkError::io(link, source))
    }
    #[cfg(not(unix))]
    {
        let _ = target;
        Ok(())
    }
}

/// Rebuilds every `.bin` directory the layout implies.
///
/// `.bin` is derived state and tiny, so it is rebuilt rather than diffed. The
/// entries must be symlinks: a copied CLI script would resolve its own relative
/// `require`s against `.bin/`, and break.
fn write_bin_directories(
    project_root: &Path,
    layout: &Layout,
    fetched: &Fetched,
    cas: &Cas,
    report: &mut LinkReport,
) -> Result<(), LinkError> {
    let mut by_owner: BTreeMap<NormalizedPath, Vec<(&NormalizedPath, &PackageId)>> =
        BTreeMap::new();
    for (path, id) in layout {
        let Some(owner) = path.parent().and_then(|parent| {
            // `…/node_modules/<name>` and `…/node_modules/@scope/<name>`.
            if parent.file_name() == Some(NODE_MODULES) {
                Some(parent)
            } else {
                parent.parent()
            }
        }) else {
            continue;
        };
        by_owner.entry(owner).or_default().push((path, id));
    }

    for (owner, packages) in by_owner {
        let bin_directory = project_root.join(owner.as_str()).join(BIN_DIR);
        if bin_directory.exists() {
            std::fs::remove_dir_all(&bin_directory)
                .map_err(|source| LinkError::io(&bin_directory, source))?;
        }

        for (path, id) in packages {
            let Some(package) = fetched.get(id) else {
                continue;
            };
            let Some(manifest_file) = package.index.file("package.json") else {
                continue;
            };
            let bytes = cas.read(&manifest_file.hash)?;
            let Ok(value) = serde_json::from_slice(&bytes) else {
                continue;
            };
            let manifest = Manifest::from_value(&value);

            for (command, target) in &manifest.bin {
                // A command name is a filename, never a path.
                if command.contains('/') || command.contains('\\') {
                    continue;
                }
                let relative_target = NormalizedPath::new(target);
                let absolute_target = project_root
                    .join(path.as_str())
                    .join(relative_target.as_str());
                if !absolute_target.is_file() {
                    continue;
                }
                // npm marks bin targets executable regardless of the tarball's
                // mode, and plenty of packages rely on that.
                ensure_executable(&absolute_target, report)?;

                std::fs::create_dir_all(&bin_directory)
                    .map_err(|source| LinkError::io(&bin_directory, source))?;
                let link = bin_directory.join(command);
                let Some(from_bin) = path
                    .join(relative_target.as_str())
                    .relative_to(&owner.join(BIN_DIR))
                else {
                    continue;
                };
                symlink(Path::new(from_bin.as_str()), &link)?;
                report.bins += 1;
            }
        }
    }
    Ok(())
}

/// Makes a file executable, breaking the hardlink first so the CAS object and
/// every other package sharing that inode are unaffected.
fn ensure_executable(path: &Path, report: &mut LinkReport) -> Result<(), LinkError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let metadata = std::fs::metadata(path).map_err(|source| LinkError::io(path, source))?;
        if metadata.permissions().mode() & 0o111 != 0 {
            return Ok(());
        }
        let contents = std::fs::read(path).map_err(|source| LinkError::io(path, source))?;
        std::fs::remove_file(path).map_err(|source| LinkError::io(path, source))?;
        std::fs::write(path, contents).map_err(|source| LinkError::io(path, source))?;
        set_mode(path, 0o555)?;
        report.files_copied += 1;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, report);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Marker {
    name: String,
    version: String,
    index_hash: ContentHash,
}

/// Reads every `.opal-package` marker under the project's `node_modules`.
fn scan(project_root: &Path) -> Result<BTreeMap<NormalizedPath, Marker>, LinkError> {
    let mut found = BTreeMap::new();
    let mut queue = vec![project_root.join(NODE_MODULES)];

    while let Some(directory) = queue.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(LinkError::io(&directory, source)),
        };
        for entry in entries {
            let entry = entry.map_err(|source| LinkError::io(&directory, source))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !path.is_dir() || name == BIN_DIR {
                continue;
            }
            // A scope directory holds packages, not a package.
            if name.starts_with('@') {
                queue.push(path);
                continue;
            }

            if let Some(marker) = read_marker(&path)?
                && let Ok(relative) = relative_to_root(project_root, &path)
            {
                found.insert(relative, marker);
            }
            queue.push(path.join(NODE_MODULES));
        }
    }
    Ok(found)
}

fn read_marker(package_directory: &Path) -> Result<Option<Marker>, LinkError> {
    let path = package_directory.join(MARKER_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // No marker means "not installed", whatever else is in the directory.
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(LinkError::io(path, source)),
    };
    let mut fields = text.split_whitespace();
    let (Some(name), Some(version), Some(hash)) = (fields.next(), fields.next(), fields.next())
    else {
        return Ok(None);
    };
    Ok(ContentHash::parse_hex(hash).ok().map(|index_hash| Marker {
        name: name.to_string(),
        version: version.to_string(),
        index_hash,
    }))
}

fn relative_to_root(project_root: &Path, path: &Path) -> Result<NormalizedPath, LinkError> {
    let root = NormalizedPath::from_native(project_root)
        .map_err(|error| LinkError::io(project_root, std::io::Error::other(error)))?;
    let full = NormalizedPath::from_native(path)
        .map_err(|error| LinkError::io(path, std::io::Error::other(error)))?;
    full.relative_to(&root)
        .ok_or_else(|| LinkError::io(path, std::io::Error::other("outside the project root")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::{Algorithm, Integrity};
    use crate::manifest::DependencyClass;
    use crate::resolve::{RequirementRecord, ResolvedEdge, ResolvedPackage};
    use crate::semver::Version;

    fn package(
        name: &str,
        version: (u64, u64, u64),
        deps: &[(&str, (u64, u64, u64))],
    ) -> ResolvedPackage {
        let id = PackageId::new(name, Version::new(version.0, version.1, version.2));
        ResolvedPackage {
            id: id.clone(),
            tarball: format!("file:///fixture/{name}.tgz"),
            integrity: Integrity::of(Algorithm::Sha512, name.as_bytes()),
            dependencies: deps
                .iter()
                .map(|(name, version)| ResolvedEdge {
                    name: (*name).to_string(),
                    spec: "*".to_string(),
                    version: Version::new(version.0, version.1, version.2),
                })
                .collect(),
            os: Vec::new(),
            cpu: Vec::new(),
        }
    }

    fn only_on(mut package: ResolvedPackage, os: &str) -> ResolvedPackage {
        package.os = vec![os.to_string()];
        package
    }

    fn resolution(roots: &[&str], packages: Vec<ResolvedPackage>) -> Resolution {
        classified_resolution(
            &roots
                .iter()
                .map(|name| (DependencyClass::Runtime, *name))
                .collect::<Vec<_>>(),
            packages,
        )
    }

    /// Roots that publish exactly one version, so there is nothing to choose.
    /// Ambiguity has to be spelled out with [`rooted_resolution`] — deriving it
    /// by version order is the bug these tests exist to catch, and a helper
    /// that does it quietly reintroduces it into every test written after.
    fn classified_resolution(
        roots: &[(DependencyClass, &str)],
        packages: Vec<ResolvedPackage>,
    ) -> Resolution {
        let roots: Vec<(DependencyClass, &str, Version)> = roots
            .iter()
            .map(|(class, name)| {
                let mut versions = packages
                    .iter()
                    .filter(|package| package.id.name == *name)
                    .map(|package| package.id.version.clone());
                let version = versions.next().expect("a root has to be resolved");
                assert!(
                    versions.next().is_none(),
                    "{name} publishes more than one version: name the root's own"
                );
                (*class, *name, version)
            })
            .collect();
        rooted_resolution(&roots, packages)
    }

    fn rooted_resolution(
        roots: &[(DependencyClass, &str, Version)],
        packages: Vec<ResolvedPackage>,
    ) -> Resolution {
        Resolution {
            requirements: roots
                .iter()
                .map(|(class, name, version)| RequirementRecord {
                    class: *class,
                    name: (*name).to_string(),
                    spec: "*".to_string(),
                    version: Some(version.clone()),
                })
                .collect(),
            packages: packages
                .into_iter()
                .map(|package| (package.id.clone(), package))
                .collect(),
            skipped: Vec::new(),
        }
    }

    fn linux() -> PlanOptions {
        PlanOptions {
            platform: Platform::new("linux", "x64"),
            include_development: true,
        }
    }

    fn layout_of(resolution: &Resolution) -> Layout {
        plan(resolution, &linux()).layout
    }

    fn paths(layout: &Layout) -> Vec<String> {
        layout
            .iter()
            .map(|(path, id)| format!("{path} = {id}"))
            .collect()
    }

    #[test]
    fn test_hoists_a_simple_tree_flat() {
        let layout = layout_of(&resolution(
            &["a"],
            vec![
                package("a", (1, 0, 0), &[("b", (1, 0, 0))]),
                package("b", (1, 0, 0), &[]),
            ],
        ));
        assert_eq!(
            paths(&layout),
            vec![
                "node_modules/a = a@1.0.0".to_string(),
                "node_modules/b = b@1.0.0".to_string(),
            ]
        );
    }

    #[test]
    fn test_conflicting_versions_nest_under_the_dependent() {
        let layout = layout_of(&resolution(
            &["a", "b"],
            vec![
                package("a", (1, 0, 0), &[("shared", (1, 0, 0))]),
                package("b", (1, 0, 0), &[("shared", (2, 0, 0))]),
                package("shared", (1, 0, 0), &[]),
                package("shared", (2, 0, 0), &[]),
            ],
        ));
        // One of them wins the hoisted slot; the other nests. Which one is
        // decided by sorted order, so it is stable across runs.
        assert!(layout.contains_key(&NormalizedPath::new("node_modules/shared")));
        assert!(
            layout.contains_key(&NormalizedPath::new("node_modules/b/node_modules/shared"))
                || layout.contains_key(&NormalizedPath::new("node_modules/a/node_modules/shared"))
        );
        assert_eq!(layout.len(), 4);
    }

    #[test]
    fn test_dependency_cycles_terminate() {
        let layout = layout_of(&resolution(
            &["a"],
            vec![
                package("a", (1, 0, 0), &[("b", (1, 0, 0))]),
                package("b", (1, 0, 0), &[("a", (1, 0, 0))]),
            ],
        ));
        assert_eq!(layout.len(), 2);
    }

    #[test]
    fn test_plan_is_deterministic() {
        let build = || {
            layout_of(&resolution(
                &["a", "b"],
                vec![
                    package("a", (1, 0, 0), &[("shared", (1, 0, 0)), ("x", (1, 0, 0))]),
                    package("b", (1, 0, 0), &[("shared", (2, 0, 0))]),
                    package("shared", (1, 0, 0), &[]),
                    package("shared", (2, 0, 0), &[]),
                    package("x", (1, 0, 0), &[]),
                ],
            ))
        };
        assert_eq!(paths(&build()), paths(&build()));
    }

    #[test]
    fn test_scoped_packages_keep_their_scope_directory() {
        let layout = layout_of(&resolution(
            &["@scope/a"],
            vec![package("@scope/a", (1, 0, 0), &[])],
        ));
        assert_eq!(
            paths(&layout),
            vec!["node_modules/@scope/a = @scope/a@1.0.0".to_string()]
        );
    }

    #[test]
    fn test_a_root_keeps_its_own_version_when_a_dependency_wants_a_higher_one() {
        // The project asks for shared@1, `b` asks for shared@2. Deriving the
        // root from version order picks 2.0.0 and never places 1.0.0 at all.
        let layout = layout_of(&rooted_resolution(
            &[
                (DependencyClass::Runtime, "shared", Version::new(1, 0, 0)),
                (DependencyClass::Runtime, "b", Version::new(1, 0, 0)),
            ],
            vec![
                package("shared", (1, 0, 0), &[]),
                package("shared", (2, 0, 0), &[]),
                package("b", (1, 0, 0), &[("shared", (2, 0, 0))]),
            ],
        ));
        assert_eq!(
            layout.get(&NormalizedPath::new("node_modules/shared")),
            Some(&PackageId::new("shared", Version::new(1, 0, 0)))
        );
        assert_eq!(
            layout.get(&NormalizedPath::new("node_modules/b/node_modules/shared")),
            Some(&PackageId::new("shared", Version::new(2, 0, 0)))
        );
    }

    #[test]
    fn test_a_package_this_host_cannot_run_is_left_out_with_its_subtree() {
        let plan = plan(
            &resolution(
                &["a", "native"],
                vec![
                    package("a", (1, 0, 0), &[]),
                    only_on(
                        package("native", (1, 0, 0), &[("helper", (1, 0, 0))]),
                        "darwin",
                    ),
                    package("helper", (1, 0, 0), &[]),
                ],
            ),
            &linux(),
        );
        assert_eq!(paths(&plan.layout), vec!["node_modules/a = a@1.0.0"]);
        assert_eq!(plan.platform_skipped.len(), 1);
        assert_eq!(
            plan.platform_skipped[0].1,
            "requires os darwin (host is linux)"
        );
    }

    #[test]
    fn test_the_same_package_is_placed_for_a_host_that_can_run_it() {
        let resolution = resolution(
            &["native"],
            vec![only_on(package("native", (1, 0, 0), &[]), "darwin")],
        );
        let plan = plan(
            &resolution,
            &PlanOptions {
                platform: Platform::new("darwin", "arm64"),
                include_development: true,
            },
        );
        assert_eq!(
            paths(&plan.layout),
            vec!["node_modules/native = native@1.0.0"]
        );
        assert!(plan.platform_skipped.is_empty());
    }

    #[test]
    fn test_a_production_plan_drops_dev_roots_but_keeps_shared_packages() {
        let resolution = classified_resolution(
            &[
                (DependencyClass::Runtime, "a"),
                (DependencyClass::Development, "tool"),
            ],
            vec![
                package("a", (1, 0, 0), &[("shared", (1, 0, 0))]),
                package("tool", (1, 0, 0), &[("shared", (1, 0, 0))]),
                package("shared", (1, 0, 0), &[]),
            ],
        );
        let production = plan(
            &resolution,
            &PlanOptions {
                include_development: false,
                ..linux()
            },
        );
        assert_eq!(
            paths(&production.layout),
            vec![
                "node_modules/a = a@1.0.0",
                "node_modules/shared = shared@1.0.0"
            ]
        );
        assert_eq!(plan(&resolution, &linux()).layout.len(), 3);
    }

    #[test]
    fn test_a_cross_device_failure_is_terminal_and_anything_else_is_not() {
        // A cross-device mount cannot be conjured in a test, so the classifier
        // is exercised directly on the errno the kernel would report.
        let exdev = std::io::Error::from_raw_os_error(libc::EXDEV);
        assert!(matches!(
            classify_link_failure(&exdev),
            LinkFailure::Impossible(_)
        ));

        let too_many_links = std::io::Error::from_raw_os_error(libc::EMLINK);
        assert!(matches!(
            classify_link_failure(&too_many_links),
            LinkFailure::ThisFile
        ));

        // No errno at all: a synthesized error, not something to give up over.
        let synthetic = std::io::Error::other("no errno");
        assert!(matches!(
            classify_link_failure(&synthetic),
            LinkFailure::ThisFile
        ));
    }

    #[test]
    fn test_owning_directories_walk_up_through_nesting() {
        let owner = NormalizedPath::new("node_modules/a/node_modules/b");
        let chain: Vec<String> = owning_directories(&owner)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            chain,
            vec![
                ".".to_string(),
                "node_modules/a".to_string(),
                "node_modules/a/node_modules/b".to_string(),
            ]
        );
    }
}
