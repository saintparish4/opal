//! `package.json`, read leniently.
//!
//! Every field is optional and every unexpected shape is ignored rather than
//! fatal: npm tarballs in the wild contain manifests with numbers where strings
//! belong and objects where arrays belong, and one of them must not be able to
//! fail an install of an unrelated package. Values that *are* well-formed are
//! taken at face value.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use serde_json::Value;

use crate::semver::{Range, Version};

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: not valid JSON: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

/// How a dependency was declared, which decides whether a missing package is a
/// problem.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DependencyClass {
    Runtime,
    Development,
    Optional,
    Peer,
    OptionalPeer,
}

impl DependencyClass {
    /// Whether an absent package of this class is expected rather than broken.
    pub fn tolerates_absence(self) -> bool {
        matches!(self, Self::Optional | Self::OptionalPeer | Self::Peer)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Runtime => "dependency",
            Self::Development => "devDependency",
            Self::Optional => "optionalDependency",
            Self::Peer => "peerDependency",
            Self::OptionalPeer => "optional peerDependency",
        }
    }
}

/// How a dependency names what it wants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spec {
    Range(Range),
    /// A dist-tag: `"latest"`, `"next"`. Resolved against the packument, not
    /// against version ordering.
    Tag(String),
    /// `git+https://…`, `file:../x`, `npm:alias@^1`. PRD §3 scopes v1 to the
    /// public registry, so these are reported rather than guessed at.
    Unsupported(String),
}

impl Spec {
    pub fn parse(text: &str) -> Self {
        let trimmed = text.trim();
        if let Ok(range) = Range::parse(trimmed) {
            return Self::Range(range);
        }
        let is_tag = !trimmed.is_empty()
            && trimmed
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if is_tag {
            Self::Tag(trimmed.to_string())
        } else {
            Self::Unsupported(trimmed.to_string())
        }
    }
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Range(range) => f.write_str(range.as_str()),
            Self::Tag(tag) => f.write_str(tag),
            Self::Unsupported(text) => f.write_str(text),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Requirement {
    pub name: String,
    pub spec: Spec,
    pub class: DependencyClass,
}

#[derive(Clone, Debug, Default)]
pub struct Manifest {
    pub name: Option<String>,
    pub version: Option<Version>,
    pub requirements: Vec<Requirement>,
    /// `bin` entries, normalized to a map of command name to relative path.
    pub bin: BTreeMap<String, String>,
}

impl Manifest {
    pub fn read(path: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(path).map_err(|source| ManifestError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let value: Value = serde_json::from_str(&text).map_err(|source| ManifestError::Json {
            path: path.display().to_string(),
            source,
        })?;
        Ok(Self::from_value(&value))
    }

    pub fn from_value(value: &Value) -> Self {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string);
        let version = value
            .get("version")
            .and_then(Value::as_str)
            .and_then(|text| Version::parse(text).ok());

        let optional_peers: BTreeSet<String> = value
            .get("peerDependenciesMeta")
            .and_then(Value::as_object)
            .map(|meta| {
                meta.iter()
                    .filter(|(_, entry)| {
                        entry.get("optional").and_then(Value::as_bool) == Some(true)
                    })
                    .map(|(name, _)| name.clone())
                    .collect()
            })
            .unwrap_or_default();

        let mut manifest = Self {
            name,
            version,
            bin: read_bin(value),
            ..Self::default()
        };

        for (field, class) in [
            ("dependencies", DependencyClass::Runtime),
            ("devDependencies", DependencyClass::Development),
            ("optionalDependencies", DependencyClass::Optional),
            ("peerDependencies", DependencyClass::Peer),
        ] {
            let Some(entries) = value.get(field).and_then(Value::as_object) else {
                continue;
            };
            for (name, spec) in entries {
                let Some(spec) = spec.as_str() else { continue };
                let class = match class {
                    DependencyClass::Peer if optional_peers.contains(name) => {
                        DependencyClass::OptionalPeer
                    }
                    other => other,
                };
                manifest.requirements.push(Requirement {
                    name: name.clone(),
                    spec: Spec::parse(spec),
                    class,
                });
            }
        }
        manifest
            .requirements
            .sort_by(|left, right| (left.class, &left.name).cmp(&(right.class, &right.name)));
        manifest
    }

    /// Requirements that an install should fetch: runtime always, dev only for
    /// the project's own manifest, optional best-effort. Peers are never
    /// installed — npm's auto-install-peers behaviour is out of scope for v1.
    pub fn installable(&self, include_development: bool) -> impl Iterator<Item = &Requirement> {
        self.requirements.iter().filter(move |requirement| {
            matches!(
                requirement.class,
                DependencyClass::Runtime | DependencyClass::Optional
            ) || (include_development && requirement.class == DependencyClass::Development)
        })
    }

