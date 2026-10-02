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
//! Size matters too, for a different question. The default workload's files
//! are padding that gzip reduces to nothing, so a download costs a round trip
//! and no transfer, and the benchmark can only show time spent waiting on
//! round trips. Measured on the Next.js scaffold (2026-10-01), that is not
//! what bounds a real cold install: 359 tarballs are 157 MB on the wire and
//! unpack to 21,676 files and 544 MB, and downloading them 16 at a time took
//! 34s with no install work at all. [`Shape::scaffold`] reproduces those
//! totals, and with `--bandwidth-mbit` the benchmark prices the transfer.
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
    /// Bytes in each ordinary file.
    pub file_bytes: usize,
    /// Every nth package also ships one large file, the way a few native
    /// binaries hold most of a real tree's bytes. Zero for none.
    pub heavy_every: usize,
    pub heavy_bytes: usize,
    /// Whether file contents resist compression about as well as real
    /// package contents do. Off, files are padding that compresses away.
    pub dense: bool,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            packages: 64,
            versions: 12,
            files: 6,
            fanout: 2,
            roots: 8,
            file_bytes: 1900,
            heavy_every: 0,
            heavy_bytes: 0,
            dense: false,
        }
    }
}

impl Shape {
    /// The Next.js scaffold's totals, as measured on 2026-10-01: 364
    /// packages, about 21,700 files, about 540 MB unpacked and about 155 MB
    /// of tarballs. The real tree's three largest tarballs are 33 to 42 MB;
    /// here the weight is spread evenly over 52 packages instead.
    pub fn scaffold() -> Self {
        Self {
            packages: 364,
            files: 59,
            file_bytes: 2_000,
            heavy_every: 7,
            heavy_bytes: 9_600_000,
            dense: true,
            ..Self::default()
        }
    }
}

/// One in seven edges onto the popular pool pins an exact old version, so the
/// tree contains duplicates and the linker has to nest. A tree that hoists
/// perfectly flat is not a tree anyone installs.
const PIN_EVERY: usize = 7;

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
            payload(shape, index, file, shape.file_bytes)
        );
        package = package.file(&format!("lib/mod-{file}.js"), &contents);
    }
    if shape.heavy_every > 0 && index.is_multiple_of(shape.heavy_every) {
        package = package.file(
            "build/native.node",
            &payload(shape, index, shape.files, shape.heavy_bytes),
        );
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

/// `bytes` of file content: padding, or text that gzip can only shrink to
/// about 29% of its size, which is the ratio the scaffold's tarballs have
/// (157 MB for 544 MB unpacked).
fn payload(shape: &Shape, index: usize, file: usize, bytes: usize) -> String {
    if !shape.dense {
        return "x".repeat(bytes);
    }
    // Four symbols drawn evenly carry two bits a byte, which deflate cannot
    // do much better than. The generator is SplitMix64, seeded per file so
    // no two files share content.
    const SYMBOLS: [u8; 4] = *b"acgt";
    let mut state = (index as u64) << 32 | file as u64;
    let mut text = Vec::with_capacity(bytes);
    while text.len() < bytes {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^= mixed >> 31;
        for pair in 0..32 {
            if text.len() == bytes {
                break;
            }
            text.push(SYMBOLS[(mixed >> (pair * 2)) as usize & 3]);
        }
    }
    String::from_utf8(text).expect("ASCII")
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
