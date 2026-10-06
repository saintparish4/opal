//! `opal.lock`: flat, line-oriented, and sorted.
//!
//! PRD §4.3 asks for a format that is fast to parse and not deeply nested. One
//! line per fact, fields separated by single spaces, every list sorted. Parsing
//! is `split_ascii_whitespace`; diffing it in review shows exactly which
//! dependency moved.
//!
//! ```text
//! opal-lock 3
//! require dependency express - 4.18.2 ^4.18.2
//! require optionalDependency fsevents - - ^2.3.0
//! pkg accepts 1.3.8 sha512-… - - https://registry.npmjs.org/accepts/-/accepts-1.3.8.tgz
//! pkg fsevents 2.3.3 sha512-… darwin x64,arm64 https://registry.npmjs.org/…
//! dep express 4.18.2 accepts - 1.3.8 - ^1.3.8
//! dep @isaacs/cliui 8.0.2 string-width-cjs string-width 4.2.3 - npm:string-width@^4.2.0
//! skip fsevents no version satisfies ^2.3.0
//! ```
//!
//! Any field that can contain a space — a range like `>=1 <2`, a skip reason —
//! is last on its line, so no escaping is needed anywhere. `-` is the empty
//! marker: an unresolved requirement, or an unconstrained `os`/`cpu`.
//!
//! **v2** added the resolved version to `require` and `os`/`cpu` to `pkg`. Both
//! exist so the lockfile alone determines the tree: without the first, the root
//! of each dependency chain has to be guessed from version order, and without
//! the second, platform filtering would have to happen during resolution and
//! the file would stop being portable across platforms.
//!
//! **v3** added the aliased package name to `require` and `dep`, and whether an
//! edge was optional to `dep`. `npm:string-width@^4.2.0` installs one package
//! under another's name, so the name and the package stop being the same fact;
//! and the optional flag is what lets the planner tell npm's two platform
//! outcomes apart — a mismatched optional is skipped, a mismatched requirement
//! is `EBADPLATFORM`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use opal_core::atomic::write_atomic_via;
use opal_core::fault::FaultPoint;

use crate::integrity::Integrity;
use crate::manifest::DependencyClass;
use crate::resolve::{PackageId, RequirementRecord, Resolution, ResolvedEdge, ResolvedPackage};
use crate::semver::Version;

pub const LOCKFILE_NAME: &str = "opal.lock";
pub const LOCKFILE_VERSION: u32 = 3;
/// Stands in for a field with nothing in it, so every line keeps its shape.
const EMPTY: &str = "-";
/// Marks a `dep` edge its dependent tolerates the absence of.
const OPTIONAL: &str = "optional";

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
    #[error("{field} {value:?} contains a line break, which opal.lock cannot represent")]
    Unrepresentable { field: &'static str, value: String },
}

impl LockfileError {
    /// An older lockfile, which `opal install` replaces by re-resolving. A
    /// *newer* one is not this: re-resolving would overwrite a file written by
    /// a build that knows more than this one does.
    pub fn is_outdated_version(&self) -> bool {
        match self {
            Self::Version { found, .. } => found
                .parse::<u32>()
                .is_ok_and(|found| found < LOCKFILE_VERSION),
            _ => false,
        }
    }

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
    write_rendered(path, &render(resolution)?)
}

/// [`write`], for a caller that has to know the lockfile renders before it
/// writes something else that only makes sense alongside it.
pub fn write_rendered(path: &Path, rendered: &str) -> Result<(), LockfileError> {
    write_atomic_via(
        path,
        &temp_path(path),
        rendered.as_bytes(),
        Some(FAULT_BEFORE_RENAME),
    )
    .map_err(|source| LockfileError::io(path, source))
}

/// The file a lockfile at `path` is written to before it is renamed into
/// place: `opal.lock.tmp`, beside it.
///
/// One fixed name, where every other atomic write in opal takes a unique
/// one, because this one lands in the user's project. A kill mid-write
/// leaves it behind, and a name that says whose it is can be cleaned up by
/// the next install; a unique one would sit in `git status` for good. The
/// project lock is what makes one name enough.
pub fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Refuses a value the format cannot hold.
///
/// One line per fact means a value containing a line break does not merely
/// round-trip badly — it *writes another fact*. `Range::parse(">=1\n<2")`
/// succeeds and keeps the newline, so a dependency's `package.json` could put
/// arbitrary lines into the lockfile of every project that installs it, and
/// `--frozen-lockfile` exists precisely to trust that file. No legitimate
/// specifier contains one (`Spec::parse` already trims the ends), so this
/// fails the write rather than escaping into a format that has no escaping.
fn representable<'a>(field: &'static str, value: &'a str) -> Result<&'a str, LockfileError> {
    if value.contains(['\n', '\r']) {
        return Err(LockfileError::Unrepresentable {
            field,
            value: value.to_string(),
        });
    }
    Ok(value)
}

