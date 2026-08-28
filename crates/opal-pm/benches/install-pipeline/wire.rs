//! A registry that meters what an install asks of the network.
//!
//! This wraps the real `NpmRegistry` instead of replacing it, so packument JSON
//! parsing, integrity verification, and tarball reading all stay inside the
//! measurement — on a packument carrying a few dozen published versions the
//! parse is not the small half of the cost.
//!
//! What the wrapper adds is the one thing a `file://` fixture cannot supply:
//! round-trip latency. A sequential download loop with nothing to wait for
//! measures as free, which hides exactly the property the loop's shape is
//! supposed to expose. The stall is a real `sleep`, not an arithmetic
//! adjustment at the end, so that a future concurrent fetcher shows up here as
//! wall time *below* `round_trips * rtt` rather than needing a different model.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

use opal_pm::registry::{Packument, Registry, RegistryError};

#[derive(Clone, Copy, Default, Debug)]
pub struct Traffic {
    /// Calls the pipeline made.
    pub requests: usize,
    /// Calls that would have reached the network.
    pub round_trips: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Meter {
    pub packuments: Traffic,
    pub tarballs: Traffic,
    pub tarball_bytes: u64,
    pub rtt: Duration,
}

impl Meter {
    /// Wall time this install spent waiting on round-trips it made one at a
    /// time. The headline number for anything that wants to overlap them.
    pub fn stalled(&self) -> Duration {
        self.rtt * (self.packuments.round_trips + self.tarballs.round_trips) as u32
    }
}

pub struct MeteredRegistry<R> {
    inner: R,
    rtt: Duration,
    /// Names already requested, mirroring `NpmRegistry`'s in-process cache:
    /// only the first request for a name reaches the wire, and charging a cache
    /// hit a full round-trip would price the resolver's repeat lookups as
    /// network cost they do not have.
    seen: RefCell<BTreeSet<String>>,
    packuments: Cell<Traffic>,
    tarballs: Cell<Traffic>,
    tarball_bytes: Cell<u64>,
}

impl<R> MeteredRegistry<R> {
    pub fn new(inner: R, rtt: Duration) -> Self {
        Self {
            inner,
            rtt,
            seen: RefCell::new(BTreeSet::new()),
            packuments: Cell::new(Traffic::default()),
            tarballs: Cell::new(Traffic::default()),
            tarball_bytes: Cell::new(0),
        }
    }

    pub fn meter(&self) -> Meter {
        Meter {
            packuments: self.packuments.get(),
            tarballs: self.tarballs.get(),
            tarball_bytes: self.tarball_bytes.get(),
            rtt: self.rtt,
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

impl<R: Registry> Registry for MeteredRegistry<R> {
    fn packument(&self, name: &str) -> Result<Rc<Packument>, RegistryError> {
        let cold = self.seen.borrow_mut().insert(name.to_string());
        let mut traffic = self.packuments.get();
        traffic.requests += 1;
        traffic.round_trips += usize::from(cold);
        self.packuments.set(traffic);

        if cold {
            self.stall();
        }
        self.inner.packument(name)
    }

    fn tarball(&self, url: &str) -> Result<Vec<u8>, RegistryError> {
        self.stall();
        let bytes = self.inner.tarball(url)?;

        let mut traffic = self.tarballs.get();
        traffic.requests += 1;
        traffic.round_trips += 1;
        self.tarballs.set(traffic);
        self.tarball_bytes
            .set(self.tarball_bytes.get() + bytes.len() as u64);
        Ok(bytes)
    }
}
