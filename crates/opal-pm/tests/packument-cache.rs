//! The packument cache, against a transport that counts what it is asked for.
//!
//! What matters here is not that bytes round-trip — `packuments.rs` unit-tests
//! that — but *when the client reaches the wire at all*. Every assertion below
//! is about a request that did or did not happen, because the gap this closes
//! is 440 sequential round-trips for metadata a previous run already paid for.

use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use opal_pm::packuments::PackumentCache;
use opal_pm::registry::{
    ABBREVIATED_PACKUMENT, Fetched, Freshness, NpmRegistry, Registry, RegistryError, Request,
    Response, Transport,
};

const BASE: &str = "https://registry.example";

#[derive(Clone, Debug, Default)]
struct Call {
    url: String,
    accept: Option<String>,
    etag: Option<String>,
}

/// Answers every packument request, and remembers being asked.
#[derive(Default)]
struct Wire {
    calls: Mutex<Vec<Call>>,
    /// Served instead of a body once set, standing in for a server that agrees
    /// the cached copy is still good.
    not_modified: Mutex<bool>,
    cache_control: Mutex<Option<String>>,
    body: Mutex<String>,
}

impl Wire {
    fn new() -> Self {
        Self {
            body: Mutex::new(packument("1.0.0")),
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

/// A handle to the wire, shared rather than borrowed: a boxed transport has to
/// outlive the client holding it, and these tests deliberately build several
/// clients over one wire — that is what makes them stand in for separate runs.
struct Shared(Arc<Wire>);

impl Shared {
    fn of(wire: &Arc<Wire>) -> Box<Self> {
        Box::new(Self(Arc::clone(wire)))
    }
}

impl Transport for Shared {
    fn get(&self, request: &Request<'_>) -> Result<Fetched, RegistryError> {
        self.0.calls.lock().unwrap().push(Call {
            url: request.url.to_string(),
            accept: request.accept.map(str::to_string),
            etag: request.etag.map(str::to_string),
        });
        if *self.0.not_modified.lock().unwrap() {
            return Ok(Fetched::NotModified);
        }
        Ok(Fetched::Fresh(Response {
            body: self.0.body.lock().unwrap().clone().into_bytes(),
            etag: Some("W/\"one\"".to_string()),
            max_age: self
                .0
                .cache_control
                .lock()
                .unwrap()
                .as_deref()
                .map(|_| Duration::from_secs(300)),
        }))
    }
}

fn packument(version: &str) -> String {
    serde_json::json!({
        "name": "demo",
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "version": version,
                "dist": {
                    "tarball": format!("{BASE}/demo-{version}.tgz"),
                    "integrity": "sha512-Zm9vYmFy"
                }
            }
        }
    })
    .to_string()
}

fn client(wire: &Arc<Wire>, cache: &PackumentCache, freshness: Freshness) -> NpmRegistry {
    NpmRegistry::with_transport(BASE, Shared::of(wire))
        .with_packument_cache(cache.clone())
        .with_freshness(freshness)
}

#[test]
fn test_metadata_survives_the_process_that_fetched_it() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = PackumentCache::new(directory.path());
    let wire = Arc::new(Wire::new());
    *wire.cache_control.lock().unwrap() = Some("max-age=300".to_string());

    client(&wire, &cache, Freshness::Revalidate)
        .packument("demo")
        .expect("first fetch");
    assert_eq!(wire.count(), 1);

    // A second client is a second `opal install`: same cache directory, new
    // process, and nothing left in memory.
    for _ in 0..3 {
        client(&wire, &cache, Freshness::Revalidate)
            .packument("demo")
            .expect("cached fetch");
    }
    assert_eq!(wire.count(), 1, "a fresh record answers without the wire");
}

#[test]
fn test_the_first_request_asks_for_the_abbreviated_document() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = PackumentCache::new(directory.path());
    let wire = Arc::new(Wire::new());

    client(&wire, &cache, Freshness::Revalidate)
        .packument("demo")
        .expect("fetch");

    let call = wire.calls().remove(0);
    assert_eq!(call.url, format!("{BASE}/demo"));
    assert_eq!(call.accept.as_deref(), Some(ABBREVIATED_PACKUMENT));
    assert_eq!(call.etag, None, "nothing cached yet to validate against");
}

