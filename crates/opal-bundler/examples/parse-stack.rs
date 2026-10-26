//! How much stack oxc's parser, and a walk over the tree it builds, take
//! per level of nesting, for each form nesting can take, so the stack a
//! module is parsed on can be sized to the file.
//!
//! ```text
//! cargo run --release -p opal-bundler --example parse-stack
//! cargo run -p opal-bundler --example parse-stack     # a debug build's frames
//! ```
//!
//! For each form it writes a module nested `LOW` and then `HIGH` levels
//! deep, finds by bisection the smallest thread stack that parses each, and
//! prints the difference per level, and per byte of source a level takes.
//! A trial parses the module as the bundler does and then walks the whole
//! tree, as the module graph's resolver does to find imports. Every trial
//! runs in a child process, since overflowing the stack aborts the process.

use std::path::Path;
use std::process::{Command, ExitCode};

use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

/// A form of nesting: the module is `before`, `open` repeated, `core`,
/// `close` repeated, `after`. `{i}` in `open` is replaced by the level.
struct Form {
    name: &'static str,
    extension: &'static str,
    bracket: bool,
    before: &'static str,
    open: &'static str,
    core: &'static str,
    close: &'static str,
    after: &'static str,
}

const fn form(
    name: &'static str,
    extension: &'static str,
    before: &'static str,
    open: &'static str,
    core: &'static str,
    close: &'static str,
    after: &'static str,
) -> Form {
    Form {
        name,
        extension,
        bracket: false,
        before,
        open,
        core,
        close,
        after,
    }
}

const fn bracket(
    name: &'static str,
    before: &'static str,
    open: &'static str,
    core: &'static str,
    close: &'static str,
    after: &'static str,
) -> Form {
    Form {
        name,
        extension: "mjs",
        bracket: true,
        before,
        open,
        core,
        close,
        after,
    }
}

const FORMS: &[Form] = &[
    form(
        "a?1:",
        "mjs",
        "var a;export const v = ",
        "a?1:",
        "0",
        "",
        ";",
    ),
    form("a=", "mjs", "var a;export const v = ", "a=", "0", "", ";"),
    form("a=>", "mjs", "export const v = ", "a=>", "0", "", ";"),
    form(
        "async a=>",
        "mjs",
        "export const v = ",
        "async a=>",
        "0",
        "",
        ";",
    ),
    form("2**", "mjs", "export const v = ", "2**", "1", "", ";"),
    form("!", "mjs", "export const v = ", "!", "1", "", ";"),
    form("~", "mjs", "export const v = ", "~", "1", "", ";"),
    form("- ", "mjs", "export const v = ", "- ", "1", "", ";"),
    form(
        "typeof ",
        "mjs",
        "export const v = ",
        "typeof ",
        "1",
        "",
        ";",
    ),
    form("await ", "mjs", "export const v = ", "await ", "1", "", ";"),
    form(
        "new ",
        "mjs",
        "function F(){}export const v = ",
        "new ",
        "F",
        "",
        ";",
    ),
    form(
        "yield ",
        "mjs",
        "export function* g(){",
        "yield ",
        "1",
        "",
        "}",
    ),
    form(
        "class extends ",
        "mjs",
        "export const v = ",
        "class extends ",
        "Object",
        "{}",
        ";",
    ),
    form("if(a)", "mjs", "var a;", "if(a)", ";", "", "export {}"),
    form(
        "if(a);else ",
        "mjs",
        "var a;",
        "if(a);else ",
        ";",
        "",
        "export {}",
    ),
    form(
        "while(a)",
        "mjs",
        "var a;",
        "while(a)",
        ";",
        "",
        "export {}",
    ),
    form("for(;;)", "mjs", "", "for(;;)", ";", "", "export {}"),
    form("label:", "mjs", "", "l{i}:", ";", "", "export {}"),
    form("<a>", "jsx", "export const v = ", "<a>", "x", "</a>", ";"),
    form("Array<", "ts", "export type T = ", "Array<", "1", ">", ";"),
    form(
        "extends ?:",
        "ts",
        "type A = 1;export type T = ",
        "A extends 1 ? 1 : ",
        "0",
        "",
        ";",
    ),
    bracket("(", "export const v = ", "(", "1", ")", ";"),
    bracket("[", "export const v = ", "[", "1", "]", ";"),
    bracket("{a:", "export const v = ", "{a:", "1", "}", ";"),
    bracket("{ block", "", "{", ";", "}", "export {}"),
    bracket("`${", "export const v = ", "`${", "1", "}`", ";"),
    bracket("function(){", "", "function f(){", ";", "}", "export {}"),
    bracket("()=>{", "export const v = ", "()=>{", "", "}", ";"),
];

