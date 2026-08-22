//! `opal.lock`: flat, line-oriented, and sorted.
//!
//! PRD §4.3 asks for a format that is fast to parse and not deeply nested. One
//! line per fact, fields separated by single spaces, every list sorted. Parsing
//! is `split_ascii_whitespace`; diffing it in review shows exactly which
//! dependency moved.
//!
//! ```text
//! opal-lock 1
//! require dependency express ^4.18.2
//! pkg accepts 1.3.8 sha512-… https://registry.npmjs.org/accepts/-/accepts-1.3.8.tgz
//! dep express 4.18.2 accepts 1.3.8 ^1.3.8
//! skip fsevents no version satisfies ^2.3.0
//! ```
//!
//! Any field that can contain a space — a range like `>=1 <2`, a skip reason —
//! is last on its line, so no escaping is needed anywhere.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use opal_core::atomic::write_atomic;
use opal_core::fault::FaultPoint;

use crate::integrity::Integrity;
use crate::manifest::DependencyClass;
use crate::resolve::{PackageId, RequirementRecord, Resolution, ResolvedEdge, ResolvedPackage};
use crate::semver::Version;

pub const LOCKFILE_NAME: &str = "opal.lock";
pub const LOCKFILE_VERSION: u32 = 1;

/// The new lockfile is written and fsynced; the rename over the old one has not
/// happened yet.
pub const FAULT_BEFORE_RENAME: FaultPoint = FaultPoint::new("pm-before-lockfile-rename");

#[derive(Debug, thiserror::Error)]
pub enum LockfileError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: line {line}: {message}")]
    Malformed {
        path: PathBuf,
        line: usize,
        message: String,
    },
    #[error("{path}: lockfile version {found}, this build writes v{LOCKFILE_VERSION}")]
    Version { path: PathBuf, found: String },
}

impl LockfileError {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

pub fn path_in(project_root: &Path) -> PathBuf {
    project_root.join(LOCKFILE_NAME)
}

/// Reads a lockfile, or `None` if there is not one yet.
pub fn read(path: &Path) -> Result<Option<Resolution>, LockfileError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(LockfileError::io(path, source)),
    };
    parse(path, &text).map(Some)
}

/// Writes atomically: a crash mid-resolution leaves the previous lockfile
/// untouched, never a torn one.
pub fn write(path: &Path, resolution: &Resolution) -> Result<(), LockfileError> {
    write_atomic(
        path,
        render(resolution).as_bytes(),
        Some(FAULT_BEFORE_RENAME),
    )
    .map_err(|source| LockfileError::io(path, source))
}

pub fn render(resolution: &Resolution) -> String {
    let mut out = format!("opal-lock {LOCKFILE_VERSION}\n");

    for requirement in &resolution.requirements {
        out.push_str(&format!(
            "require {} {} {}\n",
            class_name(requirement.class),
            requirement.name,
            requirement.spec
        ));
    }
    for package in resolution.packages.values() {
        out.push_str(&format!(
            "pkg {} {} {} {}\n",
            package.id.name, package.id.version, package.integrity, package.tarball
        ));
    }
    for package in resolution.packages.values() {
        for edge in &package.dependencies {
            out.push_str(&format!(
                "dep {} {} {} {} {}\n",
                package.id.name, package.id.version, edge.name, edge.version, edge.spec
            ));
        }
    }
    let mut skipped = resolution.skipped.clone();
    skipped.sort();
    for (name, reason) in skipped {
        out.push_str(&format!("skip {name} {reason}\n"));
    }
    out
}