#[test]
fn test_a_stale_record_is_revalidated_rather_than_re_downloaded() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = PackumentCache::new(directory.path());
    let wire = Arc::new(Wire::new());
    // No Cache-Control and a zero default would still be stale; this test wants
    // staleness on purpose, so the server sends nothing and the record is aged
    // out by asking for it after its max-age with a zero-length window.
    *wire.cache_control.lock().unwrap() = None;

    client(&wire, &cache, Freshness::Revalidate)
        .packument("demo")
        .expect("first fetch");

    // Age the record past its window by rewriting it as fetched long ago.
    let record = cache.get(BASE, "demo").expect("record");
    cache.put(
        BASE,
        "demo",
        &opal_pm::packuments::Record {
            fetched: record.fetched - Duration::from_secs(3600),
            ..record
        },
    );

    *wire.not_modified.lock().unwrap() = true;
    let packument = client(&wire, &cache, Freshness::Revalidate)
        .packument("demo")
        .expect("revalidated fetch");

    assert_eq!(wire.count(), 2);
    let second = wire.calls().remove(1);
    assert_eq!(
        second.etag.as_deref(),
        Some("W/\"one\""),
        "a stale record revalidates with its validator"
    );
    assert!(packument.dist_tags.contains_key("latest"));

    // And a 304 restarts the clock, so the next run is free again.
    *wire.not_modified.lock().unwrap() = false;
    client(&wire, &cache, Freshness::Revalidate)
        .packument("demo")
        .expect("fresh again");
    assert_eq!(wire.count(), 2, "the revalidated record is fresh again");
}

#[test]
fn test_prefer_offline_takes_a_stale_record_without_asking() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = PackumentCache::new(directory.path());
    let wire = Arc::new(Wire::new());

    client(&wire, &cache, Freshness::Revalidate)
        .packument("demo")
        .expect("first fetch");
    let record = cache.get(BASE, "demo").expect("record");
    cache.put(
        BASE,
        "demo",
        &opal_pm::packuments::Record {
            fetched: record.fetched - Duration::from_secs(86_400),
            ..record
        },
    );

    client(&wire, &cache, Freshness::PreferOffline)
        .packument("demo")
        .expect("stale is fine");
    assert_eq!(wire.count(), 1, "a day-old record is still an answer");
}

#[test]
fn test_offline_refuses_rather_than_reaching_the_wire() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = PackumentCache::new(directory.path());
    let wire = Arc::new(Wire::new());

    let error = client(&wire, &cache, Freshness::Offline)
        .packument("demo")
        .expect_err("nothing is cached");

    assert!(matches!(error, RegistryError::NotCached(name) if name == "demo"));
    assert_eq!(wire.count(), 0, "offline means offline");
}

#[test]
fn test_a_scoped_name_is_escaped_on_the_way_out() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = PackumentCache::new(directory.path());
    let wire = Arc::new(Wire::new());

    client(&wire, &cache, Freshness::Revalidate)
        .packument("@scope/pkg")
        .expect("fetch");

    assert_eq!(wire.calls()[0].url, format!("{BASE}/@scope%2fpkg"));
}

#[test]
fn test_a_second_registry_is_never_answered_by_this_ones_cache() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = PackumentCache::new(directory.path());
    let wire = Arc::new(Wire::new());

    client(&wire, &cache, Freshness::Revalidate)
        .packument("demo")
        .expect("fetch");

    NpmRegistry::with_transport("https://other.example", Shared::of(&wire))
        .with_packument_cache(cache.clone())
        .with_freshness(Freshness::Revalidate)
        .packument("demo")
        .expect("fetch from the other registry");

    assert_eq!(
        wire.count(),
        2,
        "a different registry is a different answer"
    );
}

#[test]
fn test_without_a_cache_every_process_starts_cold() {
    let wire = Arc::new(Wire::new());
    for _ in 0..3 {
        NpmRegistry::with_transport(BASE, Shared::of(&wire))
            .packument("demo")
            .expect("fetch");
    }
    assert_eq!(wire.count(), 3);

    // Within one client the in-process cache still holds.
    let registry = NpmRegistry::with_transport(BASE, Shared::of(&wire));
    registry.packument("demo").expect("fetch");
    registry.packument("demo").expect("memoized");
    assert_eq!(wire.count(), 4);
}

/// Holds each request until a second one has arrived, so two callers are
/// certain to have both missed the in-process cache before either is answered.
struct Meeting {
    wire: Shared,
    both_asked: Barrier,
}

impl Transport for Meeting {
    fn get(&self, request: &Request<'_>) -> Result<Fetched, RegistryError> {
        self.both_asked.wait();
        self.wire.get(request)
    }
}

#[test]
fn test_two_threads_that_both_fetch_a_package_share_one_packument() {
    let wire = Arc::new(Wire::new());
    let registry = NpmRegistry::with_transport(
        BASE,
        Box::new(Meeting {
            wire: Shared(Arc::clone(&wire)),
            both_asked: Barrier::new(2),
        }),
    );
    let shared: &dyn Registry = &registry;

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| shared.packument("demo").expect("fetch"));
        let second = scope.spawn(|| shared.packument("demo").expect("fetch"));
        (
            first.join().expect("no panic"),
            second.join().expect("no panic"),
        )
    });

    assert_eq!(wire.count(), 2, "the duplicate request is allowed");
    assert!(
        Arc::ptr_eq(&first, &second),
        "but one resolve never sees two packuments for a name"
    );
    assert!(Arc::ptr_eq(
        &first,
        &registry.cached_packument("demo").expect("memoized")
    ));
}
