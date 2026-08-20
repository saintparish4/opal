//! The one place path handling lives.
//!
//! Every path that reaches the resolver, the CAS, or the memoization layer is a
//! [`NormalizedPath`]: UTF-8, `/`-separated, and lexically normalized (no `.`,
//! no interior `..`, no repeated or trailing separators). Two spellings of the
//! same location therefore compare equal and hash equal, which is what lets the
//! graph use paths as identity and the memo layer use them as cache keys.
//!
//! v1 targets macOS, Linux, and WSL2 only. This module is nonetheless written as
//! if Windows already existed — it recognizes `\` as a separator and `C:/`-style
//! roots, and it exposes case-folding so callers can reason about
//! case-insensitive filesystems — because retrofitting that into every call site
//! later is the expensive version of the same work.
//!

use std::fmt;
use std::path::{Path, PathBuf};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Failure to bring an OS path into normalized form.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    /// Opal treats paths as text: they end up in lockfiles, cache keys, and JSON
    /// output. A non-UTF-8 path cannot round-trip through those, and lossy
    /// conversion would map two distinct files onto one cache key.
    #[error("path is not valid UTF-8: {0}")]
    NonUtf8(PathBuf),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedPath {
    inner: String,
}

impl NormalizedPath {
    /// Normalizes any path-like string.
    ///
    /// Both `/` and `\` are treated as separators on every platform. The cost is
    /// that a Unix file with a literal backslash in its name is not addressable
    /// through this type; the benefit is that a Windows-shaped path in a
    /// `package.json` or a CLI argument resolves the same everywhere. The JS
    /// ecosystem makes the same trade.
    pub fn new(input: impl AsRef<str>) -> Self {
        Self {
            inner: normalize(input.as_ref()),
        }
    }

    /// Converts an OS path, rejecting non-UTF-8 input
    pub fn from_native(path: &Path) -> Result<Self, PathError> {
        match path.to_str() {
            Some(text) => Ok(Self::new(text)),
            None => Err(PathError::NonUtf8(path.to_path_buf())),
        }
    }

    /// Borrows the normalized form as an OS path
    ///
    /// On the v1 targets the normalized form *is* the native form. Native
    /// Windows (v2) is where this stops being free
    pub fn as_path(&self) -> &Path {
        Path::new(&self.inner)
    }
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    pub fn into_string(self) -> String {
        self.inner
    }

    pub fn is_absolute(&self) -> bool {
        !root_prefix(&self.inner).is_empty()
    }

    /// Resolves `other` against this path, or replaces it if `other` is absolute
    pub fn join(&self, other: impl AsRef<str>) -> Self {
        let other = other.as_ref();
        if !root_prefix(other).is_empty() {
            return Self::new(other);
        }
        Self::new(format!("{}/{}", self.inner, other))
    }

    /// Appends to the final segment without inserting a separator `x` + `.js`
    pub fn with_suffix(&self, suffix: &str) -> Self {
        Self::new(format!("{}{}", self.inner, suffix))
    }

    /// The containing directory of `None` at a filesystem root
    pub fn parent(&self) -> Option<Self> {
        let parent = self.join("..");
        (parent != *self).then_some(parent)
    }

    /// The final segment, or `None` for roots, `.`, and `..`
    pub fn file_name(&self) -> Option<&str> {
        let root = root_prefix(&self.inner);
        let rest = &self.inner[root.len()..];
        match rest.rsplit('/').next() {
            None | Some("") | Some(".") | Some("..") => None,
            Some(name) => Some(name),
        }
    }

