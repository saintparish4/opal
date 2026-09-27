//! `opal upgrade`: replace this binary with a release from GitHub.
//!
//! The same steps as `install.sh`, from inside the binary: pick the asset for
//! this OS and CPU, download it, check it against the release's `SHA256SUMS`,
//! and put it in place. Two rules keep a failed upgrade harmless:
//!
//! - The new binary is written next to the old one, run with `--version`, and
//!   renamed over it only if it starts and reports the version that was asked
//!   for. A corrupt or truncated download, or one built for a glibc this host
//!   doesn't have, fails that check, and the old binary is never touched.
//! - The swap is a single `rename(2)` within one directory, so a kill at any
//!   point leaves the old binary or the new one, never half of each. Renaming
//!   over a running executable is fine on macOS and Linux: the running process
//!   keeps the old inode.
//!
//! GitHub Releases are the only source of binaries. `OPAL_INTERNAL_RELEASES_URL`
//! points this at a stand-in (a `file://` directory in the tests), the way
//! `OPAL_INTERNAL_FAULT_INJECT` drives the crash suite.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use opal_core::atomic::TempFile;
use opal_pm::registry::{Fetched, RegistryError, Request, Transport};
use opal_pm::semver::Version;
use sha2::{Digest, Sha256};

const REPO: &str = "saintparish4/opal";
pub const RELEASES_ENV: &str = "OPAL_INTERNAL_RELEASES_URL";
/// A release binary is about 7 MB. An archive entry past this isn't one.
const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;

/// Where releases are read from.
pub struct Releases {
    latest: String,
    download: String,
}

impl Releases {
    pub fn github() -> Self {
        Self {
            latest: format!("https://api.github.com/repos/{REPO}/releases/latest"),
            download: format!("https://github.com/{REPO}/releases/download"),
        }
    }

    /// A stand-in laid out as `<base>/latest` (GitHub's JSON for the latest
    /// release) and `<base>/download/<tag>/<asset>`.
    pub fn at(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            latest: format!("{base}/latest"),
            download: format!("{base}/download"),
        }
    }

    pub fn from_env() -> Self {
        match std::env::var(RELEASES_ENV) {
            Ok(base) if !base.is_empty() => Self::at(&base),
            _ => Self::github(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Upgraded {
        from: Version,
        to: Version,
    },
    AlreadyLatest(Version),
    AlreadyInstalled(Version),
    /// A build newer than anything released, such as one from `master`.
    NewerThanLatest {
        current: Version,
        latest: Version,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum UpgradeError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error("opal has no release for {os}/{arch}; releases cover Linux and macOS on x64 and arm64")]
    UnsupportedPlatform { os: String, arch: String },
    #[error("{0:?} isn't a version; try one like 0.3.1")]
    BadVersion(String),
    #[error("there is no opal {0} release")]
    NoSuchRelease(Version),
    #[error("couldn't read the latest release's version from {0}")]
    NoLatestVersion(String),
    #[error("{asset} isn't listed in the release's SHA256SUMS; nothing was changed")]
    NotListed { asset: String },
    #[error(
        "checksum mismatch for {asset}: expected {expected}, got {actual}; nothing was changed"
    )]
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
    #[error("{asset} has no `opal` binary in it; nothing was changed")]
    NoBinary { asset: String },
    #[error("the downloaded opal didn't start ({reason}); nothing was changed")]
    WontRun { reason: String },
    #[error("the downloaded binary reports {reported:?}, not opal {expected}; nothing was changed")]
    WrongVersion { expected: Version, reported: String },
    #[error("can't replace {}: {source}", path.display())]
    Replace { path: PathBuf, source: io::Error },
    #[error("couldn't read the downloaded archive: {0}")]
    Archive(#[from] io::Error),
}

/// Moves `executable` to `requested`, or to the latest release when `None`.
/// `say` hears each step as it starts.
pub fn upgrade(
    releases: &Releases,
    transport: &dyn Transport,
    requested: Option<&str>,
    current: &Version,
    executable: &Path,
    (os, arch): (&str, &str),
    say: &mut dyn FnMut(&str),
) -> Result<Outcome, UpgradeError> {
    let platform = platform_name(os, arch)?;
    let target = match requested {
        Some(text) => {
            let target = parse_version(text)?;
            if &target == current {
                return Ok(Outcome::AlreadyInstalled(target));
            }
            target
        }
        None => {
            let latest = latest_version(releases, transport)?;
            match latest.cmp(current) {
                std::cmp::Ordering::Equal => return Ok(Outcome::AlreadyLatest(latest)),
                std::cmp::Ordering::Less => {
                    return Ok(Outcome::NewerThanLatest {
                        current: current.clone(),
                        latest,
                    });
                }
                std::cmp::Ordering::Greater => latest,
            }
        }
    };

    let asset = format!("opal-{platform}.tar.gz");
    let release = format!("{}/v{target}", releases.download);
    say(&format!("downloading opal {target} for {platform}"));
    let sums = download(transport, &format!("{release}/SHA256SUMS"), &target)?;
    let archive = download(transport, &format!("{release}/{asset}"), &target)?;
    verify(&asset, &sums, &archive)?;
    say("checksum verified");
    let binary = extract(&asset, &archive)?;
    replace(executable, &binary, &target)?;
    Ok(Outcome::Upgraded {
        from: current.clone(),
        to: target,
    })
}

/// The release matrix's names: `linux-x64`, `macos-arm64`, and so on.
fn platform_name(os: &str, arch: &str) -> Result<String, UpgradeError> {
    let unsupported = || UpgradeError::UnsupportedPlatform {
        os: os.to_string(),
        arch: arch.to_string(),
    };
    let os_name = match os {
        "linux" => "linux",
        "macos" => "macos",
        _ => return Err(unsupported()),
    };
    let arch_name = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return Err(unsupported()),
    };
    Ok(format!("{os_name}-{arch_name}"))
}

