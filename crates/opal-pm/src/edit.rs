//! Edits to a project's `package.json`: text in, text out.
//!
//! This is the one hand-written file opal writes, so an edit changes what it
//! was asked to change and nothing else. Every top-level member is carried as
//! the bytes it was written with (an inline array stays inline, an escape
//! stays an escape) and only a dependency group an edit actually changes is
//! rendered again. The layout *between* top-level members is regenerated from
//! the file's own indent and line ending, which is the form npm and every
//! formatter write.
//!
//! Nothing here touches the filesystem. The install pipeline reads the file
//! under the project lock, edits the text, and writes it back atomically;
//! keeping that out of this module is what lets every edit be tested and
//! fuzzed as a pure function.
//!
//! A manifest that cannot be edited without guessing is refused, not repaired.
//! JSON parsers keep the last of two duplicate keys without saying so, and
//! rendering a group from that would silently delete the other entry.

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

use crate::manifest::Spec;
use crate::semver::Version;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("\"{field}\" appears more than once, so there is no one group to edit")]
    DuplicateGroup { field: &'static str },
    #[error("\"{name}\" appears more than once in \"{field}\"")]
    DuplicateName { field: &'static str, name: String },
    #[error("\"{field}\" is not an object")]
    GroupNotAnObject { field: &'static str },
    #[error("no dependency named {}", names.join(", "))]
    NotDeclared { names: Vec<String> },
}

/// An `opal add` argument that names nothing opal can install.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{argument}: {reason}")]
pub struct RequestError {
    pub argument: String,
    pub reason: &'static str,
}

/// A dependency group `opal add` writes to.
///
/// `peerDependencies` is deliberately not one of them: opal does not install
/// peers, so an entry added there would install nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Runtime,
    Development,
    Optional,
}

impl Group {
    pub const ALL: [Self; 3] = [Self::Runtime, Self::Development, Self::Optional];

    pub fn field(self) -> &'static str {
        match self {
            Self::Runtime => "dependencies",
            Self::Development => "devDependencies",
            Self::Optional => "optionalDependencies",
        }
    }
}

const PEER_FIELD: &str = "peerDependencies";

/// Every group `opal remove` clears a name from.
const FIELDS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    PEER_FIELD,
];

/// One `opal add` argument: a package name, and what followed its `@`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AddRequest {
    pub name: String,
    /// The specifier as typed. `None` for a bare name, which means "whatever
    /// `latest` points at".
    pub spec: Option<String>,
}

impl AddRequest {
    pub fn parse(argument: &str) -> Result<Self, RequestError> {
        let refuse = |reason| RequestError {
            argument: argument.to_string(),
            reason,
        };
        let trimmed = argument.trim();
        // A leading `@` opens a scope. The separator is the first `@` after
        // it, not the last: `alias@npm:@scope/pkg@^1` names `alias`.
        let separator = trimmed
            .char_indices()
            .skip(1)
            .find(|(_, character)| *character == '@')
            .map(|(index, _)| index);
        let (name, spec) = match separator {
            Some(index) => (&trimmed[..index], Some(trimmed[index + 1..].trim())),
            None => (trimmed, None),
        };
        if !is_package_name(name) {
            return Err(refuse(
                "not a registry package name (git, tarball, and local-path \
                 dependencies are not supported)",
            ));
        }
        let spec = spec.filter(|spec| !spec.is_empty());
        // `Range::parse` accepts a range with a line break in it, and
        // `opal.lock` has one fact per line and no escaping, so the lockfile
        // would refuse it later. Refusing it here names the argument.
        if spec.is_some_and(|spec| spec.chars().any(char::is_control)) {
            return Err(refuse("a version range cannot contain a control character"));
        }
        if let Some(spec) = spec
            && let Spec::Unsupported(_) = Spec::parse(spec)
        {
            return Err(refuse(
                "not a version, range, tag, or npm: alias (git, tarball, and \
                 local-path dependencies are not supported)",
            ));
        }
        Ok(Self {
            name: name.to_string(),
            spec: spec.map(str::to_string),
        })
    }
}

