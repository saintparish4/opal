//! Deterministic fault injection for the crash-safety suite.
//!
//! exit criteria require proving that a process killed mid-CAS-write
//! never leaves a corrupt entry; Phase 1 extends the same technique across the
//! whole install pipeline. Killing on a timer is a coin flip; instead each risky
//! write path calls [`checkpoint`], and when `OPAL_INTERNAL_FAULT_INJECT` names
//! that point the process announces itself on stderr and parks, so the test can
//! deliver a real SIGKILL at exactly the interesting instant.
//!
//! A fault point is just a name, so a tool can delcare its own without this
//! module — or `opal-core` at all — knowing anything about that tool
//!
//! This is compiled unconditionally rather than behind a feature. The cost is
//! one `OnceLock` read per write (not per byte), which is invisible next to the
//! syscall it guards; the benefit is that `cargo test` exercises it with no
//! special flags, and the same probe works against a release binary — which is
//! what chaos tests over the whole install pipeline will need.

use std::io::Write as _;
use std::sync::OnceLock;
use std::time::Duration;

/// Env var naming the point at which the process should park to be killed.
pub const FAULT_ENV: &str = "OPAL_INTERNAL_FAULT_INJECT";

/// Printed to stderr when a fault point is reached, so the test knows the
/// process is parked and it is safe to kill.
pub const READY_MARKER: &str = "opal-fault-reached";

/// A named point in a write path where a kill is interesting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultPoint(&'static str);

impl FaultPoint {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub fn name(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for FaultPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Parks the process if it was configured to fail at `point`.
pub fn checkpoint(point: FaultPoint) {
    if configured().is_some_and(|configured| configured == point.name()) {
        let mut stderr = std::io::stderr();
        let _ = writeln!(stderr, "{READY_MARKER} {}", point.name());
        let _ = stderr.flush();
        loop {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn configured() -> Option<&'static str> {
    static CONFIGURED: OnceLock<Option<String>> = OnceLock::new();
    CONFIGURED
        .get_or_init(|| {
            std::env::var(FAULT_ENV)
                .ok()
                .map(|value| value.trim().to_string())
        })
        .as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: FaultPoint = FaultPoint::new("test-probe");

    #[test]
    fn test_point_carries_its_name() {
        assert_eq!(PROBE.name(), "test-probe");
        assert_eq!(PROBE.to_string(), "test-probe");
    }

    #[test]
    fn test_checkpoint_is_inert_when_unconfigured() {
        // The test process sets no fault env var, so every checkpoint returns.
        checkpoint(PROBE);
        checkpoint(crate::cas::FAULT_MID_WRITE);
    }
}
