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
    /// The dependency whose own `package.json` declared this, when it was not
    /// the project's. Its own `devDependencies` are categorically not
    /// installable, which is a different thing from missing.
    pub declared_by: Option<String>,
    pub severity: Severity,
}

impl Unresolved {
    pub fn explain(&self) -> String {
        match (self.class, &self.declared_by, self.severity) {
            (Some(DependencyClass::Development), Some(owner), _) => format!(
                "{} is {owner}'s own devDependency, which is never installed",
                self.package
            ),
            (Some(class), _, Severity::Informational) => {
                format!("{} is an absent {}", self.package, class.label())
            }
            (Some(class), _, Severity::Error) => format!(
                "{} is a declared {} but is not installed — run `opal install`",
                self.package,
                class.label()
            ),
            (None, _, _) => format!(
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
        let governing = nearest_manifest(&importer_directory, project_root, &mut manifests);
        let class = governing
            .as_ref()
            .and_then(|found| found.manifest.class_of(&package));
        // Only a nested manifest's own dev tooling; the project's own
        // devDependencies are installable and stay actionable.
        let declared_by = governing.and_then(|found| found.owner);

        findings.push(Unresolved {
            importer: module.path.clone(),
            specifier: dependency.specifier.clone(),
            package,
            severity: match (class, &declared_by) {
                (Some(class), _) if class.tolerates_absence() => Severity::Informational,
                // No package manager installs a dependency's own dev tooling,
                // so reporting it as an error with "run `opal install`" sends
                // the reader to a command that cannot change the outcome.
                (Some(DependencyClass::Development), Some(_)) => Severity::Informational,
                _ => Severity::Error,
            },
            class,
            declared_by,
        });
    }
    findings
}

/// A manifest, and whose it is.
struct Governing<'a> {
    manifest: &'a Manifest,
    /// The dependency's name when the manifest is a dependency's own, and
    /// `None` when it is the project's. What a declaration *means* depends on
    /// which of the two it came from.
    owner: Option<String>,
}

/// The `package.json` governing a directory: the closest one at or above it,
/// stopping at the project root.
fn nearest_manifest<'a>(
    directory: &Path,
    project_root: &Path,
    cache: &'a mut Vec<(PathBuf, Option<Manifest>)>,
) -> Option<Governing<'a>> {
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
    let is_project = directory == project_root;
    let index = match cache.iter().position(|(path, _)| *path == directory) {
        Some(index) => index,
        None => {
            let manifest = Manifest::read(&directory.join("package.json")).ok();
            cache.push((directory, manifest));
            cache.len() - 1
        }
    };

    let manifest = cache[index].1.as_ref()?;
    let owner = (!is_project).then(|| {
        manifest
            .name
            .clone()
            .unwrap_or_else(|| "this dependency".to_string())
    });
    Some(Governing { manifest, owner })
}