    /// The extension without its dot: `a/b.min.js` is `js`, `.bashrc` is `None`
    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name()?;
        match name.rfind('.') {
            Some(0) | None => None,
            Some(dot) => Some(&name[dot + 1..]),
        }
    }

    /// Path segments, excluding any root prefix
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        let root = root_prefix(&self.inner);
        let rest = &self.inner[root.len()..];
        rest.split('/').filter(|segment| !segment.is_empty())
    }

    /// Whether `base` is a path prefix of `self`, compared segment-wise so that
    /// `/a/bc` does not start with `/a/b`
    pub fn starts_with(&self, base: &Self) -> bool {
        if root_prefix(&self.inner) != root_prefix(&base.inner) {
            return false;
        }
        let mut base_segments = base.segments();
        let mut segments = self.segments();
        loop {
            match (base_segments.next(), segments.next()) {
                (None, _) => return true,
                (Some(_), None) => return false,
                (Some(expected), Some(actual)) if expected != actual => return false,
                (Some(_), Some(_)) => {}
            }
        }
    }

    /// Expresses this path relative to `base`, climbing with `..` when needed.
    ///
    /// Returns `None` when the two are anchored differently (one absolute, one
    /// relative, or different Windows drives), since no relative path connects
    /// them
    pub fn relative_to(&self, base: &Self) -> Option<Self> {
        if root_prefix(&self.inner) != root_prefix(&base.inner) {
            return None;
        }
        let mine: Vec<&str> = self.segments().collect();
        let theirs: Vec<&str> = base.segments().collect();
        let shared = mine
            .iter()
            .zip(theirs.iter())
            .take_while(|(a, b)| a == b)
            .count();

        // A relative base with leading `..` cannot be climbed out of lexically:
        // `../x` relative to `../../y` needs a real path to resolve
        if theirs[shared..].contains(&"..") {
            return None;
        }

        let mut parts: Vec<&str> = vec![".."; theirs.len() - shared];
        parts.extend_from_slice(&mine[shared..]);
        Some(Self::new(parts.join("/")))
    }

    /// Key for detecting collisions on case-insensitive filesystems.
    ///
    /// This is an approximation: APFS and NTFS fold case with Unicode tables
    /// that drift between versions. It is good enough to *flag* a collision,
    /// which is all Opal needs — it never uses this as an identity.
    pub fn case_fold_key(&self) -> String {
        self.inner.to_lowercase()
    }

    /// Whether two paths would collide on a case-insensitive filesystem.
    pub fn eq_case_insensitive(&self, other: &Self) -> bool {
        self.case_fold_key() == other.case_fold_key()
    }
}

/// Returns the root prefix of `input`: `""`, `"/"`, or a `"C:/"`-style drive.
fn root_prefix(input: &str) -> &str {
    let bytes = input.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        // Windows (v2) drive-absolute: `C:\x`. A bare `C:` is drive-relative,
        // which Opal does not model — treat it as absolute at the drive root.
        return match bytes.get(2) {
            Some(b'/' | b'\\') => &input[..3],
            _ => &input[..2],
        };
    }
    if matches!(bytes.first(), Some(b'/' | b'\\')) {
        return &input[..1];
    }
    ""
}

fn normalize(input: &str) -> String {
    let root = root_prefix(input);
    let is_absolute = !root.is_empty();
    let normalized_root = if is_absolute && root.len() > 1 {
        // `c:\` and `C:/` name the same root; case is normalized so the two
        // spellings share one cache key.
        format!("{}:/", root[..1].to_ascii_uppercase())
    } else {
        root.replace('\\', "/")
    };

    let mut segments: Vec<&str> = Vec::new();
    for segment in input[root.len()..].split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => match segments.last() {
                Some(&"..") => segments.push(".."),
                Some(_) => {
                    segments.pop();
                }
                // POSIX: `..` at the root is the root. A relative path keeps
                // climbing, because it has no known anchor to stop at.
                None if is_absolute => {}
                None => segments.push(".."),
            },
            other => segments.push(other),
        }
    }

    if segments.is_empty() {
        if is_absolute {
            normalized_root
        } else {
            ".".to_string()
        }
    } else {
        format!("{normalized_root}{}", segments.join("/"))
    }
}

impl fmt::Display for NormalizedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl fmt::Debug for NormalizedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.inner)
    }
}

impl From<&str> for NormalizedPath {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl Serialize for NormalizedPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.inner)
    }
}