/// Accepts `0.3.1` and `v0.3.1`, since release tags carry the `v`.
fn parse_version(text: &str) -> Result<Version, UpgradeError> {
    let bare = text.trim().strip_prefix('v').unwrap_or(text.trim());
    Version::parse(bare).map_err(|_| UpgradeError::BadVersion(text.to_string()))
}

fn latest_version(releases: &Releases, transport: &dyn Transport) -> Result<Version, UpgradeError> {
    let body = fetch(
        transport,
        &releases.latest,
        Some("application/vnd.github+json"),
    )?;
    let tag = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|release| release.get("tag_name")?.as_str().map(str::to_string))
        .ok_or_else(|| UpgradeError::NoLatestVersion(releases.latest.clone()))?;
    parse_version(&tag).map_err(|_| UpgradeError::NoLatestVersion(releases.latest.clone()))
}

fn fetch(
    transport: &dyn Transport,
    url: &str,
    accept: Option<&str>,
) -> Result<Vec<u8>, RegistryError> {
    let request = Request {
        url,
        accept,
        etag: None,
    };
    match transport.get(&request)? {
        Fetched::Fresh(response) => Ok(response.body),
        // No etag was sent, so a server has nothing to confirm.
        Fetched::NotModified => Err(RegistryError::Transport {
            url: url.to_string(),
            message: "not modified, though nothing was cached".to_string(),
        }),
    }
}

/// A release file. A 404 here means the version was never released.
fn download(
    transport: &dyn Transport,
    url: &str,
    target: &Version,
) -> Result<Vec<u8>, UpgradeError> {
    fetch(transport, url, None).map_err(|error| match error {
        RegistryError::Status { status: 404, .. } => UpgradeError::NoSuchRelease(target.clone()),
        other => UpgradeError::Registry(other),
    })
}

