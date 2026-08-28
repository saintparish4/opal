//! Property tests for npm-flavored semver.
//!
//! `testing_strategy.md` §1 names range solving the highest-value `proptest`
//! target in the whole project, for a specific reason: this is the one
//! component whose bugs do not crash. A comparator that is wrong at one
//! boundary installs a different version than the manifest asked for, the tree
//! builds, the tests pass, and nobody finds out until the wrong version's
//! behaviour surfaces somewhere unrelated.
//!
//! Most of what follows needs no oracle — `max_satisfying` returning the
//! greatest match, a union matching exactly what its branches match, and
//! reparsing being a fixpoint are all checkable against the implementation's
//! own output. Where an oracle *is* needed, for `^`, `~`, and hyphen ranges,
//! it is written as a plain interval over release versions, which is a genuinely
//! different formulation from the comparator sets `semver.rs` builds, rather
//! than the same code twice.

use std::cmp::Ordering;

use opal_pm::semver::{Range, Version};
use proptest::prelude::*;

fn version() -> impl Strategy<Value = Version> {
    // Deliberately a small space: boundary bugs live at 0, at the version next
    // to the one named, and at the release/prerelease line, none of which need
    // large numbers to reach.
    (
        0u64..3,
        0u64..4,
        0u64..4,
        prop::option::weighted(0.3, tag()),
    )
        .prop_map(|(major, minor, patch, tag)| match tag {
            Some(tag) => parse(&format!("{major}.{minor}.{patch}-{tag}")),
            None => parse(&format!("{major}.{minor}.{patch}")),
        })
}

fn release() -> impl Strategy<Value = Version> {
    (0u64..3, 0u64..4, 0u64..4).prop_map(|(major, minor, patch)| Version::new(major, minor, patch))
}

fn prerelease() -> impl Strategy<Value = Version> {
    (0u64..3, 0u64..4, 0u64..4, tag())
        .prop_map(|(major, minor, patch, tag)| parse(&format!("{major}.{minor}.{patch}-{tag}")))
}

fn tag() -> impl Strategy<Value = String> {
    let identifier = prop_oneof![
        (0u64..5).prop_map(|number| number.to_string()),
        prop::sample::select(vec!["alpha", "beta", "rc", "x-1"]).prop_map(str::to_string),
    ];
    prop::collection::vec(identifier, 1..=2).prop_map(|parts| parts.join("."))
}

/// Version text as manifests actually write it, partials and wildcards included.
fn version_text() -> impl Strategy<Value = String> {
    prop_oneof![
        version().prop_map(|version| version.to_string()),
        (0u64..3).prop_map(|major| major.to_string()),
        (0u64..3, 0u64..4).prop_map(|(major, minor)| format!("{major}.{minor}")),
        (0u64..3).prop_map(|major| format!("{major}.x")),
        Just("*".to_string()),
    ]
}

fn comparator_text() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!["", "=", "^", "~", ">", ">=", "<", "<="]),
        version_text(),
    )
        .prop_map(|(operator, version)| format!("{operator}{version}"))
}

fn range_text() -> impl Strategy<Value = String> {
    prop_oneof![
        comparator_text(),
        (version_text(), version_text()).prop_map(|(low, high)| format!("{low} - {high}")),
        (comparator_text(), comparator_text()).prop_map(|(low, high)| format!("{low} {high}")),
        (comparator_text(), comparator_text())
            .prop_map(|(left, right)| format!("{left} || {right}")),
    ]
}

fn parse(text: &str) -> Version {
    Version::parse(text).expect("the generators only produce well-formed versions")
}

fn range(text: &str) -> Range {
    Range::parse(text).expect("the generators only produce well-formed ranges")
}

/// `^X.Y.Z` allows anything below the next change to the left-most non-zero
/// part. Stated as an interval, independently of how `semver.rs` gets there.
fn caret_upper_bound(version: &Version) -> Version {
    if version.major > 0 {
        Version::new(version.major + 1, 0, 0)
    } else if version.minor > 0 {
        Version::new(0, version.minor + 1, 0)
    } else {
        Version::new(0, 0, version.patch + 1)
    }
}

/// `~X.Y.Z` allows patch-level changes only.
fn tilde_upper_bound(version: &Version) -> Version {
    Version::new(version.major, version.minor + 1, 0)
}

