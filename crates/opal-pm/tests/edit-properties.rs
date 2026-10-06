//! Property tests for `package.json` edits, over generated manifests.
//!
//! The unit tests in `edit.rs` pin each rule on a manifest written for it.
//! These hold the promise the module makes about every manifest: an edit
//! changes the group it was asked to change, and every other member comes
//! back as the bytes it went in with.
//!
//! Member values are drawn from text written the awkward ways real manifests
//! are (inline arrays, escapes, exponents, nested objects on several lines),
//! because a value that survives only when it is already tidy is the bug.

use std::collections::BTreeMap;

use opal_pm::edit::{self, Addition, Group};
use proptest::prelude::*;
use serde_json::value::RawValue;

/// Values as source text, none of them in the form a serializer would write.
const VALUES: [&str; 8] = [
    "\"plain\"",
    "\"caf\\u00e9 \\/ \\u2014\"",
    "[\"a.js\", \"b.js\"]",
    "[\n    1e3,\n    1.0\n  ]",
    "{\"node\": \">=18\"}",
    "{\n    \"build\": \"tsc\",\n    \"test\": \"vitest\"\n  }",
    "true",
    "null",
];

const KEYS: [&str; 6] = ["name", "version", "files", "scripts", "engines", "private"];

#[derive(Clone, Debug)]
struct Layout {
    indent: &'static str,
    newline: &'static str,
    trailing_newline: bool,
}

#[derive(Clone, Debug)]
struct Plan {
    /// Other members, as (key, value text). Keys repeat on purpose: a
    /// duplicate outside the dependency groups is carried, not refused.
    before: Vec<(usize, usize)>,
    after: Vec<(usize, usize)>,
    dependencies: BTreeMap<String, String>,
    dev_dependencies: BTreeMap<String, String>,
    layout: Layout,
}

fn group_text(entries: &BTreeMap<String, String>, layout: &Layout) -> String {
    let Layout {
        indent, newline, ..
    } = layout;
    let lines: Vec<String> = entries
        .iter()
        .map(|(name, spec)| format!("{indent}{indent}\"{name}\": \"{spec}\""))
        .collect();
    format!(
        "{{{newline}{}{newline}{indent}}}",
        lines.join(&format!(",{newline}"))
    )
}

/// The manifest a plan describes, in the layout npm writes: one member per
/// line, groups sorted.
fn manifest(plan: &Plan) -> String {
    let Layout {
        indent,
        newline,
        trailing_newline,
    } = &plan.layout;
    let member =
        |(key, value): &(usize, usize)| (KEYS[*key].to_string(), VALUES[*value].to_string());

    let mut members: Vec<(String, String)> = plan.before.iter().map(member).collect();
    for (field, entries) in [
        ("dependencies", &plan.dependencies),
        ("devDependencies", &plan.dev_dependencies),
    ] {
        if !entries.is_empty() {
            members.push((field.to_string(), group_text(entries, &plan.layout)));
        }
    }
    members.extend(plan.after.iter().map(member));

    if members.is_empty() {
        return "{}".to_string();
    }
    let lines: Vec<String> = members
        .iter()
        .map(|(key, value)| format!("{indent}\"{key}\": {value}"))
        .collect();
    format!(
        "{{{newline}{}{newline}}}{}",
        lines.join(&format!(",{newline}")),
        if *trailing_newline { *newline } else { "" }
    )
}

fn package_name() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z][a-z0-9._-]{0,8}",
        "@[a-z][a-z0-9-]{0,5}/[a-z][a-z0-9._-]{0,6}",
    ]
}

fn spec() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("^1.2.3".to_string()),
        Just("1.0.0".to_string()),
        Just(">=2 <3".to_string()),
        Just("npm:other@^4.0.0".to_string()),
    ]
}

fn group() -> impl Strategy<Value = BTreeMap<String, String>> {
    prop::collection::btree_map(package_name(), spec(), 0..5)
}

fn members() -> impl Strategy<Value = Vec<(usize, usize)>> {
    prop::collection::vec((0..KEYS.len(), 0..VALUES.len()), 0..4)
}

fn layout() -> impl Strategy<Value = Layout> {
    (
        prop_oneof![Just("  "), Just("    "), Just("\t")],
        prop_oneof![Just("\n"), Just("\r\n")],
        any::<bool>(),
    )
        .prop_map(|(indent, newline, trailing_newline)| Layout {
            indent,
            newline,
            trailing_newline,
        })
}

fn plan() -> impl Strategy<Value = Plan> {
    (members(), members(), group(), group(), layout()).prop_map(
        |(before, after, dependencies, dev_dependencies, layout)| Plan {
            before,
            after,
            dependencies,
            dev_dependencies,
            layout,
        },
    )
}

