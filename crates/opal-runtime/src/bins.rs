//! `opal x <package>`: which executable a package name means.
//!
//! The reference is npm 12's `libnpmexec`. Before anything is installed, a
//! bare command is looked for where it is already: as a `bin` of the package
//! in the current directory, then in `node_modules/.bin` from the current
//! directory up to `/`. Only then is it taken as a package to install, and
//! the command run is the package's single bin, or the one named after the
//! package with its scope dropped (`getBinFromManifest`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::scripts::is_executable;

/// `node_modules/.bin/<command>` in `start` or the nearest folder above it
/// that has one.
pub fn find_local_bin(start: &Path, command: &str) -> Option<PathBuf> {
    if command.is_empty() || command.contains('/') {
        return None;
    }
    start
        .ancestors()
        .map(|dir| dir.join("node_modules").join(".bin").join(command))
        .find(|candidate| is_executable(candidate))
}

/// The command to run from a package whose `bin` is `bin`: its only one (or
/// several names for one file), else the one named after the package.
pub fn command_for(package: &str, bin: &BTreeMap<String, String>) -> Option<String> {
    let mut targets = bin.values();
    if let Some(first) = targets.next()
        && targets.all(|target| target == first)
    {
        return bin.keys().next().cloned();
    }
    let unscoped = package.rsplit('/').next().unwrap_or(package);
    bin.contains_key(unscoped).then(|| unscoped.to_string())
}

/// `name` and the version or range after it in `name@range`, with a scope's
/// leading `@` not taken for the separator.
pub fn split_spec(spec: &str) -> (&str, Option<&str>) {
    let search_from = usize::from(spec.starts_with('@'));
    match spec[search_from..].find('@') {
        Some(at) => {
            let at = at + search_from;
            (&spec[..at], Some(&spec[at + 1..]))
        }
        None => (spec, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bins(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_single_bin_is_the_command_whatever_its_name() {
        assert_eq!(
            command_for("typescript", &bins(&[("tsc", "bin/tsc")])),
            Some("tsc".into())
        );
    }

    #[test]
    fn aliases_of_one_file_count_as_one_bin() {
        let aliases = bins(&[("cowsay", "cli.js"), ("cowthink", "cli.js")]);
        assert_eq!(command_for("cowsay", &aliases), Some("cowsay".into()));
    }

    #[test]
    fn several_bins_pick_the_one_named_after_the_package_without_its_scope() {
        let several = bins(&[("tool", "a.js"), ("tool-helper", "b.js")]);
        assert_eq!(command_for("@acme/tool", &several), Some("tool".into()));
        assert_eq!(command_for("other", &several), None);
    }

    #[test]
    fn specs_split_at_the_version_not_the_scope() {
        assert_eq!(split_spec("cowsay"), ("cowsay", None));
        assert_eq!(split_spec("cowsay@1.6.0"), ("cowsay", Some("1.6.0")));
        assert_eq!(split_spec("@scope/pkg"), ("@scope/pkg", None));
        assert_eq!(split_spec("@scope/pkg@^2"), ("@scope/pkg", Some("^2")));
    }

    #[test]
    fn local_bins_are_found_from_the_nearest_folder_up() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("tool"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join("tool"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let nested = root.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_local_bin(&nested, "tool"), Some(bin.join("tool")));
        assert_eq!(find_local_bin(&nested, "missing"), None);
        assert_eq!(find_local_bin(&nested, "../tool"), None);
    }
}
