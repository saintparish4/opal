//! A transport that meters what an install asks of the network.
//!
//! This sits *below* `NpmRegistry` rather than wrapping it, and that placement
//! is the whole point. The client now keeps packument metadata on disk between
//! runs, so metering above it would charge a cache hit for a round trip it
//! never made — and no amount of caching could then show up as an improvement.
//! Below it, "reached the wire" means exactly that.
//!
//! What the wrapper adds is the one thing a `file://` fixture cannot supply:
//! round-trip latency. A download loop with nothing to wait for measures as
//! free, which hides exactly the property the loop's shape is supposed to
//! expose. The stall is a real `sleep`, not an arithmetic adjustment at the
//! end, so that concurrent fetching shows up here as wall time *below*
//! `round_trips * rtt` rather than needing a different model.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use opal_pm::registry::{Fetched, RegistryError, Request, Transport};

#[derive(Clone, Copy, Default, Debug)]
pub struct Traffic {
    /// Requests that reached the transport at all.
    pub round_trips: usize,
    /// Of those, the ones the server answered with "still good". A 304 is a
    /// round trip and is priced as one — it is cheaper in bytes, not in
    /// latency, and latency is what this harness models.
    pub revalidations: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Meter {
    pub packuments: Traffic,
    pub tarballs: Traffic,
    pub tarball_bytes: u64,
    pub rtt: Duration,
}

impl Meter {
    /// What this install's round-trips would cost taken one at a time. Wall
    /// time below it is the evidence that they overlapped.
    pub fn stalled(&self) -> Duration {
        self.rtt * (self.packuments.round_trips + self.tarballs.round_trips) as u32
    }
}

/// The counters, held apart from the transport so the harness can still read
/// them after the client has taken ownership of it.
#[derive(Default)]
pub struct Counters {
    packuments: Mutex<Traffic>,
    tarballs: Mutex<Traffic>,
    tarball_bytes: AtomicU64,
}

impl Counters {
    pub fn meter(&self, rtt: Duration) -> Meter {
        Meter {
            packuments: *lock(&self.packuments),
            tarballs: *lock(&self.tarballs),
            tarball_bytes: self.tarball_bytes.load(Ordering::Relaxed),
            rtt,
        }
    }
}

fn lock(traffic: &Mutex<Traffic>) -> MutexGuard<'_, Traffic> {
    traffic.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct MeteredTransport<T> {
    inner: T,
    rtt: Duration,
    counters: Arc<Counters>,
}

impl<T> MeteredTransport<T> {
    pub fn new(inner: T, rtt: Duration, counters: Arc<Counters>) -> Self {
        Self {
            inner,
            rtt,
            counters,
        }
    }

    fn stall(&self) {
        // `sleep(ZERO)` is still a syscall, and the zero-latency run is the one
        // that reports local cost, so it should carry none of this.
        if !self.rtt.is_zero() {
            std::thread::sleep(self.rtt);
        }
    }
}

impl<T: Transport> Transport for MeteredTransport<T> {
    fn get(&self, request: &Request<'_>) -> Result<Fetched, RegistryError> {
        // Metadata carries an `Accept`; a tarball does not. Classifying on that
        // rather than on the URL keeps the harness out of the business of
        // knowing what a fixture registry's paths look like.
        let counter = if request.accept.is_some() {
            &self.counters.packuments
        } else {
            &self.counters.tarballs
        };

        self.stall();
        let fetched = self.inner.get(request)?;

        {
            let mut traffic = lock(counter);
            traffic.round_trips += 1;
            if matches!(fetched, Fetched::NotModified) {
                traffic.revalidations += 1;
            }
        }

        if let Fetched::Fresh(response) = &fetched
            && request.accept.is_none()
        {
            self.counters
                .tarball_bytes
                .fetch_add(response.body.len() as u64, Ordering::Relaxed);
        }
        Ok(fetched)
    }
}
