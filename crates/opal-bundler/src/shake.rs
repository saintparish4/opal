//! Tree shaking (`SEMANTICS.md` rule 15): leaving out top-level declarations
//! nothing reads and whose evaluation cannot have an effect.
//!
//! "Cannot have an effect" is decided from syntax alone, conservatively: a
//! declaration is removable only if evaluating it creates functions,
//! classes, and literal values and nothing else. An identifier read is not
//! side-effect free, since it throws in the TDZ, and neither is any call,
//! `/* @__PURE__ */` annotated or not: an annotation is a claim, not a proof.
//!
//! "Nothing reads" is decided over the whole bundle: a binding is live if an
//! effectful statement reads it, a live declaration reads it, another module
//! imports it, a namespace object exposes it, or the entry exports it.

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    ArrayExpressionElement, BindingPattern, Class, ClassElement, Declaration,
    ExportDefaultDeclarationKind, Expression, ObjectPropertyKind, Program, PropertyKey, Statement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Scoping, SymbolId};

/// One top-level statement of an ES module.
#[derive(Clone, Debug, Default)]
pub struct TopStatement {
    /// Top-level bindings it declares.
    pub declares: Vec<SymbolId>,
    /// Whether it declares the `*default*` binding.
    pub declares_default: bool,
    /// Whether evaluating it cannot have an effect.
    pub pure: bool,
    /// Top-level bindings it reads or writes.
    pub reads: Vec<SymbolId>,
    /// Import, `export … from`, and `export { … }` statements, which the
    /// bundle drops whatever their liveness.
    pub linkage: bool,
}

/// Per-statement facts for an ES module's top level.
pub fn statements(program: &Program<'_>, scoping: &Scoping) -> Vec<TopStatement> {
    let root = scoping.root_scope_id();
    program
        .body
        .iter()
        .map(|statement| {
            let mut top = TopStatement {
                pure: statement_is_pure(statement),
                ..TopStatement::default()
            };
            match statement {
                Statement::ImportDeclaration(_)
                | Statement::ExportFromDeclaration(_)
                | Statement::ExportAllDeclaration(_)
                | Statement::ExportNamedDeclaration(_) => {
                    top.linkage = true;
                    return top;
                }
                Statement::ExportDeclaration(export) => {
                    top.declares = declared_symbols(&export.declaration)
                }
                Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => match &f.id {
                        Some(id) => top.declares.extend(id.symbol_id.get()),
                        None => top.declares_default = true,
                    },
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => match &c.id {
                        Some(id) => top.declares.extend(id.symbol_id.get()),
                        None => top.declares_default = true,
                    },
                    _ => top.declares_default = true,
                },
                _ => {
                    if let Some(declaration) = statement.as_declaration() {
                        top.declares = declared_symbols(declaration);
                    }
                }
            }
            let mut reads = Reads {
                scoping,
                root,
                found: Vec::new(),
            };
            reads.visit_statement(statement);
            let mut seen = HashSet::new();
            top.reads = reads
                .found
                .into_iter()
                .filter(|s| seen.insert(*s))
                .collect();
            top
        })
        .collect()
}

fn declared_symbols(declaration: &Declaration<'_>) -> Vec<SymbolId> {
    crate::analyze::declared(declaration)
        .into_iter()
        .map(|(_, symbol)| symbol)
        .collect()
}

struct Reads<'s> {
    scoping: &'s Scoping,
    root: oxc_syntax::scope::ScopeId,
    found: Vec<SymbolId>,
}

impl<'a> Visit<'a> for Reads<'_> {
    fn visit_identifier_reference(&mut self, reference: &oxc_ast::ast::IdentifierReference<'a>) {
        if let Some(id) = reference.reference_id.get()
            && let Some(symbol) = self.scoping.get_reference(id).symbol_id()
            && self.scoping.symbol_scope_id(symbol) == self.root
        {
            self.found.push(symbol);
        }
        walk::walk_identifier_reference(self, reference);
    }
}

fn statement_is_pure(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::FunctionDeclaration(_) | Statement::EmptyStatement(_) => true,
        Statement::ClassDeclaration(class) => class_is_pure(class),
        Statement::VariableDeclaration(declaration) => declaration.declarations.iter().all(|d| {
            matches!(d.id, BindingPattern::BindingIdentifier(_))
                && d.init.as_ref().is_none_or(expression_is_pure)
        }),
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::FunctionDeclaration(_) => true,
            Declaration::ClassDeclaration(class) => class_is_pure(class),
            Declaration::VariableDeclaration(declaration) => {
                declaration.declarations.iter().all(|d| {
                    matches!(d.id, BindingPattern::BindingIdentifier(_))
                        && d.init.as_ref().is_none_or(expression_is_pure)
                })
            }
            _ => false,
        },
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(_) => true,
            ExportDefaultDeclarationKind::ClassDeclaration(class) => class_is_pure(class),
            kind if kind.is_expression() => expression_is_pure(kind.to_expression()),
            _ => false,
        },
        _ => false,
    }
}

fn key_is_pure(key: &PropertyKey<'_>, computed: bool) -> bool {
    if !computed {
        return true;
    }
    matches!(
        key,
        PropertyKey::StringLiteral(_)
            | PropertyKey::NumericLiteral(_)
            | PropertyKey::BigIntLiteral(_)
    )
}