pub fn parse(path: &Path, text: &str) -> Result<Resolution, LockfileError> {
    let mut lines = text.lines().enumerate();
    let malformed = |line: usize, message: &str| LockfileError::Malformed {
        path: path.to_path_buf(),
        line,
        message: message.to_string(),
    };

    match lines.next() {
        Some((_, header)) => {
            let found = header.strip_prefix("opal-lock ").unwrap_or(header).trim();
            if found != LOCKFILE_VERSION.to_string() {
                return Err(LockfileError::Version {
                    path: path.to_path_buf(),
                    found: found.to_string(),
                });
            }
        }
        None => return Err(malformed(0, "empty lockfile")),
    }

    let mut resolution = Resolution {
        requirements: Vec::new(),
        packages: BTreeMap::new(),
        skipped: Vec::new(),
    };
    let mut edges: Vec<(PackageId, ResolvedEdge)> = Vec::new();

    for (index, line) in lines {
        let number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let mut fields = line.splitn(2, ' ');
        let kind = fields.next().unwrap_or_default();
        let rest = fields.next().unwrap_or_default();

        match kind {
            "require" => {
                let [class, name, spec] = split_n::<3>(rest)
                    .ok_or_else(|| malformed(number, "expected: require <class> <name> <spec>"))?;
                resolution.requirements.push(RequirementRecord {
                    class: parse_class(class)
                        .ok_or_else(|| malformed(number, "unknown dependency class"))?,
                    name: name.to_string(),
                    spec: spec.to_string(),
                });
            }
            "pkg" => {
                let [name, version, integrity, tarball] = split_n::<4>(rest).ok_or_else(|| {
                    malformed(
                        number,
                        "expected: pkg <name> <version> <integrity> <tarball>",
                    )
                })?;
                let version = Version::parse(version)
                    .map_err(|error| malformed(number, &error.to_string()))?;
                let integrity = Integrity::parse(integrity)
                    .map_err(|error| malformed(number, &error.to_string()))?;
                let id = PackageId::new(name, version);
                resolution.packages.insert(
                    id.clone(),
                    ResolvedPackage {
                        id,
                        tarball: tarball.to_string(),
                        integrity,
                        dependencies: Vec::new(),
                    },
                );
            }
            "dep" => {
                let [name, version, dep_name, dep_version, spec] =
                    split_n::<5>(rest).ok_or_else(|| {
                        malformed(
                            number,
                            "expected: dep <name> <version> <dep-name> <dep-version> <spec>",
                        )
                    })?;
                let parent = PackageId::new(
                    name,
                    Version::parse(version)
                        .map_err(|error| malformed(number, &error.to_string()))?,
                );
                let edge = ResolvedEdge {
                    name: dep_name.to_string(),
                    spec: spec.to_string(),
                    version: Version::parse(dep_version)
                        .map_err(|error| malformed(number, &error.to_string()))?,
                };
                edges.push((parent, edge));
            }
            "skip" => {
                let [name, reason] = split_n::<2>(rest)
                    .ok_or_else(|| malformed(number, "expected: skip <name> <reason>"))?;
                resolution
                    .skipped
                    .push((name.to_string(), reason.to_string()));
            }
            other => return Err(malformed(number, &format!("unknown entry {other:?}"))),
        }
    }

    for (parent, edge) in edges {
        let package = resolution.packages.get_mut(&parent).ok_or_else(|| {
            malformed(0, &format!("dep line references unknown package {parent}"))
        })?;
        package.dependencies.push(edge);
    }
    Ok(resolution)
}

/// Splits into exactly `N` fields, with the last one absorbing any remainder.
fn split_n<const N: usize>(text: &str) -> Option<[&str; N]> {
    let mut fields = [""; N];
    let mut rest = text;
    for slot in fields.iter_mut().take(N - 1) {
        let (head, tail) = rest.split_once(' ')?;
        if head.is_empty() {
            return None;
        }
        *slot = head;
        rest = tail;
    }
    if rest.is_empty() {
        return None;
    }
    fields[N - 1] = rest;
    Some(fields)
}

fn class_name(class: DependencyClass) -> &'static str {
    match class {
        DependencyClass::Runtime => "dependency",
        DependencyClass::Development => "devDependency",
        DependencyClass::Optional => "optionalDependency",
        DependencyClass::Peer => "peerDependency",
        DependencyClass::OptionalPeer => "optionalPeerDependency",
    }
}

