//! `package.json` as `opal add` and `opal remove` edit it. This is the
//! project's own file, so it is trusted, but it is hand-written, and it is
//! the one file opal rewrites that it did not create: a panic here stops a
//! command, and a wrong edit damages something the user wrote.
//!
//! The first byte picks the edit and the rest is the manifest. Beyond "does
//! not panic", an edit that succeeds has to produce a manifest that says what
//! the edit was for and nothing else new: every member outside the dependency
//! groups is the value it was. And it never turns a manifest opal could read
//! into one it cannot.
//!
//! The editor accepts more than the reader does. It carries values as the
//! text they were written with, so a number too large to hold (`1e999`) goes
//! through an edit untouched, where reading the manifest refuses it. Such an
//! input is edited here and compared no further.

#![no_main]

use libfuzzer_sys::fuzz_target;
use opal_pm::edit::{self, AddRequest, Addition, Group};
use serde_json::Value;

const NAMES: [&str; 6] = ["a", "ms", "left-pad", "@scope/pkg", "z.z", "dependencies"];
const SPECS: [&str; 4] = ["^1.2.3", "1.0.0", ">=2 <3", "npm:other@^4.0.0"];
const GROUPS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

fn read(text: &str) -> Option<Value> {
    serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text)).ok()
}

fn outside_the_groups(mut manifest: Value) -> Value {
    let members = manifest
        .as_object_mut()
        .expect("an edit only succeeds on an object");
    for group in GROUPS {
        members.remove(group);
    }
    manifest
}

fn declared<'a>(manifest: &'a Value, group: &str, name: &str) -> Option<&'a Value> {
    manifest.get(group)?.get(name)
}

fuzz_target!(|data: &[u8]| {
    let Some((&choice, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    // Whatever was typed after `opal add`, including the manifest itself.
    if let Ok(request) = AddRequest::parse(text) {
        assert!(!request.name.is_empty());
    }

    let name = NAMES[usize::from(choice) % NAMES.len()];
    let spec = SPECS[usize::from(choice >> 3) % SPECS.len()];
    let group = match (choice >> 5) % 4 {
        0 => None,
        1 => Some(Group::Runtime),
        2 => Some(Group::Development),
        _ => Some(Group::Optional),
    };

    if choice & 0x80 == 0 {
        let addition = [Addition {
            name: name.to_string(),
            spec: spec.to_string(),
            group,
        }];
        let Ok(added) = edit::add(text, &addition) else {
            return;
        };
        let again = edit::add(&added, &addition).expect("an edited manifest can be edited");
        assert_eq!(
            again, added,
            "adding the same thing twice changed something"
        );
        let Some(before) = read(text) else {
            return;
        };
        let after = read(&added).expect("an edit made a readable manifest unreadable");
        let listed: Vec<&str> = Group::ALL
            .into_iter()
            .map(Group::field)
            .filter(|field| declared(&after, field, name) == Some(&Value::from(spec)))
            .collect();
        match group {
            Some(group) => assert_eq!(listed, [group.field()]),
            None => assert!(!listed.is_empty()),
        }
        assert_eq!(outside_the_groups(after), outside_the_groups(before));
    } else {
        let Ok(removed) = edit::remove(text, &[name.to_string()]) else {
            return;
        };
        assert!(
            edit::remove(&removed, &[name.to_string()]).is_err(),
            "removed a name that was already gone"
        );
        let Some(before) = read(text) else {
            return;
        };
        let after = read(&removed).expect("an edit made a readable manifest unreadable");
        for group in GROUPS {
            assert_eq!(declared(&after, group, name), None);
        }
        assert_eq!(outside_the_groups(after), outside_the_groups(before));
    }
});
