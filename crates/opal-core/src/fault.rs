//! Deterministic fault injection for the crash-safety suite.
//!
//! exit criteria require proving that a process killed mid-CAS-write
//! never leaves a corrupt entry. Killing on a timer is a coin flip; instead each
//! risky write path calls [`checkpoint`], and when `OPAL_INTERNAL_FAULT_INJECT`
//! names that point the process announces itself on stderr and parks, so the
//! test can deliver a real SIGKILL at exactly the interesting instant.
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

/// Points in a write path where a kill is interesting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultPoint {
    /// Temp file partially written: bytes on disk, hash not yet known.
    CasMidWrite,
    /// Temp file written, fsynced, and hash-verified; rename not yet issued.
    CasBeforeRename,
    /// Memo record written to its temp file; rename not yet issued.
    MemoBeforeRename,
}

impl FaultPoint {
    pub fn name(self) -> &'static str {
        match self {
            Self::CasMidWrite => "cas-mid-write",
            Self::CasBeforeRename => "cas-before-rename",
            Self::MemoBeforeRename => "memo-before-rename",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::CasMidWrite,
            Self::CasBeforeRename,
            Self::MemoBeforeRename,
        ]
        .into_iter()
        .find(|point| point.name() == name)
    }
}

/// Parks the process if it was configured to fail at `point`.
pub(crate) fn checkpoint(point: FaultPoint) {
    if configured() == Some(point) {
        let mut stderr = std::io::stderr();
        let _ = writeln!(stderr, "{READY_MARKER} {}", point.name());
        let _ = stderr.flush();
        loop {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn configured() -> Option<FaultPoint> {
    static CONFIGURED: OnceLock<Option<FaultPoint>> = OnceLock::new();
    *CONFIGURED.get_or_init(|| {
        std::env::var(FAULT_ENV)
            .ok()
            .and_then(|value| FaultPoint::parse(value.trim()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_point_names_round_trip() {
        for point in [
            FaultPoint::CasMidWrite,
            FaultPoint::CasBeforeRename,
            FaultPoint::MemoBeforeRename,
        ] {
            assert_eq!(FaultPoint::parse(point.name()), Some(point));
        }
        assert_eq!(FaultPoint::parse("nonsense"), None);
    }

    #[test]
    fn test_checkpoint_is_inert_when_unconfigured() {
        // The test process sets no fault env var, so every checkpoint returns.
        checkpoint(FaultPoint::CasMidWrite);
        checkpoint(FaultPoint::CasBeforeRename);
        checkpoint(FaultPoint::MemoBeforeRename);
    }
}
