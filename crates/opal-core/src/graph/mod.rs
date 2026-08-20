//! The module graph: what every Opal tool consumes.
//!
//! A graph is defined *relative to a project root*. Module paths, trace paths,
//! and diagnostics are all root-relative, and the absolute root lives only in
//! memory. That is what lets one cached graph serve the same project checked
//! out at a different path, in a different container, on a different machine.
//!
//! Modules are stored sorted by path and ids are assigned in that order, so a
//! graph built by walking imports and a graph loaded from cache are identical
//! structures, not merely equivalent ones.

pub mod memo;
pub mod resolver;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::hash::{ContentHash, HashBuilder};
use crate::path::NormalizedPath;

pub use memo::{
    CacheStatus, CachedResolution, GraphCache, MemoError, MemoKey, MissReason, resolve_cached,
};
pub use resolver::{Resolution, ResolveError, ResolveTrace, ResolverOptions, resolve};

/// Bumped when the serialized shape changes; older snapshots then miss instead
/// of being misread.
pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Memo(#[from] MemoError),
}

/// Index into [`ModuleGraph::modules`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct ModuleId(u32);

impl ModuleId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What the file is written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    #[serde(rename = "javascript")]
    JavaScript,
    Jsx,
    #[serde(rename = "typescript")]
    TypeScript,
    Tsx,
    Json,
    /// Anything Opal does not parse for imports — `.node`, `.wasm`, assets.
    /// Real packages `require` these, so they are graph nodes, just leaves.
    Opaque,
}

impl SourceKind {
    pub fn is_parsed(self) -> bool {
        matches!(
            self,
            Self::JavaScript | Self::Jsx | Self::TypeScript | Self::Tsx
        )
    }
}

/// Which module semantics the file is loaded under.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModuleSystem {
    Esm,
    Cjs,
    /// JSON and opaque files, which have no import semantics of their own.
    NotApplicable,
}

/// How one module reaches another.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyKind {
    StaticImport,
    DynamicImport,
    Require,
    ExportFrom,
    /// TypeScript's `import x = require('y')`.
    TsImportEquals,
}

impl DependencyKind {
    /// Whether this edge resolves under the `require` export condition rather
    /// than `import` — the distinction Node makes when reading an `exports` map.
    pub fn is_require_like(self) -> bool {
        matches!(self, Self::Require | Self::TsImportEquals)
    }
}

/// Where a dependency points.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "kebab-case")]
pub enum DependencyTarget {
    Module {
        id: ModuleId,
    },
    /// A Node builtin (`fs`, `node:path`) — outside the graph by definition.
    Builtin {
        name: String,
    },
    /// Recorded rather than fatal: a real project graph contains optional deps,
    /// platform-specific requires, and packages that are not installed yet.
    Unresolved {
        reason: String,
    },
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub specifier: String,
    pub kind: DependencyKind,
    #[serde(flatten)]
    pub target: DependencyTarget,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Module {
    /// Root-relative.
    pub path: NormalizedPath,
    pub content_hash: ContentHash,
    pub source: SourceKind,
    pub system: ModuleSystem,
    pub dependencies: Vec<Dependency>,
}

/// Something worth reporting that did not stop resolution.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Root-relative path of the module the diagnostic is about.
    pub module: NormalizedPath,
    pub message: String,
}

/// The serialized form. This is what lands in the CAS and what golden tests
/// compare against.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub version: u32,
    pub entry: NormalizedPath,
    pub modules: Vec<Module>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug)]
pub struct ModuleGraph {
    root: NormalizedPath,
    entry: ModuleId,
    modules: Vec<Module>,
    diagnostics: Vec<Diagnostic>,
    by_path: HashMap<NormalizedPath, ModuleId>,
}

impl ModuleGraph {
    /// The absolute project root the graph was resolved against.
    pub fn root(&self) -> &NormalizedPath {
        &self.root
    }

    pub fn entry(&self) -> ModuleId {
        self.entry
    }

    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    pub fn module(&self, id: ModuleId) -> &Module {
        &self.modules[id.index()]
    }

    /// Looks up a module by its root-relative path.
    pub fn id_of(&self, path: &NormalizedPath) -> Option<ModuleId> {
        self.by_path.get(path).copied()
    }

