//! Which test files a change affects: what `--findRelatedTests` runs, and what
//! watch mode reruns when a file changes.
//!
//! It is a reverse-dependency query on opal-core's module graph, made to give
//! the answer jest-resolve-dependencies' `resolveInverse` gives:
//!
//! - The graph is walked from every test file and every manual mock at once.
//!   Imports are resolved with `moduleFileExtensions` in Jest's order and with
//!   `moduleNameMapper` applied first. `jest.requireActual`, `jest.requireMock`
//!   and `jest.createMockFromModule` count as imports, as Jest's dependency
//!   extractor counts them. Packages are where the graph stops.
//! - Jest adds edges of its own. A module that imports `./x` (or `pkg/x`)
//!   also depends on the manual mock named `x` when that mock is the one in
//!   the `__mocks__` folder beside what `./x` resolved to; a root mock of the
//!   same name is not an edge. An import that doesn't resolve depends on the
//!   manual mock named by the whole specifier, if there is one.
//! - Only files in Jest's haste map take part. A changed file outside it
//!   (in `node_modules`, outside `roots`, with another extension) relates to
//!   nothing, and a dependency chain only passes through files inside it.
//! - A changed snapshot file stands for its test file.
//!
//! The related tests are the changed files that are tests, and every test that
//! reaches a changed file through those dependencies.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use opal_core::graph::{
    DependencyTarget, ModuleGraph, ResolverHooks, ResolverOptions, resolve_with,
};
use opal_core::path::NormalizedPath;
use regex::Regex;
use serde_json::Value;

use crate::config::ProjectConfig;
use crate::discover::Found;

/// The functions Jest's dependency extractor reads besides `require` and
/// `import` (jest-haste-map's `dependencyExtractor`).
const JEST_REQUIRE_FUNCTIONS: &[&str] = &[
    "jest.requireActual",
    "jest.requireMock",
    "jest.createMockFromModule",
];

/// jest-resolve's `moduleNameMapper`: the first pattern that matches a
/// specifier maps it, and `$1`... in a replacement are the pattern's groups.
struct NameMapper {
    entries: Vec<(Regex, Vec<String>)>,
}

impl NameMapper {
    fn new(mapper: &serde_json::Map<String, Value>) -> Self {
        let entries = mapper
            .iter()
            .filter_map(|(pattern, replacement)| {
                // A JavaScript pattern Rust's regex can't read maps nothing.
                let regex = Regex::new(pattern).ok()?;
                let replacements = match replacement {
                    Value::String(one) => vec![one.clone()],
                    Value::Array(many) => many
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect(),
                    _ => return None,
                };
                Some((regex, replacements))
            })
            .collect();
        NameMapper { entries }
    }

    fn map(&self, specifier: &str) -> Option<Vec<String>> {
        let (regex, replacements) = self
            .entries
            .iter()
            .find(|(regex, _)| regex.is_match(specifier))?;
        let captures = regex.captures(specifier)?;
        let group = Regex::new(r"\$([0-9]+)").expect("valid");
        Some(
            replacements
                .iter()
                .map(|replacement| {
                    group
                        .replace_all(replacement, |found: &regex::Captures| {
                            let index: usize = found[1].parse().unwrap_or(usize::MAX);
                            captures
                                .get(index)
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_default()
                        })
                        .into_owned()
                })
                .collect(),
        )
    }
}

/// The test files related to `changed`, sorted. Relative paths in `changed`
/// are relative to `cwd`.
pub fn related_tests(
    config: &ProjectConfig,
    found: &Found,
    changed: &[PathBuf],
    cwd: &Path,
) -> Vec<PathBuf> {
    let universe: HashSet<&Path> = found.files.iter().map(PathBuf::as_path).collect();
    let tests: HashSet<&Path> = found.tests.iter().map(PathBuf::as_path).collect();

    // resolveInverseModuleMap's starting point.
    let mut related: BTreeSet<PathBuf> = BTreeSet::new();
    let mut frontier: HashSet<PathBuf> = HashSet::new();
    for path in changed {
        let path = clean(&cwd.join(path));
        if !universe.contains(path.as_path()) {
            continue;
        }
        let module = if path.extension().is_some_and(|e| e == "snap") {
            test_path_for_snapshot(&path)
        } else {
            path
        };
        if tests.contains(module.as_path()) {
            related.insert(module.clone());
        }
        frontier.insert(module);
    }
    if frontier.is_empty() {
        return related.into_iter().collect();
    }

    let Some(dependents) = dependents(config, found, &universe) else {
        return related.into_iter().collect();
    };

    // Breadth first from the changed files, through files in the haste map.
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<PathBuf> = frontier.into_iter().collect();
    while let Some(path) = queue.pop_front() {
        for dependent in dependents.get(&path).into_iter().flatten() {
            if visited.insert(dependent.clone()) {
                if tests.contains(dependent.as_path()) {
                    related.insert(dependent.clone());
                }
                queue.push_back(dependent.clone());
            }
        }
    }
    related.into_iter().collect()
}