pub fn render(resolution: &Resolution) -> Result<String, LockfileError> {
    let mut out = format!("opal-lock {LOCKFILE_VERSION}\n");

    for requirement in &resolution.requirements {
        out.push_str(&format!(
            "require {} {} {} {} {}\n",
            class_name(requirement.class),
            representable("package name", &requirement.name)?,
            aliased(&requirement.name, &requirement.package),
            requirement
                .version
                .as_ref()
                .map_or_else(|| EMPTY.to_string(), ToString::to_string),
            representable("specifier", &requirement.spec)?
        ));
    }
    for package in resolution.packages.values() {
        out.push_str(&format!(
            "pkg {} {} {} {} {} {}\n",
            representable("package name", &package.id.name)?,
            package.id.version,
            package.integrity,
            representable("os constraint", &render_list(&package.os))?,
            representable("cpu constraint", &render_list(&package.cpu))?,
            representable("tarball URL", &package.tarball)?
        ));
    }
    for package in resolution.packages.values() {
        for edge in &package.dependencies {
            out.push_str(&format!(
                "dep {} {} {} {} {} {} {}\n",
                representable("package name", &package.id.name)?,
                package.id.version,
                representable("package name", &edge.name)?,
                aliased(&edge.name, &edge.package),
                edge.version,
                if edge.optional { OPTIONAL } else { EMPTY },
                representable("specifier", &edge.spec)?
            ));
        }
    }
    let mut skipped = resolution.skipped.clone();
    skipped.sort();
    for (name, reason) in skipped {
        out.push_str(&format!(
            "skip {} {}\n",
            representable("package name", &name)?,
            representable("skip reason", &reason)?
        ));
    }
    Ok(out)
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
        // `str::lines` strips one trailing `\r`; a value that itself ends in
        // one would otherwise lose a character per round trip, so take them
        // all and make parsing idempotent.
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let mut fields = line.splitn(2, ' ');
        let kind = fields.next().unwrap_or_default();
        let rest = fields.next().unwrap_or_default();

        match kind {
            "require" => {
                let [class, name, package, version, spec] =
                    split_n::<5>(rest).ok_or_else(|| {
                        malformed(
                            number,
                            "expected: require <class> <name> <package> <version> <spec>",
                        )
                    })?;
                let version = match version {
                    EMPTY => None,
                    text => Some(
                        Version::parse(text)
                            .map_err(|error| malformed(number, &error.to_string()))?,
                    ),
                };
                resolution.requirements.push(RequirementRecord {
                    class: parse_class(class)
                        .ok_or_else(|| malformed(number, "unknown dependency class"))?,
                    name: name.to_string(),
                    package: unaliased(name, package),
                    spec: spec.to_string(),
                    version,
                });
            }
            "pkg" => {
                let [name, version, integrity, os, cpu, tarball] =
                    split_n::<6>(rest).ok_or_else(|| {
                        malformed(
                            number,
                            "expected: pkg <name> <version> <integrity> <os> <cpu> <tarball>",
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
                        os: parse_list(os),
                        cpu: parse_list(cpu),
                    },
                );
            }
            "dep" => {
                let [
                    name,
                    version,
                    dep_name,
                    dep_package,
                    dep_version,
                    optional,
                    spec,
                ] = split_n::<7>(rest).ok_or_else(|| {
                    malformed(
                        number,
                        "expected: dep <name> <version> <dep-name> <dep-package> \
                             <dep-version> <optional> <spec>",
                    )
                })?;
                let parent = PackageId::new(
                    name,
                    Version::parse(version)
                        .map_err(|error| malformed(number, &error.to_string()))?,
                );
                let edge = ResolvedEdge {
                    name: dep_name.to_string(),
                    package: unaliased(dep_name, dep_package),
                    spec: spec.to_string(),
                    version: Version::parse(dep_version)
                        .map_err(|error| malformed(number, &error.to_string()))?,
                    optional: optional == OPTIONAL,
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

    // Parsing is a canonicalizer, not a transcription. `render` sorts, so a
    // parse that kept file order would make `parse(render(x)) != x` for any
    // hand-edited or hand-written lockfile — and a format whose parse is not a
    // fixed point of its render cannot be compared, diffed, or reasoned about.
    // The orders here are the ones `resolve` and `render` produce.
    resolution
        .requirements
        .sort_by(|left, right| (left.class, &left.name).cmp(&(right.class, &right.name)));
    resolution.skipped.sort();
    for package in resolution.packages.values_mut() {
        package
            .dependencies
            .sort_by(|left, right| (&left.name, &left.spec).cmp(&(&right.name, &right.spec)));
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

/// The package installed under a name, or `-` when they are the same — which
/// they are for everything except an alias.
fn aliased(name: &str, package: &str) -> String {
    if name == package {
        EMPTY.to_string()
    } else {
        package.to_string()
    }
}

/// Comma-separated, or `-` when there is nothing to constrain. npm's values
/// never contain a comma or a space, so no escaping is needed.
fn render_list(items: &[String]) -> String {
    if items.is_empty() {
        EMPTY.to_string()
    } else {
        items.join(",")
    }
}

fn unaliased(name: &str, package: &str) -> String {
    if package == EMPTY {
        name.to_string()
    } else {
        package.to_string()
    }
}

fn parse_list(text: &str) -> Vec<String> {
    if text == EMPTY {
        return Vec::new();
    }
    text.split(',')
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
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
                    package: "accepts".to_string(),
                    spec: ">=1.3.0 <2".to_string(),
                    version: Version::new(1, 3, 8),
                    optional: false,
                }],
                os: Vec::new(),
                cpu: Vec::new(),
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
                os: vec!["darwin".to_string(), "!win32".to_string()],
                cpu: vec!["x64".to_string(), "arm64".to_string()],
            },
        );

        Resolution {
            requirements: vec![
                RequirementRecord {
                    class: DependencyClass::Runtime,
                    name: "express".to_string(),
                    package: "express".to_string(),
                    spec: "^4.18.2".to_string(),
                    version: Some(Version::new(4, 18, 2)),
                },
                RequirementRecord {
                    class: DependencyClass::Optional,
                    name: "fsevents".to_string(),
                    package: "fsevents".to_string(),
                    spec: "^2.3.0".to_string(),
                    version: None,
                },
            ],
            packages,
            skipped: vec![("fsevents".to_string(), "not in the registry".to_string())],
        }
    }

    #[test]
    fn test_round_trips() {
        let resolution = sample();
        let text = render(&resolution).expect("renderable");
        let parsed = parse(Path::new("opal.lock"), &text).unwrap();
        assert_eq!(parsed, resolution);
    }

    #[test]
    fn test_render_is_stable_and_sorted() {
        let text = render(&sample()).expect("renderable");
        let kinds: Vec<&str> = text
            .lines()
            .skip(1)
            .map(|line| line.split(' ').next().unwrap())
            .collect();
        assert_eq!(
            kinds,
            vec!["require", "require", "pkg", "pkg", "dep", "skip"]
        );
        // BTreeMap ordering puts accepts before express, whatever order they
        // were resolved in.
        assert!(text.contains("\npkg accepts 1.3.8 "));
        assert!(text.contains(" darwin,!win32 x64,arm64 "));
        assert_eq!(render(&sample()).unwrap(), text);
    }

    #[test]
    fn test_ranges_containing_spaces_survive() {
        let text = render(&sample()).expect("renderable");
        assert!(text.contains("dep express 4.18.2 accepts - 1.3.8 - >=1.3.0 <2\n"));
        let parsed = parse(Path::new("opal.lock"), &text).unwrap();
        let express = parsed
            .package(&PackageId::new("express", Version::new(4, 18, 2)))
            .unwrap();
        assert_eq!(express.dependencies[0].spec, ">=1.3.0 <2");
    }

    #[test]
    fn test_a_line_break_in_a_value_is_refused_rather_than_written() {
        // `Range::parse(">=1\n<2")` succeeds and keeps the newline, so without
        // this a dependency's own package.json could append lines to the
        // lockfile of every project that installs it — and `--frozen-lockfile`
        // exists to trust that file.
        let mut resolution = sample();
        resolution.requirements[0].spec =
            ">=1\npkg backdoor 1.0.0 sha512-Zm9vYmFy - - http://attacker.invalid/x.tgz".to_string();

        let error = render(&resolution).expect_err("a second fact is not a specifier");
        assert!(
            matches!(error, LockfileError::Unrepresentable { .. }),
            "{error}"
        );
    }

    #[test]
    fn test_a_carriage_return_is_refused_rather_than_silently_trimmed() {
        // Found by `cargo fuzz run lockfile`: `str::lines` strips one trailing
        // `\r`, so a value ending in one lost a character on every write.
        // Refusing it beats the trimming that hid it.
        let mut resolution = sample();
        resolution.skipped = vec![("demo".to_string(), "no version satisfies ^1\r".to_string())];

        let error = render(&resolution).expect_err("a value the format mangles is not writable");
        assert!(
            matches!(error, LockfileError::Unrepresentable { .. }),
            "{error}"
        );
    }

    #[test]
    fn test_a_lockfile_with_windows_line_endings_parses() {
        // Nothing opal writes looks like this, but a checkout with
        // `core.autocrlf` on does, and refusing to read it would be refusing to
        // read a file this build wrote.
        let text = render(&sample()).expect("renderable").replace('\n', "\r\n");
        let parsed = parse(Path::new("opal.lock"), &text).expect("CRLF parses");
        assert_eq!(parsed, sample());
    }

    #[test]
    fn test_parsing_canonicalises_order() {
        // Found by `cargo fuzz run lockfile`: render sorts and parse did not,
        // so a lockfile whose lines arrived in another order parsed into a
        // resolution that did not survive being written back out.
        let text = "opal-lock 3\nskip zeta gone\nskip alpha gone\n";
        let parsed = parse(Path::new("opal.lock"), text).expect("parses");
        let again = parse(
            Path::new("opal.lock"),
            &render(&parsed).expect("renderable"),
        )
        .expect("parses");

        assert_eq!(parsed, again);
        assert_eq!(parsed.skipped[0].0, "alpha");
    }

    #[test]
    fn test_rejects_a_future_version() {
        let error = parse(Path::new("opal.lock"), "opal-lock 99\n").unwrap_err();
        assert!(matches!(error, LockfileError::Version { .. }));
        // Re-resolving would overwrite a lockfile written by a build that
        // knows more than this one does.
        assert!(!error.is_outdated_version());
    }

    #[test]
    fn test_an_older_version_is_replaceable_rather_than_fatal() {
        let error = parse(Path::new("opal.lock"), "opal-lock 1\n").unwrap_err();
        assert!(error.is_outdated_version());
        let garbage = parse(Path::new("opal.lock"), "opal-lock v1\n").unwrap_err();
        assert!(!garbage.is_outdated_version());
    }

    #[test]
    fn test_a_requirement_without_a_resolution_round_trips() {
        let text = render(&sample()).expect("renderable");
        assert!(text.contains("require optionalDependency fsevents - - ^2.3.0\n"));
        let parsed = parse(Path::new("opal.lock"), &text).unwrap();
        assert_eq!(parsed.requirements[1].version, None);
        assert_eq!(parsed.requirements[0].version, Some(Version::new(4, 18, 2)));
    }

    #[test]
    fn test_platform_constraints_round_trip() {
        let parsed = parse(Path::new("opal.lock"), &render(&sample()).unwrap()).unwrap();
        let accepts = parsed
            .package(&PackageId::new("accepts", Version::new(1, 3, 8)))
            .unwrap();
        assert_eq!(accepts.os, vec!["darwin".to_string(), "!win32".to_string()]);
        assert_eq!(accepts.cpu, vec!["x64".to_string(), "arm64".to_string()]);

        let express = parsed
            .package(&PackageId::new("express", Version::new(4, 18, 2)))
            .unwrap();
        assert!(express.os.is_empty() && express.cpu.is_empty());
    }

    #[test]
    fn test_rejects_malformed_lines() {
        for text in [
            "opal-lock 3\npkg only-a-name\n",
            "opal-lock 3\nnonsense a b\n",
            "opal-lock 3\ndep ghost 1.0.0 x - 1.0.0 - ^1\n",
            "opal-lock 3\nrequire dependency a ^1\n",
            "opal-lock 3\nrequire dependency a - not-a-version ^1\n",
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