const LOW: usize = 1_000;
const HIGH: usize = 3_000;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--child") {
        return child(&args[2], args[3].parse().unwrap());
    }
    let work = tempfile::tempdir().unwrap();
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("oxc_parser and a full walk, stack per level of nesting, {profile} build");
    println!();
    println!("| Form | Source per level | Stack per level | Stack per source byte |");
    println!("|---|---|---|---|");
    let (mut worst_form, mut worst_bracket) = (0.0f64, 0.0f64);
    for form in FORMS {
        let low = smallest_stack(form, LOW, work.path());
        let high = smallest_stack(form, HIGH, work.path());
        let (Some(low), Some(high)) = (low, high) else {
            println!("| `{}` | | does not parse | |", form.name);
            continue;
        };
        let per_level = high.saturating_sub(low) as f64 / (HIGH - LOW) as f64;
        let source =
            (module(form, HIGH).len() - module(form, LOW).len()) as f64 / (HIGH - LOW) as f64;
        let per_byte = per_level / source;
        if form.bracket {
            worst_bracket = worst_bracket.max(per_level);
        } else {
            worst_form = worst_form.max(per_byte);
        }
        println!(
            "| `{}`{} | {source:.1} B | {per_level:.0} B | {per_byte:.0} B |",
            form.name,
            if form.bracket { " (bracket)" } else { "" }
        );
    }
    println!();
    println!("worst per source byte, outside brackets: {worst_form:.0} B");
    println!("worst per bracket level: {worst_bracket:.0} B");
    ExitCode::SUCCESS
}

fn module(form: &Form, levels: usize) -> String {
    let mut text = String::from(form.before);
    for level in 0..levels {
        text.push_str(&form.open.replace("{i}", &level.to_string()));
    }
    text.push_str(form.core);
    text.push_str(&form.close.repeat(levels));
    text.push_str(form.after);
    text
}

/// The smallest stack, to within 1%, on which the module parses without
/// error, or None if it does not parse on 4 GiB.
fn smallest_stack(form: &Form, levels: usize, work: &Path) -> Option<usize> {
    let path = work.join(format!("module.{}", form.extension));
    std::fs::write(&path, module(form, levels)).unwrap();
    let parses = |stack: usize| {
        Command::new(std::env::current_exe().unwrap())
            .arg("--child")
            .arg(&path)
            .arg(stack.to_string())
            .output()
            .unwrap()
            .status
            .success()
    };
    let (mut low, mut high) = (64 * 1024, 4usize << 30);
    if !parses(high) {
        return None;
    }
    while high - low > high / 100 {
        let middle = low + (high - low) / 2;
        if parses(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    Some(high)
}

/// Every node, visited.
struct Walk;

impl<'a> Visit<'a> for Walk {}

/// Parses the file at `path` on a thread with `stack` bytes, as the bundler
/// parses a module, walks the tree, and exits 0 only if it parsed without
/// error.
fn child(path: &str, stack: usize) -> ExitCode {
    let text = std::fs::read_to_string(path).unwrap();
    let source_type = SourceType::from_path(path).unwrap().with_module(true);
    let clean = std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, &text, source_type)
                .with_options(ParseOptions {
                    parse_regular_expression: true,
                    ..ParseOptions::default()
                })
                .parse();
            Walk.visit_program(&parsed.program);
            parsed.diagnostics.is_empty()
        })
        .unwrap()
        .join()
        .unwrap();
    if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}