/// Checks `archive` against its line in `SHA256SUMS` (`shasum -a 256` output).
fn verify(asset: &str, sums: &[u8], archive: &[u8]) -> Result<(), UpgradeError> {
    let expected = String::from_utf8_lossy(sums)
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let hash = fields.next()?;
            // shasum marks binary mode with a leading `*` on the name.
            let name = fields.next()?.trim_start_matches('*');
            (name == asset).then(|| hash.to_ascii_lowercase())
        })
        .ok_or_else(|| UpgradeError::NotListed {
            asset: asset.to_string(),
        })?;
    let actual = hex(&Sha256::digest(archive));
    if actual == expected {
        Ok(())
    } else {
        Err(UpgradeError::ChecksumMismatch {
            asset: asset.to_string(),
            expected,
            actual,
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The `opal` entry of a release archive, which also holds `LICENSE`.
fn extract(asset: &str, archive: &[u8]) -> Result<Vec<u8>, UpgradeError> {
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    for entry in tar.entries()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();
        if path.components().count() != 1 || path.file_name() != Some("opal".as_ref()) {
            continue;
        }
        let mut binary = Vec::new();
        entry.take(MAX_BINARY_BYTES + 1).read_to_end(&mut binary)?;
        if binary.len() as u64 > MAX_BINARY_BYTES {
            break;
        }
        return Ok(binary);
    }
    Err(UpgradeError::NoBinary {
        asset: asset.to_string(),
    })
}

/// Writes the new binary beside the old one, proves it runs, then renames it
/// into place. Until the rename, the old binary is untouched.
fn replace(executable: &Path, binary: &[u8], expected: &Version) -> Result<(), UpgradeError> {
    // Through a symlink to the real file, so the link keeps pointing at it.
    let path = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.to_path_buf());
    let replace_error = |source: io::Error| UpgradeError::Replace {
        path: path.clone(),
        source,
    };
    let directory = path
        .parent()
        .ok_or_else(|| replace_error(io::Error::other("no parent directory")))?;

    let mut temp = TempFile::create(directory, ".opal-upgrade").map_err(replace_error)?;
    temp.file_mut().write_all(binary).map_err(replace_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o755))
            .map_err(replace_error)?;
    }
    // Closed before it runs: executing a file still open for writing fails
    // with ETXTBSY on Linux.
    temp.sync_and_close().map_err(replace_error)?;
    check_runs(temp.path(), expected)?;
    temp.persist(&path).map_err(replace_error)
}

/// Runs `binary --version`, waiting out `ETXTBSY`.
///
/// Linux won't execute a file that any process has open for writing. The temp
/// file is closed before this runs, but a process started from another thread
/// at the wrong moment briefly holds a copy of every open file, this one
/// included, until it starts its own program. Seen in CI when the tests ran in
/// parallel (2 in 300 stress runs locally); `opal upgrade` itself is single
/// threaded. The wait doubles from 5ms, about 1.3s in all.
fn run_version(binary: &Path) -> io::Result<std::process::Output> {
    let mut pause = std::time::Duration::from_millis(5);
    for _ in 0..8 {
        match Command::new(binary).arg("--version").output() {
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(pause);
                pause *= 2;
            }
            outcome => return outcome,
        }
    }
    Command::new(binary).arg("--version").output()
}