    pub fn class_of(&self, name: &str) -> Option<DependencyClass> {
        self.requirements
            .iter()
            .find(|requirement| requirement.name == name)
            .map(|requirement| requirement.class)
    }
}

/// `bin` is either a string (one command, named after the package) or a map.
fn read_bin(value: &Value) -> BTreeMap<String, String> {
    let mut bin = BTreeMap::new();
    match value.get("bin") {
        Some(Value::String(path)) => {
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                // A scoped package's command drops the scope: @scope/x -> x.
                let command = name.rsplit('/').next().unwrap_or(name);
                bin.insert(command.to_string(), path.clone());
            }
        }
        Some(Value::Object(entries)) => {
            for (command, path) in entries {
                if let Some(path) = path.as_str() {
                    bin.insert(command.clone(), path.to_string());
                }
            }
        }
        _ => {}
    }
    bin
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: serde_json::Value) -> Manifest {
        Manifest::from_value(&json)
    }

    #[test]
    fn test_reads_dependency_classes() {
        let manifest = manifest(serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { "express": "^4.18.2" },
            "devDependencies": { "jest": "^29.0.0" },
            "optionalDependencies": { "fsevents": "^2.3.0" },
            "peerDependencies": { "react": "^18.0.0", "supports-color": "*" },
            "peerDependenciesMeta": { "supports-color": { "optional": true } }
        }));

        assert_eq!(manifest.name.as_deref(), Some("app"));
        assert_eq!(manifest.class_of("express"), Some(DependencyClass::Runtime));
        assert_eq!(
            manifest.class_of("jest"),
            Some(DependencyClass::Development)
        );
        assert_eq!(
            manifest.class_of("fsevents"),
            Some(DependencyClass::Optional)
        );
        assert_eq!(manifest.class_of("react"), Some(DependencyClass::Peer));
        assert_eq!(
            manifest.class_of("supports-color"),
            Some(DependencyClass::OptionalPeer)
        );
    }

    #[test]
    fn test_installable_excludes_peers_and_optionally_dev() {
        let manifest = manifest(serde_json::json!({
            "dependencies": { "a": "^1.0.0" },
            "devDependencies": { "b": "^1.0.0" },
            "optionalDependencies": { "c": "^1.0.0" },
            "peerDependencies": { "d": "^1.0.0" }
        }));

        let names: Vec<&str> = manifest
            .installable(false)
            .map(|requirement| requirement.name.as_str())
            .collect();
        assert_eq!(names, vec!["a", "c"]);

        let with_dev: Vec<&str> = manifest
            .installable(true)
            .map(|requirement| requirement.name.as_str())
            .collect();
        assert_eq!(with_dev, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_classifies_specs() {
        let manifest = manifest(serde_json::json!({
            "dependencies": {
                "ranged": "^1.0.0",
                "tagged": "latest",
                "from-git": "git+https://example.invalid/x.git",
                "local": "file:../local"
            }
        }));
        let spec = |name: &str| {
            manifest
                .requirements
                .iter()
                .find(|requirement| requirement.name == name)
                .map(|requirement| requirement.spec.clone())
                .unwrap()
        };
        assert!(matches!(spec("ranged"), Spec::Range(_)));
        assert_eq!(spec("tagged"), Spec::Tag("latest".to_string()));
        assert!(matches!(spec("from-git"), Spec::Unsupported(_)));
        assert!(matches!(spec("local"), Spec::Unsupported(_)));
    }

    #[test]
    fn test_survives_wrong_types() {
        let manifest = manifest(serde_json::json!({
            "name": 42,
            "version": "not-a-version",
            "dependencies": { "a": 1, "b": "^1.0.0" },
            "bin": 7
        }));
        assert_eq!(manifest.name, None);
        assert_eq!(manifest.version, None);
        assert_eq!(manifest.requirements.len(), 1);
        assert_eq!(manifest.requirements[0].name, "b");
        assert!(manifest.bin.is_empty());
    }

    #[test]
    fn test_bin_string_and_map_forms() {
        let string_form = manifest(serde_json::json!({
            "name": "@scope/tool",
            "bin": "./cli.js"
        }));
        assert_eq!(
            string_form.bin.get("tool").map(String::as_str),
            Some("./cli.js")
        );

        let map_form = manifest(serde_json::json!({
            "name": "tool",
            "bin": { "tool": "./cli.js", "tool-dev": "./dev.js" }
        }));
        assert_eq!(map_form.bin.len(), 2);
    }
}
