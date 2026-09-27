//! `opal upgrade`, driven through the real binary.
//!
//! The binary is copied into a directory of its own, as `install.sh` puts it
//! in `~/.opal/bin`, and pointed at a release laid out on disk the way GitHub
//! serves one, through `OPAL_INTERNAL_RELEASES_URL`. The release's "binary" is
//! a shell script that prints its version, which is all an upgrade checks
//! before swapping it in.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

const OPAL: &str = env!("CARGO_BIN_EXE_opal");
const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn asset() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        _ => "linux",
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "x64",
    };
    format!("opal-{os}-{arch}.tar.gz")
}

fn publish(releases: &Path, version: &str) {
    let script = format!("#!/bin/sh\necho \"opal {version}\"\n");
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, "opal", script.as_bytes())
        .expect("append");
    let archive = tar.into_inner().expect("tar").finish().expect("gzip");
    let digest: String = Sha256::digest(&archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();

    let directory = releases.join("download").join(format!("v{version}"));
    fs::create_dir_all(&directory).expect("release dir");
    fs::write(directory.join(asset()), &archive).expect("archive");
    fs::write(
        directory.join("SHA256SUMS"),
        format!("{digest}  {}\n", asset()),
    )
    .expect("sums");
    fs::write(
        releases.join("latest"),
        format!("{{\"tag_name\": \"v{version}\"}}"),
    )
    .expect("latest");
}

/// A copy of the built binary, alone in a `bin` directory.
fn installed(root: &Path) -> PathBuf {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).expect("bin dir");
    let copy = bin.join("opal");
    fs::copy(OPAL, &copy).expect("copy binary");
    copy
}

fn upgrade(binary: &Path, releases: &Path) -> Output {
    Command::new(binary)
        .arg("upgrade")
        .env(
            "OPAL_INTERNAL_RELEASES_URL",
            format!("file://{}", releases.display()),
        )
        .output()
        .expect("run opal upgrade")
}

fn version_of(binary: &Path) -> String {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .expect("run --version");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn test_upgrade_swaps_in_the_latest_release() {
    let directory = tempfile::tempdir().expect("temp dir");
    let releases = directory.path().join("releases");
    publish(&releases, "99.0.0");
    let binary = installed(directory.path());

    let output = upgrade(&binary, &releases);

    assert!(
        output.status.success(),
        "upgrade failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("opal {CURRENT} → 99.0.0")
    );
    assert_eq!(version_of(&binary), "opal 99.0.0");
}

#[test]
fn test_upgrade_on_the_latest_release_changes_nothing() {
    let directory = tempfile::tempdir().expect("temp dir");
    let releases = directory.path().join("releases");
    fs::create_dir_all(&releases).expect("releases dir");
    fs::write(
        releases.join("latest"),
        format!("{{\"tag_name\": \"v{CURRENT}\"}}"),
    )
    .expect("latest");
    let binary = installed(directory.path());
    let before = fs::read(&binary).expect("read binary");

    let output = upgrade(&binary, &releases);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("opal {CURRENT} is already the latest release")
    );
    assert_eq!(fs::read(&binary).expect("read binary"), before);
}
