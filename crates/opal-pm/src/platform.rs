//! npm's `os` and `cpu` constraints, matched against a host.
//!
//! A package that declares `"os": ["darwin"]` cannot run on Linux, and npm
//! skips it rather than installing it. Without this, every platform variant of
//! a native optional dependency resolves and downloads: `@next/swc-*` alone
//! came to 714M on a Linux host where npm installs one binary.
//!
//! The host is a value, not a `cfg!`, for two reasons: the filter runs when a
//! tree is planned rather than when it is resolved, so `opal.lock` stays
//! portable across platforms — and a test can ask what a macOS host would
//! install without being one.

/// npm's vocabulary for a host: `process.platform` and `process.arch`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Platform {
    os: String,
    cpu: String,
}

impl Platform {
    pub fn new(os: impl Into<String>, cpu: impl Into<String>) -> Self {
        Self {
            os: os.into(),
            cpu: cpu.into(),
        }
    }

    /// This machine, in npm's spelling.
    pub fn host() -> Self {
        Self::new(
            host_os(std::env::consts::OS),
            host_cpu(std::env::consts::ARCH),
        )
    }

    pub fn os(&self) -> &str {
        &self.os
    }

    pub fn cpu(&self) -> &str {
        &self.cpu
    }

    /// Whether a package declaring these constraints can run here.
    pub fn supports(&self, os: &[String], cpu: &[String]) -> bool {
        satisfies(&self.os, os) && satisfies(&self.cpu, cpu)
    }

    /// Why a package was skipped, for the install report.
    pub fn rejection(&self, os: &[String], cpu: &[String]) -> String {
        let mut reasons = Vec::new();
        if !satisfies(&self.os, os) {
            reasons.push(format!("os {} (host is {})", os.join(","), self.os));
        }
        if !satisfies(&self.cpu, cpu) {
            reasons.push(format!("cpu {} (host is {})", cpu.join(","), self.cpu));
        }
        format!("requires {}", reasons.join(" and "))
    }
}

/// npm's rule: an empty list allows anything, a `!` entry excludes, and once
/// any plain entry is present the host has to be one of them.
fn satisfies(host: &str, constraints: &[String]) -> bool {
    if constraints.is_empty() {
        return true;
    }
    let mut allowed = Vec::new();
    for constraint in constraints {
        match constraint.strip_prefix('!') {
            Some(excluded) if excluded == host => return false,
            Some(_) => {}
            None => allowed.push(constraint.as_str()),
        }
    }
    allowed.is_empty() || allowed.contains(&host)
}

/// `std::env::consts::OS` to `process.platform`. Only the spellings that
/// differ are listed; anything else npm and Rust already agree on.
fn host_os(os: &str) -> &str {
    match os {
        "macos" | "ios" => "darwin",
        "windows" => "win32",
        "solaris" => "sunos",
        other => other,
    }
}

/// `std::env::consts::ARCH` to `process.arch`.
fn host_cpu(arch: &str) -> &str {
    match arch {
        "x86_64" => "x64",
        "x86" => "ia32",
        "aarch64" => "arm64",
        "powerpc" => "ppc",
        "powerpc64" => "ppc64",
        "loongarch64" => "loong64",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    fn linux() -> Platform {
        Platform::new("linux", "x64")
    }

    #[test]
    fn test_no_constraints_run_anywhere() {
        assert!(linux().supports(&[], &[]));
    }

    #[test]
    fn test_an_allow_list_must_contain_the_host() {
        assert!(linux().supports(&list(&["linux", "darwin"]), &[]));
        assert!(!linux().supports(&list(&["darwin"]), &[]));
    }

    #[test]
    fn test_a_negation_excludes_only_what_it_names() {
        assert!(linux().supports(&list(&["!win32"]), &[]));
        assert!(!linux().supports(&list(&["!linux"]), &[]));
    }

    #[test]
    fn test_a_negation_wins_over_an_allow_list() {
        assert!(!linux().supports(&list(&["linux", "!linux"]), &[]));
    }

    #[test]
    fn test_cpu_is_checked_independently_of_os() {
        assert!(!linux().supports(&list(&["linux"]), &list(&["arm64"])));
        assert!(linux().supports(&list(&["linux"]), &list(&["x64", "arm64"])));
    }

    #[test]
    fn test_host_uses_npms_spelling() {
        assert_eq!(host_os("macos"), "darwin");
        assert_eq!(host_os("windows"), "win32");
        assert_eq!(host_os("linux"), "linux");
        assert_eq!(host_cpu("x86_64"), "x64");
        assert_eq!(host_cpu("aarch64"), "arm64");
    }

    #[test]
    fn test_rejection_names_the_constraint_and_the_host() {
        let reason = linux().rejection(&list(&["darwin"]), &[]);
        assert_eq!(reason, "requires os darwin (host is linux)");
    }
}
