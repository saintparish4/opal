//! Fetching several things at once, held to what fetching one at a time does.
//!
//! Two claims, and each needs the other. Concurrency must change nothing an
//! install produces or asks for: the same lockfile, the same tree, the same
//! requests. And it must actually be concurrent, or the first claim is
//! trivially true of a loop.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use opal_core::cache::CacheRoot;
use opal_pm::fixtures::{FixtureRegistry, Package, write_project};
use opal_pm::install::{self, InstallOptions};
use opal_pm::lockfile;
use opal_pm::manifest::Manifest;
use opal_pm::package::PackageStore;
use opal_pm::progress::Silent;
use opal_pm::projects::ProjectIndex;
use opal_pm::registry::{Fetched, HttpTransport, NpmRegistry, RegistryError, Request, Transport};
use opal_pm::resolve::{self, ResolveError, ResolveOptions};

/// What reached the transport, and how many requests were ever in it at once.
#[derive(Default)]
struct Traffic {
    urls: Mutex<Vec<String>>,
    in_flight: Mutex<InFlight>,
    another_arrived: Condvar,
}

#[derive(Default)]
struct InFlight {
    now: usize,
    most: usize,
}

impl Traffic {
    /// Sorted, so two runs compare by what they asked for and not by which
    /// thread asked first.
    fn urls(&self) -> Vec<String> {
        let mut urls = self.urls.lock().unwrap().clone();
        urls.sort();
        urls
    }

    fn most_in_flight(&self) -> usize {
        self.in_flight.lock().unwrap().most
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Packument,
    Tarball,
}

/// The fixture registry's transport, observed.
struct Observed {
    inner: HttpTransport,
    traffic: Arc<Traffic>,
    /// The requests whose overlap is being measured. An install makes both
    /// kinds, and packuments overlapping says nothing about tarballs.
    watched: Kind,
    /// How long a watched request waits for company before going on alone.
    /// Zero for a run that only counts.
    linger: Duration,
}

impl Transport for Observed {
    fn get(&self, request: &Request<'_>) -> Result<Fetched, RegistryError> {
        self.traffic
            .urls
            .lock()
            .unwrap()
            .push(request.url.to_string());

        // Metadata carries an `Accept`; a tarball does not.
        let kind = if request.accept.is_some() {
            Kind::Packument
        } else {
            Kind::Tarball
        };
        if kind != self.watched {
            return self.inner.get(request);
        }

        let mut in_flight = self.traffic.in_flight.lock().unwrap();
        in_flight.now += 1;
        in_flight.most = in_flight.most.max(in_flight.now);
        self.traffic.another_arrived.notify_all();
        // Held until a second request shows up, so overlap is observed rather
        // than raced for. A sequential caller never sends one, and gives up
        // waiting instead of hanging the test.
        let (mut in_flight, _) = self
            .traffic
            .another_arrived
            .wait_timeout_while(in_flight, self.linger, |in_flight| in_flight.most < 2)
            .unwrap();
        in_flight.now -= 1;
        drop(in_flight);

        self.inner.get(request)
    }
}

struct Sandbox {
    _directory: tempfile::TempDir,
    project: PathBuf,
    registry: FixtureRegistry,
    cache: CacheRoot,
}

impl Sandbox {
    /// A tree three levels deep, wide enough at each that there is something
    /// to overlap, with the awkward edges fetching ahead has to leave alone: a
    /// package two dependents share, an optional dependency that does not
    /// exist, and a specifier that is never fetched at all.
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut registry = FixtureRegistry::new(directory.path().join("registry"));
        registry
            .publish(Package::new("leaf", "1.0.0"))
            .publish(Package::new("leaf", "1.1.0"))
            .publish(Package::new("left", "1.0.0").dependency("leaf", "^1.0.0"))
            .publish(Package::new("right", "1.0.0").dependency("leaf", "~1.0.0"))
            .publish(
                Package::new("a", "1.0.0")
                    .dependency("left", "^1.0.0")
                    .optional_dependency("missing", "^1.0.0"),
            )
            .publish(Package::new("b", "1.0.0").dependency("right", "^1.0.0"))
            .publish(
                Package::new("c", "1.0.0")
                    .dependency("left", "^1.0.0")
                    .optional_dependency("from-git", "git+https://example.invalid/x.git"),
            );