fn target() -> impl Strategy<Value = Option<Group>> {
    prop_oneof![
        Just(None),
        Just(Some(Group::Runtime)),
        Just(Some(Group::Development)),
        Just(Some(Group::Optional)),
    ]
}

/// Every member outside the dependency groups, as source text, in order.
fn untouched(text: &str) -> Vec<(String, String)> {
    let values: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(text).expect("the manifest parses");
    let order: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(text).expect("the manifest parses");
    order
        .keys()
        .filter(|key| !key.ends_with("ependencies"))
        .map(|key| (key.clone(), values[key].get().to_string()))
        .collect()
}

fn declared(text: &str, field: &str) -> BTreeMap<String, String> {
    let value: serde_json::Value = serde_json::from_str(text).expect("the manifest parses");
    value
        .get(field)
        .and_then(serde_json::Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .map(|(name, spec)| (name.clone(), spec.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn test_an_addition_leaves_every_other_member_as_it_was_written(
        plan in plan(),
        name in package_name(),
        spec in spec(),
        target in target(),
    ) {
        let before = manifest(&plan);
        let after = edit::add(&before, &[Addition { name, spec, group: target }])
            .expect("a well-formed manifest can be edited");

        prop_assert_eq!(untouched(&after), untouched(&before));
    }

    #[test]
    fn test_an_addition_is_listed_once_in_the_group_it_was_sent_to(
        plan in plan(),
        name in package_name(),
        spec in spec(),
        target in target(),
    ) {
        let before = manifest(&plan);
        let after = edit::add(
            &before,
            &[Addition { name: name.clone(), spec: spec.clone(), group: target }],
        )
        .expect("a well-formed manifest can be edited");

        let listed: Vec<&str> = ["dependencies", "devDependencies", "optionalDependencies"]
            .into_iter()
            .filter(|field| declared(&after, field).get(&name) == Some(&spec))
            .collect();
        match target {
            Some(group) => prop_assert_eq!(listed, vec![group.field()]),
            None => prop_assert!(!listed.is_empty()),
        }
        for field in ["dependencies", "devDependencies", "optionalDependencies"] {
            let stale = declared(&after, field)
                .get(&name)
                .is_some_and(|other| other != &spec);
            prop_assert!(!stale, "{} still lists {} at another range", field, name);
        }
    }

    #[test]
    fn test_adding_then_removing_a_new_name_gives_back_the_original_bytes(
        plan in plan(),
        name in package_name(),
        spec in spec(),
        target in target(),
    ) {
        prop_assume!(!plan.dependencies.contains_key(&name));
        prop_assume!(!plan.dev_dependencies.contains_key(&name));
        let before = manifest(&plan);

        let added = edit::add(&before, &[Addition { name: name.clone(), spec, group: target }])
            .expect("a well-formed manifest can be edited");
        let removed = edit::remove(&added, &[name]).expect("what was added can be removed");

        // `{}` has no layout to learn from, so it comes back in the default
        // one; every manifest with a member comes back byte for byte.
        if before != "{}" {
            prop_assert_eq!(removed, before);
        }
    }

    #[test]
    fn test_adding_the_same_thing_twice_changes_nothing_the_second_time(
        plan in plan(),
        name in package_name(),
        spec in spec(),
        target in target(),
    ) {
        let addition = [Addition { name, spec, group: target }];
        let once = edit::add(&manifest(&plan), &addition)
            .expect("a well-formed manifest can be edited");
        let twice = edit::add(&once, &addition).expect("an edited manifest can be edited");

        prop_assert_eq!(twice, once);
    }

    #[test]
    fn test_a_removal_leaves_every_other_entry_and_member_alone(plan in plan()) {
        let Some(name) = plan.dependencies.keys().next().cloned() else {
            return Ok(());
        };
        let before = manifest(&plan);
        let after = edit::remove(&before, std::slice::from_ref(&name))
            .expect("a declared name can be removed");

        prop_assert_eq!(untouched(&after), untouched(&before));
        let mut expected = plan.dependencies.clone();
        expected.remove(&name);
        prop_assert_eq!(declared(&after, "dependencies"), expected);
        let mut expected = plan.dev_dependencies.clone();
        expected.remove(&name);
        prop_assert_eq!(declared(&after, "devDependencies"), expected);
    }

    #[test]
    fn test_no_text_makes_an_edit_panic(text in ".{0,200}", name in package_name()) {
        let _ = edit::add(
            &text,
            &[Addition { name: name.clone(), spec: "^1.0.0".to_string(), group: None }],
        );
        let _ = edit::remove(&text, &[name]);
    }
}
