//! JS/TS source and a dependency's `package.json`, through the module resolver.
//!
//! Both come out of installed packages, so both are whatever a publisher
//! wrote: source text the parser has to survive, and an `exports` map the
//! resolver has to obey without letting it point outside the package.
//!
//! Input: one byte choosing the entry file's extension, the entry's source, a
//! NUL, then the `package.json` of `node_modules/pkg`. Start from the
//! hand-written seeds, and splice in the tokens in `resolver.dict`:
//!
//! ```text
//! cargo +nightly fuzz run resolver corpus/resolver seeds/resolver -- -dict=resolver.dict -timeout=10
//! ```
//!
//! This target's strength is volume: every parser and resolver path, and the
//! containment check on every input. Finding an escape is its weak spot,
//! because nothing rewards a specifier for getting one `../` closer. The same
//! property is therefore also a proptest,
//! `crates/opal-core/tests/exports-properties.rs`, which builds those segments
//! on purpose.

#![no_main]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use opal_core::graph::{DependencyTarget, ResolverOptions, resolver};
use opal_core::path::NormalizedPath;

const EXTENSIONS: [&str; 9] = ["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts", ""];

fn entry_name(extension: &str) -> String {
    if extension.is_empty() {
        "entry".to_string()
    } else {
        format!("entry.{extension}")
    }
}

/// One project per fuzzing process, reused across inputs. A fresh directory
/// per input capped throughput at a few hundred inputs a second, and only the
/// entry and `pkg`'s manifest change between inputs.
fn project() -> Option<&'static (PathBuf, NormalizedPath)> {
    static PROJECT: OnceLock<Option<(PathBuf, NormalizedPath)>> = OnceLock::new();
    PROJECT
        .get_or_init(|| {
            let root = tempfile::Builder::new()
                .prefix("opal-fuzz-resolver")
                .tempdir()
                .ok()?
                .keep();
            let fixed: [(PathBuf, &[u8]); 7] = [
                (root.join("node_modules/pkg/index.js"), b"module.exports = 1;\n"),
                (root.join("node_modules/pkg/a.js"), b"export default 1;\n"),
                (root.join("node_modules/pkg/lib/a.js"), b"export default 1;\n"),
                // Bait outside `pkg`, named like what is inside it: an escape
                // is only visible when it lands on a file, and `pkg/a` is one
                // or two inserted `../` away from these.
                (root.join("node_modules/a.js"), b"module.exports = 2;\n"),
                (root.join("node_modules/outside.js"), b"module.exports = 2;\n"),
                (root.join("a.js"), b"module.exports = 3;\n"),
                (root.join("src/util.ts"), b"export const x = 1;\n"),
            ];
            for (path, contents) in fixed {
                write(&path, contents)?;
            }
            let normalized = NormalizedPath::from_native(&root).ok()?;
            Some((root, normalized))
        })
        .as_ref()
}

fn write(path: &Path, contents: &[u8]) -> Option<()> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(path, contents).ok()
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let (source, manifest) = match rest.iter().position(|byte| *byte == 0) {
        Some(split) => (&rest[..split], &rest[split + 1..]),
        None => (rest, &b"{}"[..]),
    };
    let Some((root, normalized_root)) = project() else {
        return;
    };

    // The previous input's entry would otherwise be importable by this one.
    for extension in EXTENSIONS {
        let _ = std::fs::remove_file(root.join(entry_name(extension)));
    }
    let entry = entry_name(EXTENSIONS[usize::from(selector) % EXTENSIONS.len()]);
    if write(&root.join("node_modules/pkg/package.json"), manifest).is_none()
        || write(&root.join(&entry), source).is_none()
    {
        return;
    }

    // An error is a legitimate outcome (an unreadable file, say); only a
    // panic or an escape is a bug.
    let Ok(resolution) = resolver::resolve(
        normalized_root,
        &NormalizedPath::new(&entry),
        &ResolverOptions::default(),
    ) else {
        return;
    };

    // Node's rule: a package's `exports` maps specifiers into that package
    // and nowhere else. Without `exports`, `pkg/../x` is a plain path join,
    // and leaving the package is allowed.
    let has_exports = serde_json::from_slice::<serde_json::Value>(manifest)
        .is_ok_and(|value| value.get("exports").is_some());
    if !has_exports {
        return;
    }
    let inside = NormalizedPath::new("node_modules/pkg");
    let graph = &resolution.graph;
    for module in graph.modules() {
        for dependency in &module.dependencies {
            let into_pkg = dependency.specifier == "pkg" || dependency.specifier.starts_with("pkg/");
            if let (true, DependencyTarget::Module { id }) = (into_pkg, &dependency.target) {
                let target = &graph.module(*id).path;
                assert!(
                    target.starts_with(&inside),
                    "{:?} left the package through its exports map: {target}",
                    dependency.specifier
                );
            }
        }
    }
});
