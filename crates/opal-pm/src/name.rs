//! What counts as a package name.
//!
//! A dependency's name becomes a directory: `node_modules/<name>`. It arrives
//! from places a stranger controls — a `package.json`, a registry manifest, an
//! `opal.lock` — so a name is checked before it is used as a path. Without the
//! check, a dependency named `../../x` is a directory outside the project, and
//! installing it deletes that directory and writes a package there.
//!
//! One rule, used everywhere a name enters: the command line (`edit`), the
//! resolver, the lockfile parser, and the linker's last check before it
//! touches the disk.

/// npm's own rule: a name is what survives `encodeURIComponent` unchanged,
/// optionally behind one `@scope/`.
///
/// That is also what keeps a path (`./lib`), a URL, and `github:user/repo`
/// from being read as a package: each has a `/` or a `:` where a name cannot.
/// A part may not start with a dot, which rules out `.`, `..`, and `.bin`.
pub fn is_package_name(name: &str) -> bool {
    fn part(text: &str) -> bool {
        !text.is_empty()
            && !text.starts_with('.')
            && text.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
                    )
            })
    }
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, package)| part(scope) && part(package)),
        None => part(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordinary_and_scoped_names_are_names() {
        for name in ["express", "is-number", "JSONStream", "@types/node", "a.b_c"] {
            assert!(is_package_name(name), "{name}");
        }
    }

    #[test]
    fn test_a_path_is_not_a_name() {
        for name in [
            "",
            ".",
            "..",
            "../victim",
            "../../victim",
            "/etc/passwd",
            "a/b",
            "a\\b",
            ".bin",
            "@scope",
            "@scope/",
            "@scope/../x",
            "@../x",
            "@scope/a/b",
            "a\0b",
            "a b",
        ] {
            assert!(!is_package_name(name), "{name:?}");
        }
    }

    #[test]
    fn test_a_url_or_a_git_shorthand_is_not_a_name() {
        for name in ["https://example.com/a.tgz", "github:user/repo", "file:../x"] {
            assert!(!is_package_name(name), "{name}");
        }
    }
}
