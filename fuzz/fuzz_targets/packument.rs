//! Registry responses, which are the least trusted input in the system: they
//! arrive over the network and decide what gets installed.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    // Every field is optional and every unexpected shape is ignored rather
    // than fatal, so this must produce a packument for any JSON at all —
    // possibly one with no usable versions in it.
    let packument = opal_pm::registry::Packument::parse("fuzzed", &value);
    for (version, metadata) in &packument.versions {
        assert_eq!(version, &metadata.version, "a version indexes itself");
    }
});