/// Defining the class runs nothing: no `extends` expression (it is
/// evaluated and checked), no decorators, no computed keys but literals, no
/// static blocks, and no static initializers that could do anything.
fn class_is_pure(class: &Class<'_>) -> bool {
    if class.heritage.is_some() || !class.decorators.is_empty() {
        return false;
    }
    class.body.body.iter().all(|element| match element {
        ClassElement::MethodDefinition(method) => {
            method.decorators.is_empty() && key_is_pure(&method.key, method.computed)
        }
        ClassElement::PropertyDefinition(property) => {
            property.decorators.is_empty()
                && key_is_pure(&property.key, property.computed)
                && (!property.r#static || property.value.as_ref().is_none_or(expression_is_pure))
        }
        ClassElement::AccessorProperty(property) => {
            property.decorators.is_empty()
                && key_is_pure(&property.key, property.computed)
                && (!property.r#static || property.value.as_ref().is_none_or(expression_is_pure))
        }
        ClassElement::StaticBlock(_) => false,
        ClassElement::TSIndexSignature(_) => true,
    })
}

/// Evaluating the expression creates a value and does nothing else.
pub fn expression_is_pure(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::NumericLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.is_empty(),
        Expression::ClassExpression(class) => class_is_pure(class),
        Expression::ParenthesizedExpression(inner) => expression_is_pure(&inner.expression),
        Expression::UnaryExpression(unary) => {
            use oxc_syntax::operator::UnaryOperator;
            // `typeof x` reads x; `delete` writes; `-`, `+`, `~` convert,
            // which can call valueOf. Only `void` and `!` of a pure value.
            matches!(
                unary.operator,
                UnaryOperator::Void | UnaryOperator::LogicalNot
            ) && expression_is_pure(&unary.argument)
        }
        Expression::ArrayExpression(array) => array.elements.iter().all(|element| match element {
            ArrayExpressionElement::SpreadElement(_) => false,
            ArrayExpressionElement::Elision(_) => true,
            element => element.as_expression().is_some_and(expression_is_pure),
        }),
        Expression::ObjectExpression(object) => {
            object.properties.iter().all(|property| match property {
                ObjectPropertyKind::SpreadProperty(_) => false,
                ObjectPropertyKind::ObjectProperty(property) => {
                    key_is_pure(&property.key, property.computed)
                        && expression_is_pure(&property.value)
                }
            })
        }
        _ => false,
    }
}

/// What a binding a module reads stands for, across modules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Live {
    Local(usize, SymbolId),
    Default(usize),
}

/// Which statements of which modules the bundle leaves out. `roots` are the
/// bindings something outside the shaken statements needs (a namespace, the
/// entry's exports, an unshaken module's imports); `import_target` says what
/// an import binding of a module stands for, so a live statement that reads
/// one makes the other module's declaration live too.
pub fn removable(
    statements: &HashMap<usize, Vec<TopStatement>>,
    roots: &[Live],
    import_target: &dyn Fn(usize, SymbolId) -> Option<Live>,
) -> HashMap<usize, HashSet<usize>> {
    let mut declared_by: HashMap<Live, Vec<(usize, usize)>> = HashMap::new();
    for (module, list) in statements {
        for (index, statement) in list.iter().enumerate() {
            for symbol in &statement.declares {
                declared_by
                    .entry(Live::Local(*module, *symbol))
                    .or_default()
                    .push((*module, index));
            }
            if statement.declares_default {
                declared_by
                    .entry(Live::Default(*module))
                    .or_default()
                    .push((*module, index));
            }
        }
    }
    let mut live: HashSet<(usize, usize)> = HashSet::new();
    let mut work: Vec<(usize, usize)> = Vec::new();
    let mark =
        |binding: Live, live: &mut HashSet<(usize, usize)>, work: &mut Vec<(usize, usize)>| {
            for declaring in declared_by.get(&binding).into_iter().flatten() {
                if live.insert(*declaring) {
                    work.push(*declaring);
                }
            }
        };
    for (module, list) in statements {
        for (index, statement) in list.iter().enumerate() {
            if !statement.linkage && !statement.pure && live.insert((*module, index)) {
                work.push((*module, index));
            }
        }
    }
    for root in roots {
        mark(*root, &mut live, &mut work);
    }
    while let Some((module, index)) = work.pop() {
        for symbol in &statements[&module][index].reads {
            let binding = import_target(module, *symbol).unwrap_or(Live::Local(module, *symbol));
            mark(binding, &mut live, &mut work);
        }
    }
    let mut out: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (module, list) in statements {
        for (index, statement) in list.iter().enumerate() {
            if !statement.linkage && !live.contains(&(*module, index)) {
                out.entry(*module).or_default().insert(index);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn pure(source: &str) -> bool {
        let allocator = Allocator::default();
        let program = Parser::new(&allocator, source, SourceType::mjs())
            .parse()
            .program;
        statement_is_pure(&program.body[0])
    }

    #[test]
    fn test_only_value_creating_declarations_are_pure() {
        for source in [
            "function f() { sideEffect(); }",
            "const a = 1, b = 'x', c = () => {}, d = [1, , 'y'], e = { k: function () {} };",
            "class A { static x = 1; y = sideEffect(); method() {} }",
            "let x;",
            "export const n = void 0;",
            "export default class { static [\"k\"] = 1 }",
        ] {
            assert!(pure(source), "{source} should be pure");
        }
        for source in [
            "const a = b;",
            "const a = f();",
            "const a = /* @__PURE__ */ f();",
            "const { a } = {};",
            "const a = `${b}`;",
            "const a = -x;",
            "const a = { ...b };",
            "class A extends B {}",
            "class A { static { sideEffect(); } }",
            "class A { static x = f(); }",
            "class A { [k] = 1 }",
            "sideEffect();",
            "export default f();",
        ] {
            assert!(!pure(source), "{source} should not be pure");
        }
    }
}
