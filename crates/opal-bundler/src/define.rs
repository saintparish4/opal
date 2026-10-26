//! `--define`: replacing reads of a global property path with an
//! expression (`SEMANTICS.md` rule 14).

use oxc_ast::ast::{Expression, SimpleAssignmentTarget};
use oxc_semantic::Scoping;

use crate::options::Define;

#[derive(Clone, Debug, Default)]
pub struct Defines {
    entries: Vec<Define>,
}

impl Defines {
    pub fn new(entries: Vec<Define>) -> Self {
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The replacement for `expression` when it reads a defined path whose
    /// root is a global, not a local binding.
    pub fn matches(&self, expression: &Expression<'_>, scoping: &Scoping) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        let path = path_of(expression, scoping)?;
        self.entries
            .iter()
            .find(|define| {
                define
                    .path
                    .iter()
                    .map(String::as_str)
                    .eq(path.iter().copied())
            })
            .map(|define| define.value.as_str())
    }

    /// Whether an assignment target is a defined path.
    pub fn matches_target(&self, target: &SimpleAssignmentTarget<'_>, scoping: &Scoping) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let path = match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
                let global = ident
                    .reference_id
                    .get()
                    .is_some_and(|id| scoping.get_reference(id).symbol_id().is_none());
                global.then(|| vec![ident.name.as_str()])
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                path_of(&member.object, scoping).map(|mut path| {
                    path.push(member.property.name.as_str());
                    path
                })
            }
            _ => None,
        };
        let Some(path) = path else {
            return false;
        };
        self.entries.iter().any(|define| {
            define
                .path
                .iter()
                .map(String::as_str)
                .eq(path.iter().copied())
        })
    }
}

fn path_of<'e>(expression: &'e Expression<'_>, scoping: &Scoping) -> Option<Vec<&'e str>> {
    match expression {
        Expression::Identifier(ident) => {
            let global = ident
                .reference_id
                .get()
                .is_some_and(|id| scoping.get_reference(id).symbol_id().is_none());
            global.then(|| vec![ident.name.as_str()])
        }
        Expression::StaticMemberExpression(member) => {
            let mut path = path_of(&member.object, scoping)?;
            path.push(member.property.name.as_str());
            Some(path)
        }
        _ => None,
    }
}