/// npm's own rule: a name is what survives `encodeURIComponent` unchanged,
/// optionally behind one `@scope/`.
///
/// That is also what keeps a path (`./lib`), a URL, and `github:user/repo`
/// from being read as a package: each has a `/` or a `:` where a name cannot.
fn is_package_name(name: &str) -> bool {
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

/// What `package.json` records for a package installed at `resolved`.
///
/// A version or range the user typed is kept as typed. npm rewrites both to
/// `^resolved`, which turns a typed pin into a range a later resolve can float
/// past. A bare name or a dist-tag has nothing worth keeping, so it becomes a
/// caret range on the version it led to.
pub fn saved_spec(typed: Option<&Spec>, resolved: &Version, exact: bool) -> String {
    let prefix = if exact { "" } else { "^" };
    match typed {
        Some(Spec::Alias { package, raw, .. }) => {
            let bare = raw.strip_prefix("npm:") == Some(package.as_str());
            if exact || bare {
                format!("npm:{package}@{prefix}{resolved}")
            } else {
                raw.clone()
            }
        }
        Some(Spec::Range(range)) if !exact => range.as_str().to_string(),
        // `AddRequest::parse` refuses these, so nothing reaches here to save.
        Some(Spec::Unsupported(text)) => text.clone(),
        Some(Spec::Range(_) | Spec::Tag(_)) | None => format!("{prefix}{resolved}"),
    }
}

/// One entry to write: `name` at `spec`, into `group`.
#[derive(Clone, Debug)]
pub struct Addition {
    pub name: String,
    pub spec: String,
    /// `None` keeps the package in whichever group already lists it, and
    /// falls back to `dependencies`. `Some` moves it there.
    pub group: Option<Group>,
}

/// `text` with every addition written into its group.
pub fn add(text: &str, additions: &[Addition]) -> Result<String, EditError> {
    let mut document = Document::parse(text)?;
    for addition in additions {
        let listed: Vec<Group> = Group::ALL
            .into_iter()
            .filter(|group| document.lists(group.field(), &addition.name))
            .collect();
        let targets = match addition.group {
            Some(group) => {
                for other in listed.iter().filter(|other| **other != group) {
                    document.unset(other.field(), &addition.name)?;
                }
                vec![group]
            }
            None if listed.is_empty() => vec![Group::Runtime],
            None => listed,
        };
        for group in targets {
            document.set(group.field(), &addition.name, &addition.spec)?;
        }
    }
    Ok(document.render())
}

/// `text` with every name gone from every group that listed it.
///
/// A name no group lists is an error, and nothing is removed: a mistyped
/// name must not look like a removal that worked.
pub fn remove(text: &str, names: &[String]) -> Result<String, EditError> {
    let mut document = Document::parse(text)?;
    let missing: Vec<String> = names
        .iter()
        .filter(|name| !FIELDS.iter().any(|field| document.lists(field, name)))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(EditError::NotDeclared { names: missing });
    }
    for name in names {
        for field in FIELDS {
            if document.lists(field, name) {
                document.unset(field, name)?;
            }
        }
    }
    Ok(document.render())
}

/// A JSON object's members in the order written, duplicates included, each
/// value left as its source text.
struct Members(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Members {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InOrder;

        impl<'de> Visitor<'de> for InOrder {
            type Value = Members;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Members, A::Error> {
                let mut members = Vec::new();
                while let Some(member) = map.next_entry::<String, Box<RawValue>>()? {
                    members.push(member);
                }
                Ok(Members(members))
            }
        }

        deserializer.deserialize_map(InOrder)
    }
}

struct Member {
    key: String,
    /// The value as written. What is rendered unless `group` says otherwise.
    raw: Box<RawValue>,
    group: Option<Entries>,
}

/// A dependency group's entries, each value as JSON source text.
struct Entries {
    entries: Vec<(String, String)>,
    /// Whether an edit changed this group. Only then is it rendered from
    /// `entries`, sorted; an unchanged group keeps its bytes.
    changed: bool,
}

struct Document<'a> {
    byte_order_mark: bool,
    indent: &'a str,
    newline: &'static str,
    /// Whatever followed the closing brace, usually one line ending.
    trailing: &'a str,
    members: Vec<Member>,
}