/// For every file, the files in the haste map that depend on it directly.
fn dependents(
    config: &ProjectConfig,
    found: &Found,
    universe: &HashSet<&Path>,
) -> Option<HashMap<PathBuf, Vec<PathBuf>>> {
    let root = NormalizedPath::from_native(&config.root_dir).ok()?;
    let relative = |path: &Path| -> Option<NormalizedPath> {
        NormalizedPath::from_native(path).ok()?.relative_to(&root)
    };
    // Entries outside rootDir can't be walked from it; Jest's roots almost
    // always sit inside it.
    let mut entries: Vec<NormalizedPath> = found.tests.iter().filter_map(|p| relative(p)).collect();
    entries.extend(found.manual_mocks.values().filter_map(|p| relative(p)));
    entries.sort();
    entries.dedup();
    if entries.is_empty() {
        return None;
    }

    let options = ResolverOptions {
        extensions: config
            .module_file_extensions
            .iter()
            .map(|extension| format!(".{extension}"))
            .collect(),
        conditions: vec!["node".to_string(), "node-addons".to_string()],
        follow_type_only_imports: false,
        require_functions: JEST_REQUIRE_FUNCTIONS
            .iter()
            .map(|f| f.to_string())
            .collect(),
        packages_are_leaves: true,
    };
    let mapper = NameMapper::new(&config.module_name_mapper);
    let rewrite = |specifier: &str| mapper.map(specifier);
    let hooks = ResolverHooks {
        rewrite: Some(&rewrite),
    };
    let graph = resolve_with(&root, &entries, &options, &hooks).ok()?.graph;
    Some(invert(&graph, found, universe))
}

fn invert(
    graph: &ModuleGraph,
    found: &Found,
    universe: &HashSet<&Path>,
) -> HashMap<PathBuf, Vec<PathBuf>> {
    let mut dependents: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for module in graph.modules() {
        let from = graph
            .absolute_path(graph.id_of(&module.path).expect("own module"))
            .as_path()
            .to_path_buf();
        // jest-resolve-dependencies only reads the dependencies of files in
        // the haste map.
        if !universe.contains(from.as_path()) {
            continue;
        }
        let mut add = |to: PathBuf| dependents.entry(to).or_default().push(from.clone());
        for dependency in &module.dependencies {
            match &dependency.target {
                DependencyTarget::Module { id } => {
                    let resolved = graph.absolute_path(*id).as_path().to_path_buf();
                    // The manual mock named like the import, when it is the
                    // one beside the resolved file ("make sure mock is in the
                    // correct directory").
                    let name = Path::new(&dependency.specifier)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if let Some(mock) = found.manual_mocks.get(name) {
                        let beside = resolved.parent().map(|dir| dir.join("__mocks__"));
                        if mock.parent() == beside.as_deref() {
                            add(mock.clone());
                        }
                    }
                    add(resolved);
                }
                DependencyTarget::Unresolved { .. } => {
                    if let Some(mock) = found.manual_mocks.get(&dependency.specifier) {
                        add(mock.clone());
                    }
                }
                // jest-resolve-dependencies skips core modules.
                DependencyTarget::Builtin { .. } => {}
            }
        }
    }
    dependents
}

/// jest-snapshot's default `resolveTestPath`: `__snapshots__/a.test.js.snap`
/// belongs to `a.test.js` beside the `__snapshots__` folder.
fn test_path_for_snapshot(snapshot: &Path) -> PathBuf {
    let name = snapshot
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".snap"))
        .unwrap_or("");
    snapshot
        .parent()
        .and_then(Path::parent)
        .map(|dir| dir.join(name))
        .unwrap_or_default()
}

/// `a/./b/../c` as `a/c`, without touching the filesystem, as path.resolve.
fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_mapper_uses_the_first_match_and_its_groups() {
        let mut map = serde_json::Map::new();
        map.insert("^@/(.*)$".into(), Value::String("/src/$1".into()));
        map.insert("^@/x$".into(), Value::String("/never".into()));
        map.insert(
            "\\.css$".into(),
            Value::Array(vec!["/a.js".into(), "/b.js".into()]),
        );
        let mapper = NameMapper::new(&map);
        assert_eq!(mapper.map("@/x"), Some(vec!["/src/x".to_string()]));
        assert_eq!(
            mapper.map("./a.css"),
            Some(vec!["/a.js".to_string(), "/b.js".to_string()])
        );
        assert_eq!(mapper.map("plain"), None);
    }

    #[test]
    fn test_snapshot_paths_map_to_their_test() {
        assert_eq!(
            test_path_for_snapshot(Path::new("/p/src/__snapshots__/a.test.js.snap")),
            PathBuf::from("/p/src/a.test.js")
        );
    }
}
