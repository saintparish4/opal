//! npm-flavored semantic versioning.
//!
//! Written rather than taken from a crate for one reason: the widely used
//! `semver` crate implements *Cargo's* dialect, which rejects `||`, rejects
//! hyphen ranges, and treats `*` and prereleases differently from npm. Feeding
//! npm ranges to it does not fail loudly — it resolves a different version,
//! which is precisely the silent wrong-version install `testing_strategy.md` §1
//! calls worse than a crash. (`node-semver` on crates.io is an alternative; this
//! is ~400 lines we can proptest and fuzz on our own terms.)
//!
//! The rules encoded here that people get wrong:
//!
//! - Build metadata is ignored when ordering; prereleases order *below* their
//!   release, and an absent prerelease is greater than any present one.
//! - A prerelease version satisfies a comparator set only if some comparator in
//!   that same set names a prerelease at the identical `major.minor.patch`.
//!   `>=1.0.0` therefore does not match `2.0.0-beta.1`, which is what stops
//!   every range from silently picking up alphas.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum SemverError {
    #[error("{0:?} is not a valid version")]
    Version(String),
    #[error("{0:?} is not a valid version range")]
    Range(String),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Identifier {
    Numeric(u64),
    Alphanumeric(String),
}

impl Ord for Identifier {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            (Self::Alphanumeric(left), Self::Alphanumeric(right)) => left.cmp(right),
            // Numeric identifiers always have lower precedence (semver §11.4.3).
            (Self::Numeric(_), Self::Alphanumeric(_)) => Ordering::Less,
            (Self::Alphanumeric(_), Self::Numeric(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for Identifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Numeric(value) => write!(f, "{value}"),
            Self::Alphanumeric(value) => f.write_str(value),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Vec<Identifier>,
    pub build: Option<String>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: Vec::new(),
            build: None,
        }
    }

    pub fn parse(text: &str) -> Result<Self, SemverError> {
        let raw = text.trim();
        let trimmed = raw.trim_start_matches(['v', '=']).trim();
        let invalid = || SemverError::Version(text.to_string());

        let (core, build) = match trimmed.split_once('+') {
            Some((core, build)) if !build.is_empty() => (core, Some(build.to_string())),
            Some(_) => return Err(invalid()),
            None => (trimmed, None),
        };
        let (numbers, prerelease) = match core.split_once('-') {
            Some((numbers, pre)) => (numbers, parse_identifiers(pre).ok_or_else(invalid)?),
            None => (core, Vec::new()),
        };

        let mut parts = numbers.split('.');
        let mut number = || {
            parts
                .next()
                .and_then(parse_number)
                .ok_or_else(|| SemverError::Version(text.to_string()))
        };
        let version = Self {
            major: number()?,
            minor: number()?,
            patch: number()?,
            prerelease,
            build,
        };
        if parts.next().is_some() {
            return Err(invalid());
        }
        Ok(version)
    }

    pub fn is_prerelease(&self) -> bool {
        !self.prerelease.is_empty()
    }

    fn core(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // Build metadata is explicitly not part of precedence (semver §10).
        self.core().cmp(&other.core()).then_with(|| {
            match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, true) => Ordering::Equal,
                // A release outranks any of its prereleases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.prerelease.cmp(&other.prerelease),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.prerelease.is_empty() {
            write!(f, "-")?;
            for (index, identifier) in self.prerelease.iter().enumerate() {
                if index > 0 {
                    write!(f, ".")?;
                }
                write!(f, "{identifier}")?;
            }
        }
        match &self.build {
            Some(build) => write!(f, "+{build}"),
            None => Ok(()),
        }
    }
}

impl FromStr for Version {
    type Err = SemverError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl Serialize for Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Comparator {
    pub op: Op,
    pub version: Version,
}

impl Comparator {
    fn matches(&self, version: &Version) -> bool {
        let ordering = version.cmp(&self.version);
        match self.op {
            Op::Gt => ordering == Ordering::Greater,
            Op::Gte => ordering != Ordering::Less,
            Op::Lt => ordering == Ordering::Less,
            Op::Lte => ordering != Ordering::Greater,
            Op::Eq => ordering == Ordering::Equal,
        }
    }
}

impl fmt::Display for Comparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = match self.op {
            Op::Gt => ">",
            Op::Gte => ">=",
            Op::Lt => "<",
            Op::Lte => "<=",
            Op::Eq => "=",
        };
        write!(f, "{op}{}", self.version)
    }
}

