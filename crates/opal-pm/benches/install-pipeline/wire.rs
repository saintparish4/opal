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
//!
//! Latency is half of a network. The other half is that bytes take time to
//! arrive, and they share one link however many requests are open: sixteen
//! downloads at once do not move sixteen times the data. With a bandwidth
//! set, every response body is charged against one shared pipe, so
//! concurrency can overlap the waiting but never the transfer. Without that
//! floor a parallel fetcher measures several times faster here than it is on
//! a real network.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

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
    pub packument_bytes: u64,
    pub rtt: Duration,
    /// Bytes per second through the shared pipe, if one was simulated.
    pub bandwidth: Option<u64>,
}

impl Meter {
    /// How long this install's bytes took to arrive, which no amount of
    /// concurrency shortens. Zero when no bandwidth was simulated.
    pub fn transfer(&self) -> Duration {
        self.bandwidth.map_or(Duration::ZERO, |rate| {
            Duration::from_secs_f64(
                (self.packument_bytes + self.tarball_bytes) as f64 / rate as f64,
            )
        })
    }

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
    packument_bytes: AtomicU64,
}

impl Counters {
    pub fn meter(&self, rtt: Duration, bandwidth: Option<u64>) -> Meter {
        Meter {
            packuments: *lock(&self.packuments),
            tarballs: *lock(&self.tarballs),
            tarball_bytes: self.tarball_bytes.load(Ordering::Relaxed),
            packument_bytes: self.packument_bytes.load(Ordering::Relaxed),
            rtt,
            bandwidth,
        }
    }
}

fn lock(traffic: &Mutex<Traffic>) -> MutexGuard<'_, Traffic> {
    traffic.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct MeteredTransport<T> {
    inner: T,
    rtt: Duration,
    bandwidth: Option<u64>,
    /// When the shared pipe finishes the transfers already queued on it.
    pipe_free: Mutex<Instant>,
    counters: Arc<Counters>,
}

impl<T> MeteredTransport<T> {
    pub fn new(inner: T, rtt: Duration, bandwidth: Option<u64>, counters: Arc<Counters>) -> Self {
        Self {
            inner,
            rtt,
            bandwidth,
            pipe_free: Mutex::new(Instant::now()),
            counters,
        }
    }

    /// Waits for `bytes` to come through the pipe every request shares.
    ///
    /// Transfers queue one behind another, which finishes the whole batch at
    /// the same moment as sharing the link evenly would, and that total is
    /// what an install's wall time depends on.
    fn transfer(&self, bytes: usize) {
        let Some(rate) = self.bandwidth else { return };
        let needed = Duration::from_secs_f64(bytes as f64 / rate as f64);
        let done = {
            let mut free = self
                .pipe_free
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *free = (*free).max(Instant::now()) + needed;
            *free
        };
        std::thread::sleep(done.saturating_duration_since(Instant::now()));
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

        if let Fetched::Fresh(response) = &fetched {
            let counter = if request.accept.is_some() {
                &self.counters.packument_bytes
            } else {
                &self.counters.tarball_bytes
            };
            counter.fetch_add(response.body.len() as u64, Ordering::Relaxed);
            self.transfer(response.body.len());
        }
        Ok(fetched)
    }
}