fn check_runs(binary: &Path, expected: &Version) -> Result<(), UpgradeError> {
    let output = run_version(binary).map_err(|error| UpgradeError::WontRun {
        reason: error.to_string(),
    })?;
    if !output.status.success() {
        return Err(UpgradeError::WontRun {
            reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if reported == format!("opal {expected}") {
        Ok(())
    } else {
        Err(UpgradeError::WrongVersion {
            expected: expected.clone(),
            reported,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opal_pm::registry::HttpTransport;
    use std::fs;

    const PLATFORM: (&str, &str) = ("linux", "x86_64");
    const ASSET: &str = "opal-linux-x64.tar.gz";

    /// A binary that prints `opal <version>`, as a release build does.
    fn fake_binary(version: &str) -> Vec<u8> {
        format!("#!/bin/sh\necho \"opal {version}\"\n").into_bytes()
    }

    fn archive(binary: &[u8]) -> Vec<u8> {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        for (name, contents) in [("opal", binary), ("LICENSE", b"MIT".as_slice())] {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, name, contents)
                .expect("append");
        }
        tar.into_inner().expect("tar").finish().expect("gzip")
    }

    /// A release directory as GitHub lays it out, readable over `file://`.
    struct Fixture {
        _directory: tempfile::TempDir,
        base: PathBuf,
        executable: PathBuf,
    }

    impl Fixture {
        fn new(installed: &str) -> Self {
            let directory = tempfile::tempdir().expect("temp dir");
            let base = directory.path().join("releases");
            fs::create_dir_all(&base).expect("releases dir");
            let bin = directory.path().join("bin");
            fs::create_dir_all(&bin).expect("bin dir");
            let executable = bin.join("opal");
            fs::write(&executable, fake_binary(installed)).expect("install");
            Self {
                _directory: directory,
                base,
                executable,
            }
        }

        fn latest(&self, tag: &str) -> &Self {
            let body = format!("{{\"tag_name\": \"{tag}\"}}");
            fs::write(self.base.join("latest"), body).expect("latest");
            self
        }

        /// Publishes a release whose binary reports `reports`, with a checksum
        /// that is correct unless `tamper` is set.
        fn release(&self, tag: &str, reports: &str, tamper: bool) -> &Self {
            let directory = self.base.join("download").join(tag);
            fs::create_dir_all(&directory).expect("release dir");
            let archive = archive(&fake_binary(reports));
            let mut digest = hex(&Sha256::digest(&archive));
            if tamper {
                digest = digest.replace(&digest[..1], if &digest[..1] == "0" { "1" } else { "0" });
            }
            fs::write(directory.join(ASSET), &archive).expect("archive");
            fs::write(directory.join("SHA256SUMS"), format!("{digest}  {ASSET}\n")).expect("sums");
            self
        }

        fn run(&self, requested: Option<&str>, current: &str) -> Result<Outcome, UpgradeError> {
            let releases = Releases::at(&format!("file://{}", self.base.display()));
            upgrade(
                &releases,
                &HttpTransport::new(),
                requested,
                &Version::parse(current).expect("version"),
                &self.executable,
                PLATFORM,
                &mut |_| {},
            )
        }

        fn installed(&self) -> String {
            fs::read_to_string(&self.executable).expect("read binary")
        }
    }

    fn version(text: &str) -> Version {
        Version::parse(text).expect("version")
    }

    #[test]
    fn test_platform_names_match_the_release_matrix() {
        assert_eq!(platform_name("linux", "x86_64").unwrap(), "linux-x64");
        assert_eq!(platform_name("linux", "aarch64").unwrap(), "linux-arm64");
        assert_eq!(platform_name("macos", "x86_64").unwrap(), "macos-x64");
        assert_eq!(platform_name("macos", "aarch64").unwrap(), "macos-arm64");
        assert!(matches!(
            platform_name("windows", "x86_64"),
            Err(UpgradeError::UnsupportedPlatform { .. })
        ));
    }

    #[test]
    fn test_a_version_reads_with_or_without_its_v() {
        assert_eq!(parse_version("0.3.1").unwrap(), version("0.3.1"));
        assert_eq!(parse_version("v0.3.1").unwrap(), version("0.3.1"));
        assert!(matches!(
            parse_version("latest"),
            Err(UpgradeError::BadVersion(_))
        ));
    }

    #[test]
    fn test_an_upgrade_replaces_the_binary_with_the_new_release() {
        let fixture = Fixture::new("0.3.1");
        fixture.latest("v0.4.0").release("v0.4.0", "0.4.0", false);

        let outcome = fixture.run(None, "0.3.1").expect("upgrade");

        assert_eq!(
            outcome,
            Outcome::Upgraded {
                from: version("0.3.1"),
                to: version("0.4.0")
            }
        );
        assert!(fixture.installed().contains("opal 0.4.0"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&fixture.executable)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[test]
    fn test_a_named_version_installs_even_if_older() {
        let fixture = Fixture::new("0.3.1");
        fixture.release("v0.3.0", "0.3.0", false);

        let outcome = fixture.run(Some("0.3.0"), "0.3.1").expect("downgrade");

        assert_eq!(
            outcome,
            Outcome::Upgraded {
                from: version("0.3.1"),
                to: version("0.3.0")
            }
        );
        assert!(fixture.installed().contains("opal 0.3.0"));
    }

    #[test]
    fn test_the_latest_release_leaves_everything_alone() {
        let fixture = Fixture::new("0.3.1");
        fixture.latest("v0.3.1");

        assert_eq!(
            fixture.run(None, "0.3.1").expect("check"),
            Outcome::AlreadyLatest(version("0.3.1"))
        );
        assert!(fixture.installed().contains("opal 0.3.1"));
    }

    #[test]
    fn test_a_build_newer_than_any_release_is_left_alone() {
        let fixture = Fixture::new("0.4.0");
        fixture.latest("v0.3.1");

        assert_eq!(
            fixture.run(None, "0.4.0").expect("check"),
            Outcome::NewerThanLatest {
                current: version("0.4.0"),
                latest: version("0.3.1")
            }
        );
    }

    #[test]
    fn test_a_checksum_mismatch_changes_nothing() {
        let fixture = Fixture::new("0.3.1");
        fixture.latest("v0.4.0").release("v0.4.0", "0.4.0", true);

        assert!(matches!(
            fixture.run(None, "0.3.1"),
            Err(UpgradeError::ChecksumMismatch { .. })
        ));
        assert!(fixture.installed().contains("opal 0.3.1"));
    }

    #[test]
    fn test_a_binary_reporting_the_wrong_version_is_not_installed() {
        let fixture = Fixture::new("0.3.1");
        fixture.latest("v0.4.0").release("v0.4.0", "0.2.0", false);

        assert!(matches!(
            fixture.run(None, "0.3.1"),
            Err(UpgradeError::WrongVersion { .. })
        ));
        assert!(fixture.installed().contains("opal 0.3.1"));
    }

    #[test]
    fn test_a_version_that_was_never_released_says_so() {
        let fixture = Fixture::new("0.3.1");

        assert!(matches!(
            fixture.run(Some("9.9.9"), "0.3.1"),
            Err(UpgradeError::NoSuchRelease(_))
        ));
        assert!(fixture.installed().contains("opal 0.3.1"));
    }

    #[cfg(unix)]
    #[test]
    fn test_upgrading_through_a_symlink_replaces_its_target() {
        let fixture = Fixture::new("0.3.1");
        fixture.latest("v0.4.0").release("v0.4.0", "0.4.0", false);
        let link = fixture.base.join("opal-link");
        std::os::unix::fs::symlink(&fixture.executable, &link).expect("symlink");
        let releases = Releases::at(&format!("file://{}", fixture.base.display()));

        upgrade(
            &releases,
            &HttpTransport::new(),
            None,
            &version("0.3.1"),
            &link,
            PLATFORM,
            &mut |_| {},
        )
        .expect("upgrade");

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fixture.installed().contains("opal 0.4.0"));
    }

    #[test]
    fn test_a_failed_upgrade_leaves_no_temp_file_behind() {
        let fixture = Fixture::new("0.3.1");
        fixture.latest("v0.4.0").release("v0.4.0", "0.2.0", false);
        let _ = fixture.run(None, "0.3.1");

        let entries: Vec<_> = fs::read_dir(fixture.executable.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, ["opal"]);
    }
}
