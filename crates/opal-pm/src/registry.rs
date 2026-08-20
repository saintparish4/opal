//! npm registry client.
//!
//! Speaks the registry protocol over `https://` and `file://`. The second scheme
//! is not a convenience: it lets the entire install pipeline — including the
//! crash-safety and concurrency suites — run against a fixture registry with no
//! network and no HTTP server, exercising exactly the code path production uses.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::rc::Rc;

use opal_core::fault::{self, FaultPoint};
use serde_json::Value;

use crate::integrity::Integrity;
use crate::manifest::Manifest;
use crate::semver::Version;

/// Part of the tarball is on the wire; the rest is not.
pub const FAULT_MID_DOWNLOAD: FaultPoint = FaultPoint::new("pm-mid-download");
pub const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org";
/// Points the client at another registry — a fixture directory, in tests.
pub const REGISTRY_ENV: &str = "OPAL_REGISTRY";

const CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("{url}: {message}")]
    Transport { url: String, message: String },
    #[error("{url}: HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("package {0:?} is not in the registry")]
    NotFound(String),
    #[error("{url}: registry response is not valid JSON: {source}")]
    Json {
        url: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("package {name:?} has no version matching {tag:?}")]
    UnknownTag { name: String, tag: String },
}

#[derive(Clone, Debug)]
pub struct VersionMetadata {
    pub version: Version,
    pub tarball: String,
    pub integrity: Integrity,
    /// Dependencies as declared by that published version.
    pub manifest: Manifest,
    pub deprecated: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Packument {
    pub name: String,
    pub versions: BTreeMap<Version, VersionMetadata>,
    pub dist_tags: BTreeMap<String, Version>,
}

impl Packument {
    pub fn parse(name: &str, value: &Value) -> Self {
        let mut packument = Self {
            name: name.to_string(),
            versions: BTreeMap::new(),
            dist_tags: BTreeMap::new(),
        };

        if let Some(entries) = value.get("versions").and_then(Value::as_object) {
            for (text, entry) in entries {
                // A version the registry lists but Opal cannot parse is skipped
                // rather than fatal: one malformed entry must not make a
                // package uninstallable.
                let Ok(version) = Version::parse(text) else {
                    continue;
                };
                let Some(distribution) = entry.get("dist") else {
                    continue;
                };
                let Some(tarball) = distribution.get("tarball").and_then(Value::as_str) else {
                    continue;
                };
                let integrity = distribution
                    .get("integrity")
                    .and_then(Value::as_str)
                    .and_then(|text| Integrity::parse(text).ok())
                    .or_else(|| {
                        // Packages published before 2017 carry only a shasum.
                        distribution
                            .get("shasum")
                            .and_then(Value::as_str)
                            .and_then(|hex| Integrity::from_shasum(hex).ok())
                    });
                let Some(integrity) = integrity else {
                    continue;
                };

                packument.versions.insert(
                    version.clone(),
                    VersionMetadata {
                        version,
                        tarball: tarball.to_string(),
                        integrity,
                        manifest: Manifest::from_value(entry),
                        deprecated: entry
                            .get("deprecated")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    },
                );
            }
        }

        if let Some(tags) = value.get("dist-tags").and_then(Value::as_object) {
            for (tag, text) in tags {
                if let Some(version) = text.as_str().and_then(|text| Version::parse(text).ok()) {
                    packument.dist_tags.insert(tag.clone(), version);
                }
            }
        }
        packument
    }

    pub fn version(&self, version: &Version) -> Option<&VersionMetadata> {
        self.versions.get(version)
    }
}

pub trait Registry {
    fn packument(&self, name: &str) -> Result<Rc<Packument>, RegistryError>;
    fn tarball(&self, url: &str) -> Result<Vec<u8>, RegistryError>;
}

/// The real client, with an in-process packument cache.
///
/// Single-threaded on purpose for v1: correctness before speed, and parallel
/// downloads are a change that needs the benchmark suite to justify it.
pub struct NpmRegistry {
    base: String,
    cache: RefCell<HashMap<String, Rc<Packument>>>,
}

impl NpmRegistry {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// `$OPAL_REGISTRY`, else the public registry.
    pub fn discover() -> Self {
        Self::new(std::env::var(REGISTRY_ENV).unwrap_or_else(|_| DEFAULT_REGISTRY.to_string()))
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn packument_url(&self, name: &str) -> String {
        if self.base.starts_with("file://") {
            // Fixture layout: one JSON file per package, scopes as directories.
            format!("{}/{name}.json", self.base)
        } else {
            // The registry wants the scope separator escaped.
            format!("{}/{}", self.base, name.replace('/', "%2f"))
        }
    }

    fn fetch(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        match url.strip_prefix("file://") {
            Some(path) => read_file(url, path),
            None => read_http(url),
        }
    }
}

impl Registry for NpmRegistry {
    fn packument(&self, name: &str) -> Result<Rc<Packument>, RegistryError> {
        if let Some(cached) = self.cache.borrow().get(name) {
            return Ok(Rc::clone(cached));
        }
        let url = self.packument_url(name);
        let bytes = match self.fetch(&url) {
            Ok(bytes) => bytes,
            Err(RegistryError::Status { status: 404, .. }) => {
                return Err(RegistryError::NotFound(name.to_string()));
            }
            Err(error) => return Err(error),
        };
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|source| RegistryError::Json {
                url: url.clone(),
                source,
            })?;