        let project = directory.path().join("project");
        write_project(
            &project,
            serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "a": "^1.0.0", "b": "^1.0.0", "c": "^1.0.0" }
            }),
        );
        Self {
            project,
            registry,
            cache: CacheRoot::at(directory.path().join("cache")),
            _directory: directory,
        }
    }

    fn client(&self, watched: Kind, linger: Duration) -> (NpmRegistry, Arc<Traffic>) {
        let traffic = Arc::new(Traffic::default());
        let transport = Observed {
            inner: HttpTransport::new(),
            traffic: Arc::clone(&traffic),
            watched,
            linger,
        };
        (
            NpmRegistry::with_transport(self.registry.url(), Box::new(transport)),
            traffic,
        )
    }

    fn manifest(&self) -> Manifest {
        Manifest::read(&self.project.join("package.json")).expect("manifest")
    }

    /// The lockfile text and the requests it took to resolve it.
    fn resolve(&self, concurrent_requests: usize) -> (String, Vec<String>) {
        let (registry, traffic) = self.client(Kind::Packument, Duration::ZERO);
        let resolution = resolve::resolve(
            &registry,
            &self.manifest(),
            &ResolveOptions {
                concurrent_requests,
                ..ResolveOptions::default()
            },
        )
        .expect("resolve");
        (
            lockfile::render(&resolution).expect("render"),
            traffic.urls(),
        )
    }
}

/// Long enough that a slow machine still gets a second thread to the
/// transport, and only ever waited out in full by a build that has regressed
/// to one request at a time.
const LINGER: Duration = Duration::from_secs(2);

#[test]
fn test_fetching_ahead_asks_for_exactly_what_resolving_alone_asks_for() {
    let sandbox = Sandbox::new();

    let (alone, alone_requests) = sandbox.resolve(1);
    let (ahead, ahead_requests) = sandbox.resolve(16);

    assert_eq!(ahead, alone, "the lockfile is byte-identical");
    assert_eq!(
        ahead_requests, alone_requests,
        "no request is added, repeated, or dropped"
    );
    assert!(
        alone_requests
            .iter()
            .any(|url| url.ends_with("/missing.json")),
        "the tree includes a package the registry refuses: {alone_requests:?}"
    );
}

#[test]
fn test_packuments_for_one_level_are_fetched_at_the_same_time() {
    let sandbox = Sandbox::new();
    let (registry, traffic) = sandbox.client(Kind::Packument, LINGER);

    resolve::resolve(&registry, &sandbox.manifest(), &ResolveOptions::default()).expect("resolve");

    assert!(
        traffic.most_in_flight() >= 2,
        "resolution asked for one packument at a time"
    );
}

#[test]
fn test_a_required_package_refused_while_fetching_ahead_fails_the_resolve() {
    let sandbox = Sandbox::new();
    write_project(
        &sandbox.project,
        serde_json::json!({
            "dependencies": { "a": "^1.0.0", "absent": "^1.0.0", "b": "^1.0.0" }
        }),
    );

    for concurrent_requests in [1, 16] {
        let (registry, _) = sandbox.client(Kind::Packument, Duration::ZERO);
        let error = resolve::resolve(
            &registry,
            &sandbox.manifest(),
            &ResolveOptions {
                concurrent_requests,
                ..ResolveOptions::default()
            },
        )
        .expect_err("a required package is missing");
        assert!(
            matches!(
                &error,
                ResolveError::Registry(RegistryError::NotFound(name)) if name == "absent"
            ),
            "{concurrent_requests} at a time: {error}"
        );
    }
}

#[test]
fn test_tarballs_are_downloaded_at_the_same_time_into_the_same_tree() {
    let sandbox = Sandbox::new();
    let store = PackageStore::open(sandbox.cache.open_cas().expect("cas"), sandbox.cache.path())
        .expect("package store");
    let projects = ProjectIndex::new(sandbox.cache.path().join("projects")).expect("project index");
    let (registry, traffic) = sandbox.client(Kind::Tarball, LINGER);

    let report = install::install(
        &sandbox.project,
        &registry,
        &store,
        &projects,
        &InstallOptions::default(),
        &Silent,
    )
    .expect("install");

    let tarballs = traffic
        .urls()
        .iter()
        .filter(|url| url.ends_with(".tgz"))
        .count();
    // Six names, and `leaf` at two versions: `right` pins the older one.
    assert_eq!(tarballs, 7, "each package is downloaded once");
    assert_eq!(report.fetched, 7);
    assert_eq!(report.already_stored, 0);
    assert!(
        traffic.most_in_flight() >= 2,
        "tarballs were downloaded one at a time"
    );
    for package in [
        "a",
        "b",
        "c",
        "left",
        "right",
        "leaf",
        "right/node_modules/leaf",
    ] {
        assert!(
            sandbox
                .project
                .join("node_modules")
                .join(package)
                .join(".opal-package")
                .is_file(),
            "{package} is installed"
        );
    }
}
