//! Any source text as an entry module, ES module and CommonJS: the bundler
//! must build it or refuse it, never panic, and whatever it builds must
//! parse. No node needed.

#![no_main]

use libfuzzer_sys::fuzz_target;
use opal_core::graph::resolve;
use opal_core::path::NormalizedPath;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = NormalizedPath::from_native(&std::fs::canonicalize(dir.path()).unwrap()).unwrap();
    for entry in ["main.mjs", "main.cjs"] {
        std::fs::write(dir.path().join(entry), text).unwrap();
        let options = opal_bundler::BuildOptions::default();
        let Ok(resolution) = resolve(&root, &NormalizedPath::new(entry), &options.target.resolver_options(None)) else {
            continue;
        };
        if let Ok(output) = opal_bundler::build(&resolution.graph, &dir.path().join("dist"), &options) {
            let allocator = oxc_allocator::Allocator::default();
            let parsed = oxc_parser::Parser::new(&allocator, &output.code, oxc_span::SourceType::mjs()).parse();
            assert!(parsed.diagnostics.is_empty(), "the bundle does not parse: {:?}\n{}", parsed.diagnostics, output.code);
        }
    }
});
