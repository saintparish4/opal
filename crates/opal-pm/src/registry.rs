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
use std::time::{Duration, SystemTime};

use opal_core::fault::{self, FaultPoint};
use serde_json::Value;

use crate::integrity::Integrity;
use crate::manifest::Manifest;
use crate::packuments::{self, PackumentCache, Record};
use crate::semver::Version;

/// Part of the tarball is on the wire; the rest is not.
pub const FAULT_MID_DOWNLOAD: FaultPoint = FaultPoint::new("pm-mid-download");
pub const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org";
/// Points the client at another registry — a fixture directory, in tests.
pub const REGISTRY_ENV: &str = "OPAL_REGISTRY";

const CHUNK_BYTES: usize = 64 * 1024;

/// Asks for the install subset of a packument rather than every README of every
/// version ever published. Roughly half the bytes on a typical package, and the
/// fields an install actually reads — `dependencies`, `dist`, `bin`, `os`,
/// `cpu`, `deprecated`, `peerDependenciesMeta` — are all in it.
pub const ABBREVIATED_PACKUMENT: &str = "application/vnd.npm.install-v1+json";

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
    #[error("{0} is not in the local cache, and this install is offline")]
    NotCached(String),
}

/// One request to whatever is standing in for the network.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub url: &'a str,
    /// Set for metadata, absent for tarballs — which is also how a caller can
    /// tell the two apart without parsing the URL.
    pub accept: Option<&'a str>,
    /// Revalidates rather than re-downloads when the server agrees.
    pub etag: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct Response {
    pub body: Vec<u8>,
    pub etag: Option<String>,
    /// From `Cache-Control`, when the server sent one.
    pub max_age: Option<Duration>,
}

#[derive(Clone, Debug)]
pub enum Fetched {
    Fresh(Response),
    /// The server confirmed the cached copy is still good.
    NotModified,
}

/// Everything below the client: the one place bytes actually move.
///
/// A seam rather than a hard-coded `ureq` call so the benchmark can price the
/// wire *underneath* the metadata cache. Metering above it would charge a cache
/// hit for a round trip it never made, and then no amount of caching could show
/// up as an improvement.
pub trait Transport {
    fn get(&self, request: &Request<'_>) -> Result<Fetched, RegistryError>;
}

/// `https://` through `ureq`, `file://` straight off the disk.
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpTransport;

impl Transport for HttpTransport {
    fn get(&self, request: &Request<'_>) -> Result<Fetched, RegistryError> {
        match request.url.strip_prefix("file://") {
            // A local file has no validators and no staleness worth modelling.
            Some(path) => Ok(Fetched::Fresh(Response {
                body: read_file(request.url, path)?,
                etag: None,
                max_age: None,
            })),
            None => read_http(request),
        }
    }
}

/// What to do when a cached packument is not fresh.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Freshness {
    /// Revalidate against the registry, which is one small round trip rather
    /// than one large download.
    #[default]
    Revalidate,
    /// Take whatever is cached, however old. Only a miss reaches the network.
    PreferOffline,
    /// Never reach the network. A miss is an error.
    Offline,
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

/// The real client: an in-process packument cache over an on-disk one over a
/// transport.
///
/// Single-threaded on purpose for v1: correctness before speed, and parallel
/// downloads are a change that needs the benchmark suite to justify it.
pub struct NpmRegistry {
    base: String,
    transport: Box<dyn Transport>,
    /// Dedupes repeat lookups inside one resolve — the resolver asks for the
    /// same name several times — and saves re-parsing what it already parsed.
    memory: RefCell<HashMap<String, Rc<Packument>>>,
    /// Carries metadata between runs. Absent means every run starts cold, which
    /// is what a bare `NpmRegistry::new` is for in tests.
    disk: Option<PackumentCache>,
    freshness: Freshness,
}

impl NpmRegistry {
    pub fn new(base: impl Into<String>) -> Self {
        Self::with_transport(base, Box::new(HttpTransport))
    }

    pub fn with_transport(base: impl Into<String>, transport: Box<dyn Transport>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            transport,
            memory: RefCell::new(HashMap::new()),
            disk: None,
            freshness: Freshness::default(),
        }
    }

    /// `$OPAL_REGISTRY`, else the public registry.
    pub fn discover() -> Self {
        Self::new(std::env::var(REGISTRY_ENV).unwrap_or_else(|_| DEFAULT_REGISTRY.to_string()))
    }

    #[must_use]
    pub fn with_packument_cache(mut self, cache: PackumentCache) -> Self {
        self.disk = Some(cache);
        self
    }