impl<'de> Deserialize<'de> for NormalizedPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Re-normalize rather than trusting the encoded form: a hand-edited or
        // older-format cache file must not be able to inject `..` into a path
        // the rest of the engine assumes is normalized.
        let text = String::deserialize(deserializer)?;
        if text.is_empty() {
            return Err(D::Error::custom("empty path"));
        }
        Ok(Self::new(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalizes_redundant_syntax() {
        assert_eq!(NormalizedPath::new("a//b/./c/").as_str(), "a/b/c");
        assert_eq!(NormalizedPath::new("./a/b").as_str(), "a/b");
        assert_eq!(NormalizedPath::new("/a/b/../c").as_str(), "/a/c");
        assert_eq!(NormalizedPath::new("").as_str(), ".");
        assert_eq!(NormalizedPath::new(".").as_str(), ".");
        assert_eq!(NormalizedPath::new("/").as_str(), "/");
    }

    #[test]
    fn test_treats_backslash_as_separator() {
        assert_eq!(NormalizedPath::new(r"a\b/c").as_str(), "a/b/c");
        assert_eq!(NormalizedPath::new(r"\a\b").as_str(), "/a/b");
    }

    #[test]
    fn test_normalizes_windows_drive_roots() {
        assert_eq!(NormalizedPath::new(r"c:\a\b").as_str(), "C:/a/b");
        assert_eq!(
            NormalizedPath::new("C:/a/b"),
            NormalizedPath::new(r"c:\a\b")
        );
        assert!(NormalizedPath::new(r"c:\a").is_absolute());
    }

    #[test]
    fn test_dot_dot_cannot_escape_an_absolute_root() {
        assert_eq!(NormalizedPath::new("/../../a").as_str(), "/a");
        assert_eq!(NormalizedPath::new(r"C:\..\a").as_str(), "C:/a");
    }

    #[test]
    fn test_dot_dot_accumulates_in_a_relative_path() {
        assert_eq!(NormalizedPath::new("../../a").as_str(), "../../a");
        assert_eq!(NormalizedPath::new("a/../../b").as_str(), "../b");
    }

    #[test]
    fn test_join_replaces_on_absolute_argument() {
        let base = NormalizedPath::new("/project/src");
        assert_eq!(base.join("../lib/a.js").as_str(), "/project/lib/a.js");
        assert_eq!(base.join("/etc/hosts").as_str(), "/etc/hosts");
    }

    #[test]
    fn test_parent_stops_at_root() {
        assert_eq!(NormalizedPath::new("/a/b").parent().unwrap().as_str(), "/a");
        assert_eq!(NormalizedPath::new("a").parent().unwrap().as_str(), ".");
        assert!(NormalizedPath::new("/").parent().is_none());
        assert!(NormalizedPath::new("C:/").parent().is_none());
    }

    #[test]
    fn test_file_name_and_extension() {
        let path = NormalizedPath::new("/a/b/c.min.js");
        assert_eq!(path.file_name(), Some("c.min.js"));
        assert_eq!(path.extension(), Some("js"));
        assert_eq!(NormalizedPath::new("/a/.bashrc").extension(), None);
        assert_eq!(NormalizedPath::new("/a/b").extension(), None);
        assert_eq!(NormalizedPath::new("/").file_name(), None);
    }

    #[test]
    fn test_starts_with_compares_whole_segments() {
        let path = NormalizedPath::new("/a/bc/d");
        assert!(path.starts_with(&NormalizedPath::new("/a/bc")));
        assert!(!path.starts_with(&NormalizedPath::new("/a/b")));
        assert!(!path.starts_with(&NormalizedPath::new("a/bc")));
    }

    #[test]
    fn test_relative_to() {
        let base = NormalizedPath::new("/project");
        assert_eq!(
            NormalizedPath::new("/project/src/a.js")
                .relative_to(&base)
                .unwrap()
                .as_str(),
            "src/a.js"
        );
        assert_eq!(
            NormalizedPath::new("/other/a.js")
                .relative_to(&base)
                .unwrap()
                .as_str(),
            "../other/a.js"
        );
        assert_eq!(base.relative_to(&base).unwrap().as_str(), ".");
        assert!(NormalizedPath::new("a.js").relative_to(&base).is_none());
    }

    #[test]
    fn test_case_collision_is_detectable_but_not_identity() {
        let lower = NormalizedPath::new("/a/react.js");
        let upper = NormalizedPath::new("/a/React.js");
        assert_ne!(lower, upper);
        assert!(lower.eq_case_insensitive(&upper));
    }

    #[test]
    fn test_deserialize_renormalizes() {
        let path: NormalizedPath = serde_json::from_str(r#""/a/b/../c""#).unwrap();
        assert_eq!(path.as_str(), "/a/c");
        assert!(serde_json::from_str::<NormalizedPath>(r#""""#).is_err());
    }
}