/// A union of comparator sets: `^1 || >=2.3 <3` is two sets.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Range {
    sets: Vec<Vec<Comparator>>,
    raw: String,
}

impl Range {
    pub fn parse(text: &str) -> Result<Self, SemverError> {
        let raw = text.trim();
        let mut sets = Vec::new();
        for part in raw.split("||") {
            sets.push(
                parse_comparator_set(part).ok_or_else(|| SemverError::Range(text.to_string()))?,
            );
        }
        Ok(Self {
            sets,
            raw: if raw.is_empty() {
                "*".to_string()
            } else {
                raw.to_string()
            },
        })
    }

    /// Matches everything except prereleases — the `*` range.
    pub fn any() -> Self {
        Self {
            sets: vec![Vec::new()],
            raw: "*".to_string(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn satisfies(&self, version: &Version) -> bool {
        self.sets.iter().any(|set| set_matches(set, version))
    }

    /// The highest version satisfying this range.
    pub fn max_satisfying<'a>(
        &self,
        versions: impl IntoIterator<Item = &'a Version>,
    ) -> Option<&'a Version> {
        versions
            .into_iter()
            .filter(|version| self.satisfies(version))
            .max()
    }
}

fn set_matches(set: &[Comparator], version: &Version) -> bool {
    if !set.iter().all(|comparator| comparator.matches(version)) {
        return false;
    }
    if !version.is_prerelease() {
        return true;
    }
    // npm's prerelease rule: an alpha is only ever picked up by a range that
    // asked for an alpha of that exact release. Note this also makes the empty
    // set (`*`) reject prereleases, which is the behaviour people expect.
    set.iter().any(|comparator| {
        comparator.version.is_prerelease() && comparator.version.core() == version.core()
    })
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for Range {
    type Err = SemverError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl Serialize for Range {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for Range {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

/// A version with optional trailing parts: the `1.2.x` in `^1.2.x`.
#[derive(Clone, Debug, Default)]
struct Partial {
    major: Option<u64>,
    minor: Option<u64>,
    patch: Option<u64>,
    prerelease: Vec<Identifier>,
    build: Option<String>,
}

impl Partial {
    fn at(&self, major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
            prerelease: Vec::new(),
            build: None,
        }
    }

    /// The exact version this partial names, when it names one.
    fn exact(&self) -> Option<Version> {
        Some(Version {
            major: self.major?,
            minor: self.minor?,
            patch: self.patch?,
            prerelease: self.prerelease.clone(),
            build: self.build.clone(),
        })
    }

    fn lower_bound(&self) -> Version {
        Version {
            major: self.major.unwrap_or(0),
            minor: self.minor.unwrap_or(0),
            patch: self.patch.unwrap_or(0),
            prerelease: self.prerelease.clone(),
            build: None,
        }
    }
}

fn parse_comparator_set(text: &str) -> Option<Vec<Comparator>> {
    let tokens = tokenize(text)?;
    let mut comparators = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        // A hyphen range is three tokens: `1.2.3 - 2.3.4`.
        if index + 2 < tokens.len() && tokens[index + 1] == "-" {
            let low = parse_partial(&tokens[index])?;
            let high = parse_partial(&tokens[index + 2])?;
            comparators.extend(hyphen(&low, &high));
            index += 3;
            continue;
        }
        comparators.extend(parse_simple(&tokens[index])?);
        index += 1;
    }
    Some(comparators)
}

/// Splits on whitespace, then re-joins operators that were written detached
/// (`>= 1.2.3`), which npm tolerates and real `package.json` files contain.
fn tokenize(text: &str) -> Option<Vec<String>> {
    let mut tokens: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for word in text.split_whitespace() {
        match pending.take() {
            Some(operator) => tokens.push(format!("{operator}{word}")),
            None if matches!(word, ">" | ">=" | "<" | "<=" | "=" | "^" | "~") => {
                pending = Some(word.to_string());
            }
            None => tokens.push(word.to_string()),
        }
    }
    // A trailing bare operator is not a range.
    pending.is_none().then_some(tokens)
}

fn parse_simple(token: &str) -> Option<Vec<Comparator>> {
    let (operator, rest) = split_operator(token);
    let partial = parse_partial(rest)?;
    Some(match operator {
        "^" => caret(&partial),
        "~" | "~>" => tilde(&partial),
        ">" => primitive(Op::Gt, &partial),
        ">=" => primitive(Op::Gte, &partial),
        "<" => primitive(Op::Lt, &partial),
        "<=" => primitive(Op::Lte, &partial),
        _ => primitive(Op::Eq, &partial),
    })
}

fn split_operator(token: &str) -> (&str, &str) {
    for operator in [">=", "<=", "~>", ">", "<", "=", "^", "~"] {
        if let Some(rest) = token.strip_prefix(operator) {
            return (operator, rest);
        }
    }
    ("", token)
}

fn parse_partial(text: &str) -> Option<Partial> {
    let text = text.trim().trim_start_matches('v');
    if text.is_empty() || is_wildcard(text) {
        return Some(Partial::default());
    }

    let (core, build) = match text.split_once('+') {
        Some((core, build)) if !build.is_empty() => (core, Some(build.to_string())),
        Some(_) => return None,
        None => (text, None),
    };
    let (numbers, prerelease) = match core.split_once('-') {
        Some((numbers, pre)) => (numbers, parse_identifiers(pre)?),
        None => (core, Vec::new()),
    };

    let mut slots: [Option<u64>; 3] = [None; 3];
    for (index, part) in numbers.split('.').enumerate() {
        if index == slots.len() {
            return None;
        }
        // A wildcard ends the specified prefix: `1.x` and `1.x.x` are the same
        // range, and everything after the first wildcard adds nothing.
        if is_wildcard(part) {
            break;
        }
        slots[index] = Some(parse_number(part)?);
    }

    Some(Partial {
        major: slots[0],
        minor: slots[1],
        patch: slots[2],
        prerelease,
        build,
    })
}

fn is_wildcard(text: &str) -> bool {
    matches!(text, "*" | "x" | "X" | "")
}

/// `^1.2.3` allows changes that do not modify the left-most non-zero part.
fn caret(partial: &Partial) -> Vec<Comparator> {
    let Some(major) = partial.major else {
        return Vec::new();
    };
    let lower = partial.lower_bound();
    let upper = match (partial.minor, partial.patch) {
        (None, _) => partial.at(major + 1, 0, 0),
        (Some(_), None) if major > 0 => partial.at(major + 1, 0, 0),
        (Some(minor), None) => partial.at(0, minor + 1, 0),
        (Some(minor), Some(patch)) => {
            if major > 0 {
                partial.at(major + 1, 0, 0)
            } else if minor > 0 {
                partial.at(0, minor + 1, 0)
            } else {
                partial.at(0, 0, patch + 1)
            }
        }
    };
    bounded(lower, upper)
}

/// `~1.2.3` allows patch-level changes if a minor version is specified.
fn tilde(partial: &Partial) -> Vec<Comparator> {
    let Some(major) = partial.major else {
        return Vec::new();
    };
    let lower = partial.lower_bound();
    let upper = match partial.minor {
        None => partial.at(major + 1, 0, 0),
        Some(minor) => partial.at(major, minor + 1, 0),
    };
    bounded(lower, upper)
}

fn primitive(op: Op, partial: &Partial) -> Vec<Comparator> {
    if let Some(version) = partial.exact() {
        return vec![Comparator { op, version }];
    }
    let Some(major) = partial.major else {
        // `>=*` and `<=*` match anything; `>*` and `<*` match nothing.
        return match op {
            Op::Gte | Op::Lte | Op::Eq => Vec::new(),
            Op::Gt | Op::Lt => vec![Comparator {
                op: Op::Lt,
                version: Version::new(0, 0, 0),
            }],
        };
    };
    let minor = partial.minor;
    match op {
        // `1.2` as an equality is the whole 1.2.x line.
        Op::Eq => match minor {
            None => bounded(partial.at(major, 0, 0), partial.at(major + 1, 0, 0)),
            Some(minor) => bounded(partial.at(major, minor, 0), partial.at(major, minor + 1, 0)),
        },
        Op::Gt => vec![Comparator {
            op: Op::Gte,
            version: match minor {
                None => partial.at(major + 1, 0, 0),
                Some(minor) => partial.at(major, minor + 1, 0),
            },
        }],
        Op::Gte => vec![Comparator {
            op: Op::Gte,
            version: partial.at(major, minor.unwrap_or(0), 0),
        }],
        Op::Lt => vec![Comparator {
            op: Op::Lt,
            version: partial.at(major, minor.unwrap_or(0), 0),
        }],
        Op::Lte => vec![Comparator {
            op: Op::Lt,
            version: match minor {
                None => partial.at(major + 1, 0, 0),
                Some(minor) => partial.at(major, minor + 1, 0),
            },
        }],
    }
}

fn hyphen(low: &Partial, high: &Partial) -> Vec<Comparator> {
    let mut comparators = Vec::new();
    if low.major.is_some() {
        comparators.push(Comparator {
            op: Op::Gte,
            version: low.lower_bound(),
        });
    }
    match (high.exact(), high.major, high.minor) {
        (Some(version), _, _) => comparators.push(Comparator {
            op: Op::Lte,
            version,
        }),
        // A partial upper bound is inclusive of the whole line it names.
        (None, Some(major), Some(minor)) => comparators.push(Comparator {
            op: Op::Lt,
            version: high.at(major, minor + 1, 0),
        }),
        (None, Some(major), None) => comparators.push(Comparator {
            op: Op::Lt,
            version: high.at(major + 1, 0, 0),
        }),
        (None, None, _) => {}
    }
    comparators
}

fn bounded(lower: Version, upper: Version) -> Vec<Comparator> {
    vec![
        Comparator {
            op: Op::Gte,
            version: lower,
        },
        Comparator {
            op: Op::Lt,
            version: upper,
        },
    ]
}

fn parse_number(text: &str) -> Option<u64> {
    // Leading zeros are invalid in semver, and accepting them would make
    // `01.2.3` and `1.2.3` two spellings of one version.
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    text.parse().ok()
}

fn parse_identifiers(text: &str) -> Option<Vec<Identifier>> {
    if text.is_empty() {
        return None;
    }
    text.split('.')
        .map(|part| {
            if part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return None;
            }
            if part.bytes().all(|byte| byte.is_ascii_digit()) {
                parse_number(part).map(Identifier::Numeric)
            } else {
                Some(Identifier::Alphanumeric(part.to_string()))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).expect(text)
    }

    fn range(text: &str) -> Range {
        Range::parse(text).expect(text)
    }

    fn matches(range_text: &str, version_text: &str) -> bool {
        range(range_text).satisfies(&version(version_text))
    }

    #[test]
    fn test_parses_and_renders_versions() {
        assert_eq!(version("1.2.3").to_string(), "1.2.3");
        assert_eq!(version("v1.2.3").to_string(), "1.2.3");
        assert_eq!(version("1.2.3-alpha.1").to_string(), "1.2.3-alpha.1");
        assert_eq!(
            version("1.2.3-0.beta+build.5").to_string(),
            "1.2.3-0.beta+build.5"
        );
    }

    #[test]
    fn test_rejects_malformed_versions() {
        for text in ["1.2", "1.2.3.4", "01.2.3", "1.2.3-", "1.2.3+", "a.b.c", ""] {
            assert!(Version::parse(text).is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn test_orders_prereleases_below_their_release() {
        assert!(version("1.0.0-alpha") < version("1.0.0"));
        assert!(version("1.0.0-alpha") < version("1.0.0-alpha.1"));
        assert!(version("1.0.0-alpha.1") < version("1.0.0-alpha.beta"));
        assert!(version("1.0.0-beta.2") < version("1.0.0-beta.11"));
        assert!(version("1.0.0-rc.1") < version("1.0.0"));
        assert!(version("1.0.0") < version("1.0.1"));
    }

    #[test]
    fn test_build_metadata_is_ignored_when_ordering() {
        assert_eq!(version("1.0.0+a").cmp(&version("1.0.0+b")), Ordering::Equal);
    }

    #[test]
    fn test_caret_ranges() {
        assert!(matches("^1.2.3", "1.2.3"));
        assert!(matches("^1.2.3", "1.9.9"));
        assert!(!matches("^1.2.3", "1.2.2"));
        assert!(!matches("^1.2.3", "2.0.0"));
        // Below 1.0.0 the left-most non-zero part is the one that is locked.
        assert!(matches("^0.2.3", "0.2.9"));
        assert!(!matches("^0.2.3", "0.3.0"));
        assert!(matches("^0.0.3", "0.0.3"));
        assert!(!matches("^0.0.3", "0.0.4"));
        assert!(matches("^1.2.x", "1.5.0"));
        assert!(matches("^0.0.x", "0.0.9"));
        assert!(!matches("^0.0.x", "0.1.0"));
    }

    #[test]
    fn test_tilde_ranges() {
        assert!(matches("~1.2.3", "1.2.9"));
        assert!(!matches("~1.2.3", "1.3.0"));
        assert!(matches("~1.2", "1.2.9"));
        assert!(!matches("~1.2", "1.3.0"));
        assert!(matches("~1", "1.9.9"));
        assert!(!matches("~1", "2.0.0"));
    }

    #[test]
    fn test_wildcards_and_partials() {
        assert!(matches("*", "9.9.9"));
        assert!(matches("", "9.9.9"));
        assert!(matches("1.x", "1.9.9"));
        assert!(!matches("1.x", "2.0.0"));
        assert!(matches("1.2", "1.2.9"));
        assert!(!matches("1.2", "1.3.0"));
    }

    #[test]
    fn test_comparator_sets_and_unions() {
        assert!(matches(">=1.2.0 <2.0.0", "1.9.0"));
        assert!(!matches(">=1.2.0 <2.0.0", "2.0.0"));
        assert!(matches(">= 1.2.0 < 2.0.0", "1.9.0"));
        assert!(matches("^1 || ^3", "3.1.0"));
        assert!(!matches("^1 || ^3", "2.0.0"));
        assert!(matches(">1.2", "1.3.0"));
        assert!(!matches(">1.2", "1.2.9"));
        assert!(matches("<=1.2", "1.2.9"));
        assert!(!matches("<=1.2", "1.3.0"));
    }

    #[test]
    fn test_hyphen_ranges() {
        assert!(matches("1.2.3 - 2.3.4", "2.3.4"));
        assert!(!matches("1.2.3 - 2.3.4", "2.3.5"));
        assert!(matches("1.2 - 2.3", "2.3.9"));
        assert!(!matches("1.2 - 2.3", "2.4.0"));
        assert!(matches("1.2.3 - 2", "2.9.9"));
        assert!(!matches("1.2.3 - 2", "3.0.0"));
    }

    #[test]
    fn test_prereleases_need_an_explicit_invitation() {
        // The rule that keeps every range from picking up alphas.
        assert!(!matches(">=1.0.0", "2.0.0-beta.1"));
        assert!(!matches("*", "1.0.0-beta.1"));
        assert!(!matches("^1.0.0", "1.5.0-beta.1"));
        assert!(matches(">=1.0.0-beta.1", "1.0.0-beta.2"));
        assert!(matches("^1.0.0-beta.1", "1.0.0-rc.1"));
        assert!(matches("1.0.0-beta.1", "1.0.0-beta.1"));
    }

    #[test]
    fn test_max_satisfying_picks_the_highest_match() {
        let versions: Vec<Version> = ["1.0.0", "1.2.0", "1.9.3", "2.0.0", "2.1.0-rc.1"]
            .iter()
            .map(|text| version(text))
            .collect();
        assert_eq!(
            range("^1.0.0")
                .max_satisfying(&versions)
                .unwrap()
                .to_string(),
            "1.9.3"
        );
        assert_eq!(
            range("*").max_satisfying(&versions).unwrap().to_string(),
            "2.0.0"
        );
        assert!(range("^3.0.0").max_satisfying(&versions).is_none());
    }

    #[test]
    fn test_rejects_malformed_ranges() {
        for text in ["^", ">=", "1.2.3 -", "not-a-range", "1.2.3.4"] {
            assert!(Range::parse(text).is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn test_round_trips_through_serde() {
        let encoded = serde_json::to_string(&version("1.2.3-rc.1")).unwrap();
        assert_eq!(encoded, "\"1.2.3-rc.1\"");
        assert_eq!(
            serde_json::from_str::<Version>(&encoded).unwrap(),
            version("1.2.3-rc.1")
        );

        let encoded = serde_json::to_string(&range("^1.2.3")).unwrap();
        assert_eq!(
            serde_json::from_str::<Range>(&encoded).unwrap().as_str(),
            "^1.2.3"
        );
    }
}