impl<'a> Document<'a> {
    fn parse(text: &'a str) -> Result<Self, EditError> {
        let body = text.strip_prefix('\u{feff}').unwrap_or(text);
        let Members(pairs) = serde_json::from_str(body)?;

        let mut members: Vec<Member> = Vec::with_capacity(pairs.len());
        for (key, raw) in pairs {
            let field = FIELDS.into_iter().find(|field| *field == key);
            if let Some(field) = field
                && members.iter().any(|member| member.key == key)
            {
                return Err(EditError::DuplicateGroup { field });
            }
            // A group that is not an object is left as the bytes it is, the
            // way the lenient manifest reader ignores it. Only an edit that
            // has to write into it is refused.
            let group = field.and_then(|_| {
                let Members(entries) = serde_json::from_str(raw.get()).ok()?;
                Some(Entries {
                    entries: entries
                        .into_iter()
                        .map(|(name, value)| (name, value.get().to_string()))
                        .collect(),
                    changed: false,
                })
            });
            members.push(Member { key, raw, group });
        }

        Ok(Self {
            byte_order_mark: body.len() != text.len(),
            indent: indent_of(body),
            newline: if body.contains("\r\n") { "\r\n" } else { "\n" },
            trailing: &body[body.trim_end().len()..],
            members,
        })
    }

    fn lists(&self, field: &str, name: &str) -> bool {
        self.members
            .iter()
            .filter(|member| member.key == field)
            .filter_map(|member| member.group.as_ref())
            .any(|group| group.entries.iter().any(|(entry, _)| entry == name))
    }

    /// The group to edit, created if the manifest has none.
    fn group_to_edit(&mut self, field: &'static str) -> Result<&mut Entries, EditError> {
        let index = match self.members.iter().position(|member| member.key == field) {
            Some(index) => index,
            None => {
                self.members.push(Member {
                    key: field.to_string(),
                    raw: RawValue::from_string("{}".to_string())
                        .expect("an empty object is valid JSON"),
                    group: Some(Entries {
                        entries: Vec::new(),
                        changed: false,
                    }),
                });
                self.members.len() - 1
            }
        };
        let group = self.members[index]
            .group
            .as_mut()
            .ok_or(EditError::GroupNotAnObject { field })?;

        let mut seen: Vec<&str> = Vec::with_capacity(group.entries.len());
        for (name, _) in &group.entries {
            if seen.contains(&name.as_str()) {
                return Err(EditError::DuplicateName {
                    field,
                    name: name.clone(),
                });
            }
            seen.push(name);
        }
        group.changed = true;
        Ok(group)
    }

    fn set(&mut self, field: &'static str, name: &str, spec: &str) -> Result<(), EditError> {
        let group = self.group_to_edit(field)?;
        let value = quoted(spec);
        match group.entries.iter_mut().find(|(entry, _)| entry == name) {
            Some((_, existing)) => *existing = value,
            None => group.entries.push((name.to_string(), value)),
        }
        Ok(())
    }

    fn unset(&mut self, field: &'static str, name: &str) -> Result<(), EditError> {
        let group = self.group_to_edit(field)?;
        group.entries.retain(|(entry, _)| entry != name);
        Ok(())
    }

    fn render(&self) -> String {
        let mut out = String::new();
        if self.byte_order_mark {
            out.push('\u{feff}');
        }
        out.push('{');
        let mut written = 0;
        for member in &self.members {
            let changed = member.group.as_ref().filter(|group| group.changed);
            // A group an edit emptied goes with its key, as npm and bun both
            // leave it. One that was already empty and untouched stays.
            if changed.is_some_and(|group| group.entries.is_empty()) {
                continue;
            }
            if written > 0 {
                out.push(',');
            }
            written += 1;
            out.push_str(self.newline);
            out.push_str(self.indent);
            // A key written with escapes comes back without them; the value
            // is the part that is kept byte for byte.
            out.push_str(&quoted(&member.key));
            out.push_str(": ");
            match changed {
                Some(group) => self.render_group(group, &mut out),
                None => out.push_str(member.raw.get()),
            }
        }
        if written > 0 {
            out.push_str(self.newline);
        }
        out.push('}');
        out.push_str(self.trailing);
        out
    }

    fn render_group(&self, group: &Entries, out: &mut String) {
        // Byte order, which is how `opal.lock` sorts and does not depend on
        // a locale. npm sorts with `localeCompare`, which differs only where
        // names differ by punctuation (`a_b` against `a-b`).
        let mut entries: Vec<&(String, String)> = group.entries.iter().collect();
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        out.push('{');
        for (index, (name, value)) in entries.into_iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(self.newline);
            out.push_str(self.indent);
            out.push_str(self.indent);
            out.push_str(&quoted(name));
            out.push_str(": ");
            out.push_str(value);
        }
        out.push_str(self.newline);
        out.push_str(self.indent);
        out.push('}');
    }
}

