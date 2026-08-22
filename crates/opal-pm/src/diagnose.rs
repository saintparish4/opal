//! Explaining an unresolved import.
//!
//! `opal-core` reports a specifier it could not resolve, and stops there — it
//! has no idea whether the package was supposed to be installed. This module
//! joins that report to the `package.json` that declared (or failed to declare)
//! the dependency, which is what separates "an optional peer is absent, as
//! expected" from "this tree is broken".
//!
//! The motivating case: `npm install express` pulls in `debug`, which declares
//! `supports-color` as an optional peer dependency. It is legitimately absent.
//! Without this, `opal graph` reports it exactly like a genuinely missing
//! package.

use std::path::{Path, PathBuf};

use opal_core::graph::resolver::split_bare_specifier;
use opal_core::graph::{DependencyTarget, ModuleGraph};
use opal_core::path::NormalizedPath;

use crate::manifest::{DependencyClass, Manifest};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// Absent on purpose: an optional or peer dependency.
    Informational,
    /// A declared dependency that is not installed, or an import of a package
    /// nothing declared.
    Error,
}

#[derive(Clone, Debug)]
pub struct Unresolved {
    pub importer: NormalizedPath,
    pub specifier: String,
    pub package: String,
    pub class: Option<DependencyClass>,
    pub severity: Severity,
}

impl Unresolved {
    pub fn explain(&self) -> String {
        match (self.class, self.severity) {
            (Some(class), Severity::Informational) => {
                format!("{} is an absent {}", self.package, class.label())
            }
            (Some(class), Severity::Error) => format!(
                "{} is a declared {} but is not installed — run `opal install`",
                self.package,
                class.label()
            ),
            (None, _) => format!(
                "{} is imported by {} but declared by nothing",
                self.package, self.importer
            ),
        }
    }
}

/// Classifies every unresolved edge in a graph.
pub fn classify(graph: &ModuleGraph, project_root: &Path) -> Vec<Unresolved> {
    let mut manifests: Vec<(PathBuf, Option<Manifest>)> = Vec::new();
    let mut findings = Vec::new();

    for (module, dependency) in graph.unresolved() {
        if !matches!(dependency.target, DependencyTarget::Unresolved { .. }) {
            continue;
        }
        // Relative specifiers that do not resolve are a broken file reference,
        // not a packaging question.
        if dependency.specifier.starts_with('.') || dependency.specifier.starts_with('/') {
            continue;
        }
        let (package, _) = split_bare_specifier(&dependency.specifier);

        let importer_directory = project_root.join(
            module
                .path
                .parent()
                .unwrap_or_else(|| NormalizedPath::new("."))
                .as_str(),
        );
        let class = nearest_manifest(&importer_directory, project_root, &mut manifests)
            .and_then(|manifest| manifest.class_of(&package));

        findings.push(Unresolved {
            importer: module.path.clone(),
            specifier: dependency.specifier.clone(),
            package,
            severity: match class {
                Some(class) if class.tolerates_absence() => Severity::Informational,
                _ => Severity::Error,
            },
            class,
        });
    }
    findings
}

/// The `package.json` governing a directory: the closest one at or above it,
/// stopping at the project root.
fn nearest_manifest<'a>(
    directory: &Path,
    project_root: &Path,
    cache: &'a mut Vec<(PathBuf, Option<Manifest>)>,
) -> Option<&'a Manifest> {
    let mut current = Some(directory.to_path_buf());
    let mut found = None;

    while let Some(candidate) = current {
        if candidate.join("package.json").is_file() {
            found = Some(candidate.clone());
            break;
        }
        if candidate == project_root {
            break;
        }
        current = candidate.parent().map(Path::to_path_buf);
    }

    let directory = found?;
    if let Some(index) = cache.iter().position(|(path, _)| *path == directory) {
        return cache[index].1.as_ref();
    }
    let manifest = Manifest::read(&directory.join("package.json")).ok();
    cache.push((directory, manifest));
    cache.last().and_then(|(_, manifest)| manifest.as_ref())
}