        let packument = Rc::new(Packument::parse(name, &value));
        self.cache
            .borrow_mut()
            .insert(name.to_string(), Rc::clone(&packument));
        Ok(packument)
    }

    fn tarball(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        self.fetch(url)
    }
}

fn read_file(url: &str, path: &str) -> Result<Vec<u8>, RegistryError> {
    let file = std::fs::File::open(path).map_err(|source| match source.kind() {
        std::io::ErrorKind::NotFound => RegistryError::Status {
            url: url.to_string(),
            status: 404,
        },
        _ => RegistryError::Transport {
            url: url.to_string(),
            message: source.to_string(),
        },
    })?;
    read_chunked(url, file)
}

fn read_http(url: &str) -> Result<Vec<u8>, RegistryError> {
    let mut response = ureq::get(url).call().map_err(|error| match &error {
        ureq::Error::StatusCode(status) => RegistryError::Status {
            url: url.to_string(),
            status: *status,
        },
        _ => RegistryError::Transport {
            url: url.to_string(),
            message: error.to_string(),
        },
    })?;
    let status = response.status().as_u16();
    if status >= 400 {
        return Err(RegistryError::Status {
            url: url.to_string(),
            status,
        });
    }
    read_chunked(url, response.body_mut().as_reader())
}

/// Reads in chunks so the download has an interruptible midpoint.
fn read_chunked(url: &str, mut reader: impl Read) -> Result<Vec<u8>, RegistryError> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0u8; CHUNK_BYTES];
    let mut first = true;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|source| RegistryError::Transport {
                url: url.to_string(),
                message: source.to_string(),
            })?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if first {
            first = false;
            fault::checkpoint(FAULT_MID_DOWNLOAD);
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        serde_json::json!({
            "name": "demo",
            "dist-tags": { "latest": "1.2.0", "next": "2.0.0-rc.1" },
            "versions": {
                "1.0.0": {
                    "version": "1.0.0",
                    "dependencies": { "left-pad": "^1.0.0" },
                    "dist": { "tarball": "https://example.invalid/demo-1.0.0.tgz", "integrity": "sha512-Zm9vYmFy" }
                },
                "1.2.0": {
                    "version": "1.2.0",
                    "dist": { "tarball": "https://example.invalid/demo-1.2.0.tgz", "shasum": "0beec7b5ea3f0fdbc95d0dd47f3c5bc275da8a33" }
                },
                "not-a-version": { "dist": { "tarball": "x", "integrity": "sha512-Zm9vYmFy" } },
                "1.3.0": { "version": "1.3.0" }
            }
        })
    }

    #[test]
    fn test_parses_versions_and_tags() {
        let packument = Packument::parse("demo", &sample());
        assert_eq!(packument.versions.len(), 2);
        assert_eq!(
            packument.dist_tags.get("latest").map(ToString::to_string),
            Some("1.2.0".to_string())
        );

        let metadata = packument.version(&Version::new(1, 0, 0)).unwrap();
        assert_eq!(metadata.manifest.requirements.len(), 1);
        assert_eq!(metadata.tarball, "https://example.invalid/demo-1.0.0.tgz");
    }

    #[test]
    fn test_skips_unusable_entries_rather_than_failing() {
        let packument = Packument::parse("demo", &sample());
        // "not-a-version" is unparseable and "1.3.0" has no dist; neither can be
        // installed, and neither prevents installing the rest.
        assert!(packument.version(&Version::new(1, 3, 0)).is_none());
    }

    #[test]
    fn test_falls_back_to_shasum_when_integrity_is_absent() {
        let packument = Packument::parse("demo", &sample());
        let metadata = packument.version(&Version::new(1, 2, 0)).unwrap();
        assert!(metadata.integrity.to_string().starts_with("sha1-"));
    }

    #[test]
    fn test_packument_url_shapes() {
        let http = NpmRegistry::new("https://registry.npmjs.org/");
        assert_eq!(
            http.packument_url("@scope/pkg"),
            "https://registry.npmjs.org/@scope%2fpkg"
        );
        let file = NpmRegistry::new("file:///tmp/fixture");
        assert_eq!(
            file.packument_url("@scope/pkg"),
            "file:///tmp/fixture/@scope/pkg.json"
        );
    }
}