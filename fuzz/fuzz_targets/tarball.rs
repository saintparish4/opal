//! Package tarballs: gzip, then tar, then paths that become directories under
//! `node_modules`.
//!
//! Three separate hazards in one input — a decompression bomb, a path that
//! escapes the package, and a header that lies — so this drives the real
//! ingest rather than the tar reader alone.

#![no_main]

use libfuzzer_sys::fuzz_target;
use opal_pm::integrity::{Algorithm, Integrity};
use opal_pm::package::{Limits, PackageStore};
use opal_pm::semver::Version;

fuzz_target!(|data: &[u8]| {
    let Ok(directory) = tempfile::tempdir() else {
        return;
    };
    let Ok(cas) = opal_core::cas::Cas::open(directory.path().join("cas")) else {
        return;
    };
    let Ok(store) = PackageStore::open(cas, directory.path()) else {
        return;
    };
    // Tight ceilings so a bomb the fuzzer finds is refused in milliseconds
    // instead of filling the runner's disk before it can be reported.
    let store = store.with_limits(Limits {
        entry_bytes: 1 << 20,
        unpacked_bytes: 4 << 20,
        entries: 512,
    });

    let integrity = Integrity::of(Algorithm::Sha512, data);
    // Any outcome but a panic is correct: most inputs are not valid gzip.
    let _ = store.ingest("fuzzed", &Version::new(1, 0, 0), &integrity, data);
});
