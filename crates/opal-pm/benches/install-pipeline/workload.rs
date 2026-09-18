//! A synthetic registry shaped like a real dependency tree.
//!
//! Shape matters more than size here. Two properties of a real tree are what
//! the install pipeline actually pays for, and a naive fixture has neither:
//!
//! - **Fat packuments.** A published package carries every version it has ever
//!   released in one JSON document, so resolving 400 packages means parsing 400
//!   documents of tens of versions each, not 400 one-line stubs. Measured on a
//!   `create-next-app` tree, this, not downloading, was the top cost of a
//!   warm-cache install.
//! - **Duplicates.** Real trees have popular packages with many dependents, and
//!   some dependent pins an old version, so the layout planner has to nest
//!   rather than hoist everything flat. A tree where every package has exactly
//!   one parent cannot produce that no matter how the pins fall.
//!
//! Only the newest version of each package carries a file payload. Older
//! versions exist to weigh down the packument, are never selected by `^1.0.0`,
//! and would otherwise cost a gzip each at setup time for a tarball nothing
//! fetches.

use std::path::Path;

use opal_pm::fixtures::{FixtureRegistry, Package};

#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub packages: usize,
    /// Published versions per package: the packument-weight dial.
    pub versions: usize,
    /// Files in each package's newest version.
    pub files: usize,
    /// Dependencies each package declares.
    pub fanout: usize,
    /// Dependencies the project itself declares.
    pub roots: usize,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            packages: 64,
            versions: 12,
            files: 6,
            fanout: 2,
            roots: 8,
        }
    }
}

/// One in seven edges onto the popular pool pins an exact old version, so the
/// tree contains duplicates and the linker has to nest. A tree that hoists
/// perfectly flat is not a tree anyone installs.
const PIN_EVERY: usize = 7;

const FILE_PADDING: usize = 1900;

pub struct Workload {
    pub shape: Shape,
    pub manifest: serde_json::Value,
    pub packument_bytes: u64,
    pub tarball_bytes: u64,
    registry_url: String,
    _directory: tempfile::TempDir,
}

impl Workload {
    pub fn generate(shape: Shape) -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        let root = directory.path().join("registry");
        let mut registry = FixtureRegistry::new(&root);

        for index in 0..shape.packages {
            for version in 0..shape.versions {
                registry.publish(publish(&shape, index, version));
            }
        }

        let dependencies: serde_json::Map<String, serde_json::Value> =
            (0..shape.roots.min(shape.packages))
                .map(|index| (name(index), serde_json::json!("^1.0.0")))
                .collect();

        Self {
            shape,
            manifest: serde_json::json!({
                "name": "install-benchmark",
                "version": "1.0.0",
                "dependencies": dependencies,
            }),
            packument_bytes: bytes_in(&root),
            tarball_bytes: bytes_in(&root.join("tarballs")),
            registry_url: registry.url(),
            _directory: directory,
        }
    }

    pub fn registry_url(&self) -> &str {
        &self.registry_url
    }
}

fn name(index: usize) -> String {
    format!("pkg-{index:04}")
}

/// The last `roots` packages, which declare nothing themselves and which every
/// other package depends on — the `tslib`/`ms`/`inherits` end of a real tree,
/// where the many-dependents-plus-one-pin duplicates come from.
fn popular(shape: &Shape) -> std::ops::Range<usize> {
    shape
        .packages
        .saturating_sub(shape.roots.min(shape.packages))..shape.packages
}

/// Package `index` depends on a contiguous block further along the numbering,
/// plus one member of the popular pool. Every dependency has a strictly higher
/// index than its dependent, which makes the graph acyclic by construction.
fn children(shape: &Shape, index: usize) -> impl Iterator<Item = usize> {
    let pool = popular(shape);
    let start = shape.roots + index * shape.fanout;
    let block = (start..start + shape.fanout).filter(move |child| *child < pool.start);
    let shared = pool.start + index % pool.len().max(1);
    block.chain((index < pool.start).then_some(shared))
}

fn publish(shape: &Shape, index: usize, version: usize) -> Package {
    let newest = version + 1 == shape.versions;
    let mut package = Package::new(&name(index), &format!("1.0.{version}"));

    for child in children(shape, index) {
        let spec = if (index + child).is_multiple_of(PIN_EVERY) && shape.versions > 1 {
            "=1.0.0"
        } else {
            "^1.0.0"
        };
        package = package.dependency(&name(child), spec);
    }

    if !newest {
        return package;
    }
    for file in 0..shape.files {
        // Distinct per package so the CAS cannot dedupe the payload away.
        let contents = format!(
            "// {} file {file}\n{}\n",
            name(index),
            "x".repeat(FILE_PADDING)
        );
        package = package.file(&format!("lib/mod-{file}.js"), &contents);
    }
    // One byte-identical file across every package, so the run also exercises
    // the cross-package sharing the CAS exists for.
    package = package.file("LICENSE", "MIT, for benchmarking purposes only.\n");
    if index.is_multiple_of(10) {
        package = package
            .executable("bin/cli.js", "#!/usr/bin/env node\nconsole.log(1);\n")
            .bin(&format!("cli-{index}"), "bin/cli.js");
    }
    package
}

fn bytes_in(directory: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .filter(std::fs::Metadata::is_file)
        .map(|metadata| metadata.len())
        .sum()
}
