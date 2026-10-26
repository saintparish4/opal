//! `OPAL_TRACE_STARTUP`: where a run's startup time goes.
//!
//! With the variable set, each [`checkpoint`] records a timestamp and
//! [`finish`] prints the timeline to stderr, one line per checkpoint: time
//! since the first one, and time since the one before. Unset, a checkpoint
//! is one load of a static that is already initialized.
//!
//! The clock is `CLOCK_MONOTONIC`, read directly rather than through
//! `Instant`, so that the timeline can start before `main`: when
//! `OPAL_TRACE_SPAWN_NS` holds the parent's `CLOCK_MONOTONIC` reading in
//! nanoseconds just before it spawned opal (Python:
//! `time.clock_gettime_ns(time.CLOCK_MONOTONIC)`), the first line is `spawn`,
//! so the dynamic loader and everything else before `main` shows up as the gap
//! to the next one. Without it, the timeline starts at the first checkpoint.
//!
//! Lines are buffered until [`finish`] so that writing them does not land in
//! the gaps being measured.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// A checkpoint's name and its `CLOCK_MONOTONIC` reading in nanoseconds.
type Point = (&'static str, u64);

static TRACE: OnceLock<Option<Mutex<Vec<Point>>>> = OnceLock::new();

/// Records that startup reached `name`.
pub fn checkpoint(name: &'static str) {
    let Some(trace) = TRACE.get_or_init(open) else {
        return;
    };
    let now = monotonic_ns();
    if let Ok(mut points) = trace.lock() {
        points.push((name, now));
    }
}

/// Lines printed after the timeline, for what a checkpoint can't say.
static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Records a line for the end of the timeline, if tracing is on.
pub fn note(line: impl FnOnce() -> String) {
    if TRACE.get_or_init(open).is_none() {
        return;
    }
    if let Ok(mut notes) = NOTES.lock() {
        notes.push(line());
    }
}

/// Prints the timeline, if tracing is on, and clears it. Called before the
/// process exits or replaces itself with `exec`.
pub fn finish() {
    let Some(Some(trace)) = TRACE.get() else {
        return;
    };
    let Ok(mut points) = trace.lock() else {
        return;
    };
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(render(&points).as_bytes());
    points.clear();
    if let Ok(mut notes) = NOTES.lock() {
        for line in notes.drain(..) {
            let _ = writeln!(stderr, "opal-trace {line}");
        }
    }
}

fn open() -> Option<Mutex<Vec<Point>>> {
    let value = std::env::var_os("OPAL_TRACE_STARTUP")?;
    if value.is_empty() || value == "0" {
        return None;
    }
    let mut points = Vec::with_capacity(16);
    let spawned = std::env::var("OPAL_TRACE_SPAWN_NS").ok();
    if let Some(spawned) = spawned.and_then(|v| v.parse::<u64>().ok()) {
        points.push(("spawn", spawned));
    }
    Some(Mutex::new(points))
}

/// One line per checkpoint: `opal-trace <since first> ms <since previous> ms <name>`.
fn render(points: &[Point]) -> String {
    let Some(&(_, first)) = points.first() else {
        return String::new();
    };
    let mut out = String::new();
    let mut previous = first;
    for &(name, at) in points {
        let total = at.saturating_sub(first) as f64 / 1e6;
        let step = at.saturating_sub(previous) as f64 / 1e6;
        out.push_str(&format!(
            "opal-trace {total:9.3} ms {step:+9.3} ms  {name}\n"
        ));
        previous = at;
    }
    out
}

fn monotonic_ns() -> u64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid timespec to write into, and CLOCK_MONOTONIC
    // exists on every platform opal builds for.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_reports_offsets_from_the_first_point_and_the_previous_one() {
        let lines = render(&[
            ("spawn", 1_000_000),
            ("main", 3_500_000),
            ("exit", 4_000_000),
        ]);
        let lines: Vec<_> = lines.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("0.000 ms") && lines[0].ends_with("spawn"));
        assert!(lines[1].contains("2.500 ms") && lines[1].ends_with("main"));
        assert!(lines[2].contains("3.000 ms") && lines[2].contains("+0.500 ms"));
    }

    #[test]
    fn nothing_recorded_renders_nothing() {
        assert_eq!(render(&[]), "");
    }

    #[test]
    fn the_clock_does_not_go_backwards() {
        let a = monotonic_ns();
        let b = monotonic_ns();
        assert!(b >= a);
    }
}