    pub fn absolute_path(&self, id: ModuleId) -> NormalizedPath {
        self.root.join(self.module(id).path.as_str())
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    pub fn edge_count(&self) -> usize {
        self.modules
            .iter()
            .map(|module| module.dependencies.len())
            .sum()
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Modules that depend on `id`, sorted.
    pub fn dependents(&self, id: ModuleId) -> Vec<ModuleId> {
        self.modules
            .iter()
            .enumerate()
            .filter(|(_, module)| {
                module.dependencies.iter().any(|dependency| {
                    matches!(dependency.target, DependencyTarget::Module { id: target } if target == id)
                })
            })
            .map(|(index, _)| ModuleId(index as u32))
            .collect()
    }

    pub fn unresolved(&self) -> impl Iterator<Item = (&Module, &Dependency)> {
        self.modules.iter().flat_map(|module| {
            module
                .dependencies
                .iter()
                .filter(|dependency| {
                    matches!(dependency.target, DependencyTarget::Unresolved { .. })
                })
                .map(move |dependency| (module, dependency))
        })
    }

    /// A single hash identifying this graph's shape and contents.
    ///
    /// Diagnostics are deliberately excluded: they are advisory text, and
    /// rewording a message must not change the identity of a resolved graph
    /// that two tools are trying to share.
    pub fn digest(&self) -> ContentHash {
        let mut builder = HashBuilder::new("opal.graph.digest.v1");
        builder.push_u64(u64::from(SNAPSHOT_VERSION));
        builder.push_str(self.module(self.entry).path.as_str());
        for module in &self.modules {
            builder.push_str(module.path.as_str());
            builder.push_hash(&module.content_hash);
            builder.push_str(&format!("{:?}/{:?}", module.source, module.system));
            builder.push_u64(module.dependencies.len() as u64);
            for dependency in &module.dependencies {
                builder.push_str(&dependency.specifier);
                builder.push_str(&format!("{:?}", dependency.kind));
                match &dependency.target {
                    DependencyTarget::Module { id } => {
                        builder.push_str("module");
                        builder.push_str(self.module(*id).path.as_str());
                    }
                    DependencyTarget::Builtin { name } => {
                        builder.push_str("builtin");
                        builder.push_str(name);
                    }
                    DependencyTarget::Unresolved { reason } => {
                        builder.push_str("unresolved");
                        builder.push_str(reason);
                    }
                }
            }
        }
        builder.finish()
    }

    pub fn to_snapshot(&self) -> GraphSnapshot {
        GraphSnapshot {
            version: SNAPSHOT_VERSION,
            entry: self.module(self.entry).path.clone(),
            modules: self.modules.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    pub fn from_snapshot(
        root: &NormalizedPath,
        snapshot: GraphSnapshot,
    ) -> Result<Self, SnapshotError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::Version(snapshot.version));
        }
        let by_path: HashMap<NormalizedPath, ModuleId> = snapshot
            .modules
            .iter()
            .enumerate()
            .map(|(index, module)| (module.path.clone(), ModuleId(index as u32)))
            .collect();
        let entry = *by_path
            .get(&snapshot.entry)
            .ok_or_else(|| SnapshotError::EntryMissing(snapshot.entry.clone()))?;

        for module in &snapshot.modules {
            for dependency in &module.dependencies {
                if let DependencyTarget::Module { id } = dependency.target
                    && id.index() >= snapshot.modules.len()
                {
                    return Err(SnapshotError::DanglingEdge(id));
                }
            }
        }

        Ok(Self {
            root: root.clone(),
            entry,
            modules: snapshot.modules,
            diagnostics: snapshot.diagnostics,
            by_path,
        })
    }

    /// Deterministic pretty JSON — the form golden tests diff.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_snapshot())
            .expect("graph snapshots contain only serializable values")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot version {0} (this build understands {SNAPSHOT_VERSION})")]
    Version(u32),
    #[error("snapshot entry {0} is not among its modules")]
    EntryMissing(NormalizedPath),
    #[error("snapshot has an edge to module {0:?}, which does not exist")]
    DanglingEdge(ModuleId),
}

/// Accumulates modules during resolution, then canonicalizes them.
pub(crate) struct ModuleGraphBuilder {
    root: NormalizedPath,
    entry: Option<usize>,
    /// Absolute paths, parallel to `modules`.
    paths: Vec<NormalizedPath>,
    modules: Vec<Module>,
    ids: HashMap<NormalizedPath, usize>,
    diagnostics: Vec<(NormalizedPath, String)>,
}

impl ModuleGraphBuilder {
    pub(crate) fn new(root: NormalizedPath) -> Self {
        Self {
            root,
            entry: None,
            paths: Vec::new(),
            modules: Vec::new(),
            ids: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn id_of(&self, absolute: &NormalizedPath) -> Option<usize> {
        self.ids.get(absolute).copied()
    }

    /// Registers a module, returning its provisional index.
    pub(crate) fn insert(
        &mut self,
        absolute: NormalizedPath,
        content_hash: ContentHash,
        source: SourceKind,
        system: ModuleSystem,
    ) -> usize {
        if let Some(index) = self.ids.get(&absolute) {
            return *index;
        }
        let index = self.modules.len();
        let path = self.relative(&absolute);
        self.ids.insert(absolute.clone(), index);
        self.paths.push(absolute);
        self.modules.push(Module {
            path,
            content_hash,
            source,
            system,
            dependencies: Vec::new(),
        });
        if self.entry.is_none() {
            self.entry = Some(index);
        }
        index
    }

    pub(crate) fn add_dependency(&mut self, from: usize, dependency: Dependency) {
        self.modules[from].dependencies.push(dependency);
    }

    pub(crate) fn add_diagnostic(&mut self, absolute: &NormalizedPath, message: impl Into<String>) {
        self.diagnostics
            .push((self.relative(absolute), message.into()));
    }

    fn relative(&self, absolute: &NormalizedPath) -> NormalizedPath {
        absolute
            .relative_to(&self.root)
            .unwrap_or_else(|| absolute.clone())
    }

    /// Sorts modules by path, renumbers ids, and sorts diagnostics.
    pub(crate) fn finish(mut self) -> ModuleGraph {
        let mut order: Vec<usize> = (0..self.modules.len()).collect();
        order.sort_by(|left, right| self.modules[*left].path.cmp(&self.modules[*right].path));

        let mut remap = vec![ModuleId(0); self.modules.len()];
        for (new_index, old_index) in order.iter().enumerate() {
            remap[*old_index] = ModuleId(new_index as u32);
        }

        let mut modules: Vec<Module> = order
            .iter()
            .map(|old_index| self.modules[*old_index].clone())
            .collect();
        for module in &mut modules {
            for dependency in &mut module.dependencies {
                if let DependencyTarget::Module { id } = &mut dependency.target {
                    *id = remap[id.index()];
                }
            }
        }

        let entry = self.entry.map(|index| remap[index]).unwrap_or(ModuleId(0));

        self.diagnostics.sort();
        self.diagnostics.dedup();
        let diagnostics = self
            .diagnostics
            .into_iter()
            .map(|(module, message)| Diagnostic { module, message })
            .collect();

        let by_path = modules
            .iter()
            .enumerate()
            .map(|(index, module)| (module.path.clone(), ModuleId(index as u32)))
            .collect();

        ModuleGraph {
            root: self.root,
            entry,
            modules,
            diagnostics,
            by_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ModuleGraph {
        let root = NormalizedPath::new("/project");
        let mut builder = ModuleGraphBuilder::new(root);
        let entry = builder.insert(
            NormalizedPath::new("/project/z-entry.js"),
            ContentHash::of(b"entry"),
            SourceKind::JavaScript,
            ModuleSystem::Esm,
        );
        let leaf = builder.insert(
            NormalizedPath::new("/project/a-leaf.js"),
            ContentHash::of(b"leaf"),
            SourceKind::JavaScript,
            ModuleSystem::Esm,
        );
        builder.add_dependency(
            entry,
            Dependency {
                specifier: "./a-leaf.js".to_string(),
                kind: DependencyKind::StaticImport,
                target: DependencyTarget::Module {
                    id: ModuleId(leaf as u32),
                },
            },
        );
        builder.add_dependency(
            entry,
            Dependency {
                specifier: "node:fs".to_string(),
                kind: DependencyKind::StaticImport,
                target: DependencyTarget::Builtin {
                    name: "fs".to_string(),
                },
            },
        );
        builder.finish()
    }

    #[test]
    fn test_modules_are_sorted_and_edges_follow() {
        let graph = sample();
        let paths: Vec<&str> = graph.modules().iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, vec!["a-leaf.js", "z-entry.js"]);

        // The entry was inserted first but sorts last; its edge must have been
        // renumbered to still point at the leaf.
        let entry = graph.module(graph.entry());
        assert_eq!(entry.path.as_str(), "z-entry.js");
        assert_eq!(
            entry.dependencies[0].target,
            DependencyTarget::Module { id: ModuleId(0) }
        );
        assert_eq!(graph.edge_count(), 2);
        assert_eq!(graph.dependents(ModuleId(0)), vec![graph.entry()]);
    }

    #[test]
    fn test_snapshot_round_trip_preserves_identity() {
        let graph = sample();
        let json = graph.to_json();
        let snapshot: GraphSnapshot = serde_json::from_str(&json).unwrap();
        let restored = ModuleGraph::from_snapshot(graph.root(), snapshot).unwrap();

        assert_eq!(restored.digest(), graph.digest());
        assert_eq!(restored.to_json(), json);
        assert_eq!(restored.entry(), graph.entry());
    }

    #[test]
    fn test_digest_tracks_content_and_shape() {
        let graph = sample();
        let baseline = graph.digest();

        let mut changed = graph.to_snapshot();
        changed.modules[0].content_hash = ContentHash::of(b"different");
        let changed = ModuleGraph::from_snapshot(graph.root(), changed).unwrap();
        assert_ne!(changed.digest(), baseline);

        let mut reworded = graph.to_snapshot();
        reworded.diagnostics.push(Diagnostic {
            module: NormalizedPath::new("a-leaf.js"),
            message: "advisory".to_string(),
        });
        let reworded = ModuleGraph::from_snapshot(graph.root(), reworded).unwrap();
        assert_eq!(reworded.digest(), baseline);
    }

    #[test]
    fn test_snapshot_version_mismatch_is_refused() {
        let graph = sample();
        let mut snapshot = graph.to_snapshot();
        snapshot.version = SNAPSHOT_VERSION + 1;
        assert!(matches!(
            ModuleGraph::from_snapshot(graph.root(), snapshot),
            Err(SnapshotError::Version(_))
        ));
    }

    #[test]
    fn test_absolute_path_rebuilds_from_root() {
        let graph = sample();
        assert_eq!(
            graph.absolute_path(graph.entry()).as_str(),
            "/project/z-entry.js"
        );
    }
}
