//! The JavaScript half of opal test, embedded in the binary.
//!
//! The files under `runtime/` are written once into the cache, in a folder
//! named for a hash of their contents, so a new opal never runs an old
//! runtime and two versions side by side never share one. `OPAL_TEST_RUNTIME`
//! points at a folder to use as is instead, which is how the runtime is
//! worked on without rebuilding.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/runtime_files.rs"));

/// The folder holding `worker.cjs`.
pub fn unpack(cache: &Path) -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("OPAL_TEST_RUNTIME") {
        return Ok(PathBuf::from(dir));
    }
    let mut hasher = blake3::Hasher::new();
    for (name, source) in FILES {
        hasher.update(name.as_bytes());
        hasher.update(&[0]);
        hasher.update(source.as_bytes());
        hasher.update(&[0]);
    }
    let hash = hasher.finalize().to_hex();
    let base = cache.join("test-runtime");
    let dir = base.join(&hash.as_str()[..16]);
    if dir.join("worker.cjs").is_file() {
        return Ok(dir);
    }
    fs::create_dir_all(&base)?;
    let staging = base.join(format!(".{}-{}", &hash.as_str()[..16], std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    for (name, source) in FILES {
        fs::write(staging.join(name), source)?;
    }
    // Another opal may have unpacked the same runtime first; either copy is
    // the same bytes, so losing the race is fine.
    if fs::rename(&staging, &dir).is_err() {
        let _ = fs::remove_dir_all(&staging);
        if !dir.join("worker.cjs").is_file() {
            return Err(io::Error::other(format!(
                "could not unpack the test runtime into {}",
                dir.display()
            )));
        }
    }
    Ok(dir)
}