proptest! {
    /// Guards every other test in this file: they all `expect` a parse.
    #[test]
    fn test_the_generators_only_produce_parseable_input(text in range_text()) {
        prop_assert!(Range::parse(&text).is_ok(), "{text:?} should parse");
    }

    #[test]
    fn test_a_version_round_trips_through_its_own_rendering(version in version()) {
        prop_assert_eq!(Version::parse(&version.to_string()).ok(), Some(version));
    }

    #[test]
    fn test_version_ordering_is_a_total_order(
        left in version(),
        middle in version(),
        right in version(),
    ) {
        prop_assert_eq!(left.cmp(&middle), middle.cmp(&left).reverse());
        prop_assert_eq!(left.cmp(&left), Ordering::Equal);
        if left <= middle && middle <= right {
            prop_assert!(left <= right);
        }
    }

    /// Build metadata is not part of precedence (semver §10), so it must not
    /// reach ordering or matching. A version that sorts differently with a
    /// `+build` suffix is one that resolves differently too.
    #[test]
    fn test_build_metadata_changes_neither_ordering_nor_matching(
        version in version(),
        build in "[a-z0-9]{1,4}",
        text in range_text(),
    ) {
        let tagged = parse(&format!("{version}+{build}"));
        prop_assert_eq!(version.cmp(&tagged), Ordering::Equal);
        prop_assert_eq!(range(&text).satisfies(&version), range(&text).satisfies(&tagged));
    }

    /// The property the lockfile rests on: what gets installed is the greatest
    /// version the range allows, and `None` means nothing was allowed.
    #[test]
    fn test_max_satisfying_returns_the_greatest_match(
        text in range_text(),
        versions in prop::collection::vec(version(), 1..12),
    ) {
        let range = range(&text);
        let matching: Vec<&Version> = versions.iter().filter(|v| range.satisfies(v)).collect();

        match range.max_satisfying(&versions) {
            Some(best) => {
                prop_assert!(range.satisfies(best));
                prop_assert!(matching.iter().all(|version| *version <= best));
            }
            None => prop_assert!(matching.is_empty()),
        }
    }

    #[test]
    fn test_a_union_matches_exactly_what_its_branches_match(
        left in range_text(),
        right in range_text(),
        candidate in version(),
    ) {
        let union = range(&format!("{left} || {right}"));
        prop_assert_eq!(
            union.satisfies(&candidate),
            range(&left).satisfies(&candidate) || range(&right).satisfies(&candidate)
        );
    }

    #[test]
    fn test_caret_matches_exactly_the_interval_it_names(
        base in release(),
        candidate in release(),
    ) {
        prop_assert_eq!(
            range(&format!("^{base}")).satisfies(&candidate),
            candidate >= base && candidate < caret_upper_bound(&base)
        );
    }

    #[test]
    fn test_tilde_matches_exactly_the_interval_it_names(
        base in release(),
        candidate in release(),
    ) {
        prop_assert_eq!(
            range(&format!("~{base}")).satisfies(&candidate),
            candidate >= base && candidate < tilde_upper_bound(&base)
        );
    }

    #[test]
    fn test_a_hyphen_range_is_inclusive_at_both_ends(
        low in release(),
        high in release(),
        candidate in release(),
    ) {
        prop_assert_eq!(
            range(&format!("{low} - {high}")).satisfies(&candidate),
            candidate >= low && candidate <= high
        );
    }

    /// The rule that stops every range in every manifest from silently picking
    /// up alphas: a prerelease needs an explicit invitation at its own
    /// `major.minor.patch`.
    #[test]
    fn test_a_range_naming_no_prerelease_never_matches_one(
        base in release(),
        operator in prop::sample::select(vec!["", "=", "^", "~", ">", ">=", "<", "<="]),
        candidate in prerelease(),
    ) {
        let range = range(&format!("{operator}{base}"));
        prop_assert!(!range.satisfies(&candidate));
    }

    #[test]
    fn test_reparsing_a_range_matches_the_same_versions(
        text in range_text(),
        versions in prop::collection::vec(version(), 1..8),
    ) {
        let once = range(&text);
        let twice = range(once.as_str());
        for version in &versions {
            prop_assert_eq!(once.satisfies(version), twice.satisfies(version));
        }
    }

    /// `package.json` is untrusted input (`testing_strategy.md` §6), and a
    /// panic on install is the worst failure mode a package manager has. This
    /// is not fuzzing, but it is the same boundary.
    #[test]
    fn test_arbitrary_text_never_panics_a_parser(text in "\\PC{0,24}") {
        if let Ok(version) = Version::parse(&text) {
            prop_assert_eq!(Version::parse(&version.to_string()).ok(), Some(version));
        }
        if let Ok(range) = Range::parse(&text) {
            prop_assert!(Range::parse(range.as_str()).is_ok());
        }
    }
}