fn parse_class(text: &str) -> Option<DependencyClass> {
    Some(match text {
        "dependency" => DependencyClass::Runtime,
        "devDependency" => DependencyClass::Development,
        "optionalDependency" => DependencyClass::Optional,
        "peerDependency" => DependencyClass::Peer,
        "optionalPeerDependency" => DependencyClass::OptionalPeer,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity::Algorithm;

    fn sample() -> Resolution {
        let mut packages = BTreeMap::new();
        let express = PackageId::new("express", Version::new(4, 18, 2));
        packages.insert(
            express.clone(),
            ResolvedPackage {
                id: express,
                tarball: "https://registry.example/express-4.18.2.tgz".to_string(),
                integrity: Integrity::of(Algorithm::Sha512, b"express"),
                dependencies: vec![ResolvedEdge {
                    name: "accepts".to_string(),
                    spec: ">=1.3.0 <2".to_string(),
                    version: Version::new(1, 3, 8),
                }],
            },
        );
        let accepts = PackageId::new("accepts", Version::new(1, 3, 8));
        packages.insert(
            accepts.clone(),
            ResolvedPackage {
                id: accepts,
                tarball: "https://registry.example/accepts-1.3.8.tgz".to_string(),
                integrity: Integrity::of(Algorithm::Sha512, b"accepts"),
                dependencies: Vec::new(),
            },
        );

        Resolution {
            requirements: vec![RequirementRecord {
                class: DependencyClass::Runtime,
                name: "express".to_string(),
                spec: "^4.18.2".to_string(),
            }],
            packages,
            skipped: vec![("fsevents".to_string(), "not in the registry".to_string())],
        }
    }

    #[test]
    fn test_round_trips() {
        let resolution = sample();
        let text = render(&resolution);
        let parsed = parse(Path::new("opal.lock"), &text).unwrap();
        assert_eq!(parsed, resolution);
    }

    #[test]
    fn test_render_is_stable_and_sorted() {
        let text = render(&sample());
        let kinds: Vec<&str> = text
            .lines()
            .skip(1)
            .map(|line| line.split(' ').next().unwrap())
            .collect();
        assert_eq!(kinds, vec!["require", "pkg", "pkg", "dep", "skip"]);
        // BTreeMap ordering puts accepts before express, whatever order they
        // were resolved in.
        assert!(text.contains("\npkg accepts 1.3.8 "));
        assert_eq!(render(&sample()), text);
    }

    #[test]
    fn test_ranges_containing_spaces_survive() {
        let text = render(&sample());
        assert!(text.contains("dep express 4.18.2 accepts 1.3.8 >=1.3.0 <2\n"));
        let parsed = parse(Path::new("opal.lock"), &text).unwrap();
        let express = parsed
            .package(&PackageId::new("express", Version::new(4, 18, 2)))
            .unwrap();
        assert_eq!(express.dependencies[0].spec, ">=1.3.0 <2");
    }

    #[test]
    fn test_rejects_a_future_version() {
        let error = parse(Path::new("opal.lock"), "opal-lock 99\n").unwrap_err();
        assert!(matches!(error, LockfileError::Version { .. }));
    }

    #[test]
    fn test_rejects_malformed_lines() {
        for text in [
            "opal-lock 1\npkg only-a-name\n",
            "opal-lock 1\nnonsense a b\n",
            "opal-lock 1\ndep ghost 1.0.0 x 1.0.0 ^1\n",
        ] {
            assert!(parse(Path::new("opal.lock"), text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn test_write_then_read_from_disk() {
        let directory = tempfile::tempdir().unwrap();
        let path = path_in(directory.path());
        assert!(read(&path).unwrap().is_none());

        write(&path, &sample()).unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), sample());
    }
}
