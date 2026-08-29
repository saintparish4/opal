//! `opal.lock`, which is untrusted the moment it is committed to a repository
//! someone else can open.
//!
//! A panic here is a package manager that dies on `opal install` for a file it
//! is supposed to reject with a message.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // Parsing must return an error, never unwind. Whatever comes back, a
    // successful parse has to survive a round trip: rendering what was parsed
    // and parsing that again must produce the same resolution, or the format
    // is not the total function it claims to be.
    if let Ok(resolution) = opal_pm::lockfile::parse(Path::new("opal.lock"), text) {
        // A value the format cannot hold is refused, not mangled — that path is
        // a correct outcome, not a round trip to check.
        let Ok(rendered) = opal_pm::lockfile::render(&resolution) else {
            return;
        };
        let reparsed = opal_pm::lockfile::parse(Path::new("opal.lock"), &rendered)
            .expect("what this build rendered, this build parses");
        assert_eq!(resolution, reparsed, "lockfile round trip is not stable");
    }
});
