//! Registry responses, which are the least trusted input in the system: they
//! arrive over the network and decide what gets installed.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Every field is optional and every unexpected shape is ignored rather
    // than fatal, so this must produce a packument for any bytes at all —
    // possibly one with no usable versions in it.
    let packument = opal_pm::registry::Packument::parse("fuzzed", data);
    for version in packument.versions() {
        // Bodies are parsed on demand, so this is where a malformed one has
        // to degrade rather than panic.
        if let Some(metadata) = packument.version(version) {
            assert_eq!(version, &metadata.version, "a version indexes itself");
        }
    }
});
