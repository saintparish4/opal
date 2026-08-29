//! `package.json`, read from a downloaded tarball — so it is whatever the
//! publisher put there, including numbers where strings belong.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let manifest = opal_pm::manifest::Manifest::from_value(&value);
    // The dedupe rule has to hold for any input: one requirement per name
    // across the installable classes, or the layout plans two versions into
    // one directory.
    let mut installable: Vec<&str> = manifest
        .installable(true)
        .map(|requirement| requirement.name.as_str())
        .collect();
    let before = installable.len();
    installable.sort_unstable();
    installable.dedup();
    assert_eq!(before, installable.len(), "duplicate installable requirement");
});