    #[must_use]
    pub fn with_freshness(mut self, freshness: Freshness) -> Self {
        self.freshness = freshness;
        self
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

    /// The bytes of a packument, from the freshest place that has them.
    fn packument_bytes(&self, name: &str, url: &str) -> Result<Vec<u8>, RegistryError> {
        let cached = self
            .disk
            .as_ref()
            .and_then(|disk| disk.get(&self.base, name));

        // Fresh enough, or the caller asked not to care: no round trip at all.
        // This is the case that turns a re-resolve from minutes into
        // milliseconds, and it is why revalidation alone is not enough — a 304
        // costs one round trip less than nothing, and round trips are the cost.
        let usable = match (&cached, self.freshness) {
            (None, Freshness::Offline) => return Err(RegistryError::NotCached(name.to_string())),
            (None, _) => false,
            (Some(_), Freshness::PreferOffline | Freshness::Offline) => true,
            (Some(record), Freshness::Revalidate) => record.is_fresh(SystemTime::now()),
        };
        if usable {
            return Ok(cached.expect("a usable record is a record").body);
        }

        let request = Request {
            url,
            accept: Some(ABBREVIATED_PACKUMENT),
            etag: cached.as_ref().and_then(|record| record.etag.as_deref()),
        };
        let fetched = match self.transport.get(&request) {
            Ok(fetched) => fetched,
            Err(RegistryError::Status { status: 404, .. }) => {
                return Err(RegistryError::NotFound(name.to_string()));
            }
            Err(error) => return Err(error),
        };

        let record = match (fetched, cached) {
            // Confirmed still good: keep the body, restart its clock.
            (Fetched::NotModified, Some(record)) => Record {
                fetched: SystemTime::now(),
                ..record
            },
            // A 304 with nothing to apply it to should not happen, since the
            // request only carries a validator when there is a record.
            (Fetched::NotModified, None) => {
                return Err(RegistryError::Transport {
                    url: url.to_string(),
                    message: "server answered 304 for a request with no validator".to_string(),
                });
            }
            (Fetched::Fresh(response), _) => Record {
                body: response.body,
                etag: response.etag,
                fetched: SystemTime::now(),
                max_age: response.max_age.unwrap_or(packuments::DEFAULT_MAX_AGE),
            },
        };

        if let Some(disk) = &self.disk {
            disk.put(&self.base, name, &record);
        }
        // Moved, not cloned: a packument is measured in megabytes, and this
        // path runs once per package in the tree.
        Ok(record.body)
    }
}

impl Registry for NpmRegistry {
    fn packument(&self, name: &str) -> Result<Rc<Packument>, RegistryError> {
        if let Some(cached) = self.memory.borrow().get(name) {
            return Ok(Rc::clone(cached));
        }
        let url = self.packument_url(name);
        let bytes = self.packument_bytes(name, &url)?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|source| RegistryError::Json {
                url: url.clone(),
                source,
            })?;

        let packument = Rc::new(Packument::parse(name, &value));
        self.memory
            .borrow_mut()
            .insert(name.to_string(), Rc::clone(&packument));
        Ok(packument)
    }

    fn tarball(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        // Offline is offline. The store already answered "do I have this"
        // before the pipeline asked for a tarball at all, so reaching here
        // means it is genuinely missing and there is no local way to get it.
        if self.freshness == Freshness::Offline {
            return Err(RegistryError::NotCached(url.to_string()));
        }
        // Never cached here: a tarball's bytes go straight into the CAS, which
        // is content-addressed and already answers "do I have this".
        match self.transport.get(&Request {
            url,
            accept: None,
            etag: None,
        })? {
            Fetched::Fresh(response) => Ok(response.body),
            Fetched::NotModified => Err(RegistryError::Transport {
                url: url.to_string(),
                message: "server answered 304 for a request with no validator".to_string(),
            }),
        }
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

fn read_http(request: &Request<'_>) -> Result<Fetched, RegistryError> {
    let url = request.url;
    let mut call = ureq::get(url);
    if let Some(accept) = request.accept {
        call = call.header("Accept", accept);
    }
    if let Some(etag) = request.etag {
        call = call.header("If-None-Match", etag);
    }

    let mut response = call.call().map_err(|error| match &error {
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
    if status == 304 {
        return Ok(Fetched::NotModified);
    }
    if status >= 400 {
        return Err(RegistryError::Status {
            url: url.to_string(),
            status,
        });
    }

    let etag = header(&response, "etag");
    let max_age = header(&response, "cache-control")
        .as_deref()
        .and_then(max_age_of);
    Ok(Fetched::Fresh(Response {
        body: read_chunked(url, response.body_mut().as_reader())?,
        etag,
        max_age,
    }))
}

fn header<T>(response: &ureq::http::Response<T>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)?
        .to_str()
        .ok()
        .map(str::to_string)
}

/// `Cache-Control: public, max-age=300` — the only directive that changes what
/// this cache does. `no-store` and `no-cache` both come back as zero, which
/// makes every later read revalidate.
fn max_age_of(value: &str) -> Option<Duration> {
    let lowered = value.to_ascii_lowercase();
    if lowered.contains("no-store") || lowered.contains("no-cache") {
        return Some(Duration::ZERO);
    }
    lowered
        .split(',')
        .filter_map(|directive| {
            directive
                .trim()
                .strip_prefix("max-age=")?
                .parse::<u64>()
                .ok()
        })
        .next()
        .map(Duration::from_secs)
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
    fn test_cache_control_directives() {
        assert_eq!(
            max_age_of("public, max-age=300"),
            Some(Duration::from_secs(300))
        );
        assert_eq!(
            max_age_of("MAX-AGE=60, must-revalidate"),
            Some(Duration::from_secs(60))
        );
        // A server that says not to store gets a record that is never fresh.
        assert_eq!(max_age_of("no-store"), Some(Duration::ZERO));
        assert_eq!(max_age_of("no-cache, max-age=300"), Some(Duration::ZERO));
        assert_eq!(max_age_of("public"), None);
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