/// The whitespace the first indented line starts with, or two spaces for a
/// file with no indented line to learn from.
fn indent_of(body: &str) -> &str {
    body.lines()
        .skip(1)
        .find_map(|line| {
            let content = line.trim_start_matches([' ', '\t']);
            let indent = &line[..line.len() - content.len()];
            (!indent.is_empty() && !content.trim().is_empty()).then_some(indent)
        })
        .unwrap_or("  ")
}

fn quoted(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addition(name: &str, spec: &str, group: Option<Group>) -> Addition {
        Addition {
            name: name.to_string(),
            spec: spec.to_string(),
            group,
        }
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn version(text: &str) -> Version {
        Version::parse(text).expect("a valid version")
    }

    /// Four-space indent, an inline array, an escape, an unsorted group, and
    /// no trailing newline: everything an edit could be tempted to tidy.
    const AWKWARD: &str = "{\n    \"name\": \"probe\",\n    \"description\": \"caf\\u00e9\",\n    \"files\": [\"a.js\", \"b.js\"],\n    \"dependencies\": {\n        \"ms\": \"^2.0.0\",\n        \"is-odd\": \"3.0.1\"\n    }\n}";

    #[test]
    fn test_an_addition_sorts_its_group_and_touches_nothing_else() {
        let edited = add(AWKWARD, &[addition("is-number", "^7.0.0", None)]).unwrap();

        assert_eq!(
            edited,
            "{\n    \"name\": \"probe\",\n    \"description\": \"caf\\u00e9\",\n    \"files\": [\"a.js\", \"b.js\"],\n    \"dependencies\": {\n        \"is-number\": \"^7.0.0\",\n        \"is-odd\": \"3.0.1\",\n        \"ms\": \"^2.0.0\"\n    }\n}"
        );
    }

    #[test]
    fn test_adding_then_removing_gives_back_the_original_bytes() {
        let sorted = "{\n  \"name\": \"app\",\n  \"files\": [\"a.js\"],\n  \"dependencies\": {\n    \"is-odd\": \"3.0.1\",\n    \"ms\": \"^2.0.0\"\n  }\n}\n";

        let added = add(sorted, &[addition("left-pad", "^1.3.0", None)]).unwrap();
        let removed = remove(&added, &names(&["left-pad"])).unwrap();

        assert_ne!(added, sorted);
        assert_eq!(removed, sorted);
    }

    #[test]
    fn test_a_manifest_with_no_dependencies_gains_the_group_last() {
        let edited = add(
            "{\n  \"name\": \"app\"\n}\n",
            &[addition("ms", "^2.1.3", None)],
        )
        .unwrap();

        assert_eq!(
            edited,
            "{\n  \"name\": \"app\",\n  \"dependencies\": {\n    \"ms\": \"^2.1.3\"\n  }\n}\n"
        );
    }

    #[test]
    fn test_an_empty_manifest_gains_its_first_group() {
        assert_eq!(
            add("{}", &[addition("ms", "^2.1.3", None)]).unwrap(),
            "{\n  \"dependencies\": {\n    \"ms\": \"^2.1.3\"\n  }\n}"
        );
    }

    #[test]
    fn test_no_flag_keeps_a_package_in_the_group_that_lists_it() {
        let manifest = "{\n  \"devDependencies\": {\n    \"jest\": \"^29.0.0\"\n  }\n}\n";

        let edited = add(manifest, &[addition("jest", "^30.0.0", None)]).unwrap();

        assert_eq!(
            edited,
            "{\n  \"devDependencies\": {\n    \"jest\": \"^30.0.0\"\n  }\n}\n"
        );
    }

    #[test]
    fn test_a_group_flag_moves_a_package_and_drops_the_group_it_emptied() {
        let manifest = "{\n  \"dependencies\": {\n    \"ms\": \"^2.0.0\"\n  }\n}\n";

        let edited = add(
            manifest,
            &[addition("ms", "^2.1.3", Some(Group::Development))],
        )
        .unwrap();

        assert_eq!(
            edited,
            "{\n  \"devDependencies\": {\n    \"ms\": \"^2.1.3\"\n  }\n}\n"
        );
    }

    #[test]
    fn test_a_name_declared_only_as_a_peer_is_added_as_a_dependency() {
        let manifest = "{\n  \"peerDependencies\": {\n    \"react\": \"^19.0.0\"\n  }\n}\n";

        let edited = add(manifest, &[addition("react", "^19.2.8", None)]).unwrap();

        assert_eq!(
            edited,
            "{\n  \"peerDependencies\": {\n    \"react\": \"^19.0.0\"\n  },\n  \"dependencies\": {\n    \"react\": \"^19.2.8\"\n  }\n}\n"
        );
    }

    #[test]
    fn test_removal_clears_every_group_and_drops_the_ones_it_emptied() {
        let manifest = "{\n  \"dependencies\": {\n    \"ms\": \"^2.0.0\",\n    \"react\": \"^19.0.0\"\n  },\n  \"devDependencies\": {\n    \"react\": \"^19.0.0\"\n  },\n  \"peerDependencies\": {\n    \"react\": \"^19.0.0\"\n  }\n}\n";

        let edited = remove(manifest, &names(&["react"])).unwrap();

        assert_eq!(
            edited,
            "{\n  \"dependencies\": {\n    \"ms\": \"^2.0.0\"\n  }\n}\n"
        );
    }

    #[test]
    fn test_removing_an_undeclared_name_is_an_error_that_removes_nothing() {
        let manifest = "{\n  \"dependencies\": {\n    \"express\": \"^5.0.0\"\n  }\n}\n";

        let error = remove(manifest, &names(&["express", "expres"])).unwrap_err();

        assert!(
            matches!(&error, EditError::NotDeclared { names } if names == &["expres".to_string()]),
            "{error}"
        );
    }

    #[test]
    fn test_an_untouched_group_keeps_its_order_and_its_bytes() {
        let manifest = "{\n  \"dependencies\": {\"zod\": \"^4.0.0\", \"ms\": \"^2.0.0\"},\n  \"devDependencies\": {\n    \"jest\": \"^29.0.0\"\n  }\n}\n";

        let edited = add(
            manifest,
            &[addition("vitest", "^3.0.0", Some(Group::Development))],
        )
        .unwrap();

        assert!(
            edited.contains("\"dependencies\": {\"zod\": \"^4.0.0\", \"ms\": \"^2.0.0\"}"),
            "{edited}"
        );
    }

    #[test]
    fn test_windows_line_endings_and_tabs_are_kept() {
        let manifest = "{\r\n\t\"name\": \"app\",\r\n\t\"dependencies\": {\r\n\t\t\"ms\": \"^2.0.0\"\r\n\t}\r\n}\r\n";

        let edited = add(manifest, &[addition("is-odd", "3.0.1", None)]).unwrap();

        assert_eq!(
            edited,
            "{\r\n\t\"name\": \"app\",\r\n\t\"dependencies\": {\r\n\t\t\"is-odd\": \"3.0.1\",\r\n\t\t\"ms\": \"^2.0.0\"\r\n\t}\r\n}\r\n"
        );
    }

    #[test]
    fn test_a_byte_order_mark_survives_an_edit() {
        let manifest = "\u{feff}{\n  \"name\": \"app\"\n}\n";

        let edited = add(manifest, &[addition("ms", "^2.1.3", None)]).unwrap();

        assert!(edited.starts_with("\u{feff}{\n  \"name\""), "{edited:?}");
    }

    #[test]
    fn test_a_duplicate_name_in_the_edited_group_is_refused() {
        let manifest =
            "{\n  \"dependencies\": {\n    \"ms\": \"^1.0.0\",\n    \"ms\": \"^2.0.0\"\n  }\n}\n";

        let error = add(manifest, &[addition("is-odd", "3.0.1", None)]).unwrap_err();

        assert!(
            matches!(&error, EditError::DuplicateName { field: "dependencies", name } if name == "ms"),
            "{error}"
        );
    }

    #[test]
    fn test_a_duplicate_name_in_a_group_left_alone_is_kept_as_written() {
        let manifest =
            "{\n  \"dependencies\": {\n    \"ms\": \"^1.0.0\",\n    \"ms\": \"^2.0.0\"\n  }\n}\n";

        let edited = add(
            manifest,
            &[addition("jest", "^30.0.0", Some(Group::Development))],
        )
        .unwrap();

        assert!(
            edited.contains("\"ms\": \"^1.0.0\",\n    \"ms\": \"^2.0.0\""),
            "{edited}"
        );
    }

    #[test]
    fn test_a_dependency_group_written_twice_is_refused() {
        let manifest = "{\n  \"dependencies\": {},\n  \"dependencies\": {}\n}\n";

        let error = add(manifest, &[addition("ms", "^2.1.3", None)]).unwrap_err();

        assert!(
            matches!(
                error,
                EditError::DuplicateGroup {
                    field: "dependencies"
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn test_a_group_that_is_not_an_object_is_refused_only_when_written_to() {
        let manifest = "{\n  \"dependencies\": [],\n  \"devDependencies\": {\n    \"jest\": \"^29.0.0\"\n  }\n}\n";

        let error = add(manifest, &[addition("ms", "^2.1.3", None)]).unwrap_err();
        assert!(
            matches!(
                error,
                EditError::GroupNotAnObject {
                    field: "dependencies"
                }
            ),
            "{error}"
        );

        let edited = remove(manifest, &names(&["jest"])).unwrap();
        assert_eq!(edited, "{\n  \"dependencies\": []\n}\n");
    }

    #[test]
    fn test_text_that_is_not_a_json_object_is_refused() {
        assert!(matches!(
            add("[]", &[addition("ms", "^2.1.3", None)]),
            Err(EditError::Json(_))
        ));
        assert!(matches!(
            remove("{ not json", &names(&["ms"])),
            Err(EditError::Json(_))
        ));
    }

    #[test]
    fn test_a_request_splits_at_the_first_separator_after_a_scope() {
        let parse = |argument| AddRequest::parse(argument).unwrap();

        assert_eq!(
            parse("express"),
            AddRequest {
                name: "express".to_string(),
                spec: None
            }
        );
        assert_eq!(parse("@types/node").name, "@types/node");
        assert_eq!(parse("@types/node").spec, None);
        assert_eq!(parse("@types/node@^22").spec.as_deref(), Some("^22"));
        assert_eq!(parse("zod@3.20.0").spec.as_deref(), Some("3.20.0"));
        assert_eq!(parse("zod@latest").spec.as_deref(), Some("latest"));
        assert_eq!(parse("zod@").spec, None);

        let alias = parse("cliui-cjs@npm:@isaacs/cliui@^8.0.2");
        assert_eq!(alias.name, "cliui-cjs");
        assert_eq!(alias.spec.as_deref(), Some("npm:@isaacs/cliui@^8.0.2"));
    }

    #[test]
    fn test_a_request_for_something_other_than_a_registry_package_is_refused() {
        for argument in [
            "",
            "@",
            "@scope",
            "@scope/",
            "./local",
            "../up",
            "/absolute/path",
            "github:user/repo",
            "user/repo",
            "https://example.com/pkg.tgz",
            "git+ssh://git@github.com/user/repo.git",
            "has space",
            "new\nline",
            "pkg@github:user/repo",
            "pkg@file:../pkg",
            "pkg@https://example.com/pkg.tgz",
            "pkg@npm:other@latest",
            "pkg@>=1\n<2",
        ] {
            assert!(
                AddRequest::parse(argument).is_err(),
                "{argument:?} was accepted"
            );
        }
    }

    #[test]
    fn test_what_is_saved_is_what_was_typed_or_a_caret_on_what_was_resolved() {
        let resolved = version("6.0.3");
        let saved = |typed: Option<&str>, exact| {
            saved_spec(typed.map(Spec::parse).as_ref(), &resolved, exact)
        };

        assert_eq!(saved(None, false), "^6.0.3");
        assert_eq!(saved(Some("latest"), false), "^6.0.3");
        assert_eq!(saved(Some("6.0.3"), false), "6.0.3");
        assert_eq!(saved(Some("^6.0.0"), false), "^6.0.0");
        assert_eq!(saved(Some(">=6 <7"), false), ">=6 <7");
        assert_eq!(
            saved(Some("npm:kind-of@^6.0.0"), false),
            "npm:kind-of@^6.0.0"
        );
        assert_eq!(saved(Some("npm:kind-of"), false), "npm:kind-of@^6.0.3");

        assert_eq!(saved(None, true), "6.0.3");
        assert_eq!(saved(Some("latest"), true), "6.0.3");
        assert_eq!(saved(Some("^6.0.0"), true), "6.0.3");
        assert_eq!(saved(Some("npm:kind-of@^6.0.0"), true), "npm:kind-of@6.0.3");
    }
}
