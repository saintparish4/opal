//! Random module graphs, run unbundled and bundled under Node.
//!
//! Each seed gives one program: two to eight modules, ES modules and
//! CommonJS mixed, importing each other in every form the contract covers
//! (named, default, namespace, side-effect, `export … from`, `export *`,
//! `export * as`, `require()`, `import()`), with cycles, top-level names
//! drawn from a small pool so modules collide, live bindings mutated across
//! modules, and reads that may land in the TDZ. Every module logs as it runs
//! and probes what it imported, so evaluation order, values, names, and
//! errors all reach stdout, where the oracle compares them.
//!
//! The generator is deterministic: a seed is a reproducible program. A case
//! the bundler refuses is fine when Node also fails to link the program; a
//! case it builds must be equivalent.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::oracle::{self, Node};

/// splitmix64: small, fast, and good enough to pick shapes.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'t, T>(&mut self, items: &'t [T]) -> &'t T {
        &items[self.below(items.len())]
    }
}

/// Top-level names modules draw from, so they collide in the bundle.
const NAMES: &[&str] = &[
    "a", "b", "x", "value", "helper", "Thing", "count", "log", "make",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Esm,
    /// `"use strict"` or not.
    Cjs {
        strict: bool,
    },
}

#[derive(Clone)]
struct Export {
    name: String,
    /// let, const, var, function, class, or a function that bumps a let.
    form: &'static str,
}

struct Plan {
    kind: Kind,
    exports: Vec<Export>,
    has_default: bool,
}

/// A generated program: files by relative path, and the entry.
pub struct Generated {
    pub seed: u64,
    pub files: Vec<(String, String)>,
    pub entry: String,
}

impl Generated {
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        for (path, text) in &self.files {
            std::fs::write(dir.join(path), text)?;
        }
        Ok(())
    }
}

fn file_name(index: usize, kind: Kind) -> String {
    match kind {
        Kind::Esm => format!("m{index}.mjs"),
        Kind::Cjs { .. } => format!("m{index}.cjs"),
    }
}

const PRELUDE: &str = "const probe = (label, read) => {\n  try {\n    const v = read();\n    const shown = typeof v === \"function\" ? \"fn:\" + v.name : v !== null && typeof v === \"object\" ? \"keys:\" + Object.keys(v).join(\",\") : String(v);\n    console.log(label, shown);\n  } catch (error) {\n    console.log(label, \"threw\", error.constructor.name);\n  }\n};\n";

pub fn generate(seed: u64) -> Generated {
    let mut rng = Rng(seed);
    let count = 2 + rng.below(7);
    let plans: Vec<Plan> = (0..count)
        .map(|index| {
            let kind = if index == 0 || !rng.chance(30) {
                Kind::Esm
            } else {
                Kind::Cjs {
                    strict: rng.chance(60),
                }
            };
            let mut exports = Vec::new();
            for _ in 0..1 + rng.below(4) {
                let name = rng.pick(NAMES).to_string();
                if exports.iter().any(|e: &Export| e.name == name) {
                    continue;
                }
                let form = *rng.pick(&["let", "const", "var", "function", "class", "bump"]);
                exports.push(Export { name, form });
            }
            Plan {
                kind,
                exports,
                has_default: rng.chance(50),
            }
        })
        .collect();

    let mut files = Vec::new();
    for (index, plan) in plans.iter().enumerate() {
        let text = match plan.kind {
            Kind::Esm => esm_module(index, &plans, &mut rng),
            Kind::Cjs { strict } => cjs_module(index, strict, &plans, &mut rng),
        };
        files.push((file_name(index, plan.kind), text));
    }
    Generated {
        seed,
        files,
        entry: "m0.mjs".to_string(),
    }
}

fn esm_module(index: usize, plans: &[Plan], rng: &mut Rng) -> String {
    let mut imports = String::new();
    let mut body = String::new();
    let mut local_names: Vec<String> = Vec::new();
    let mut probes: Vec<(String, String)> = Vec::new();
    // Every name this module binds or exports, so it never declares one
    // twice: a duplicate is a SyntaxError, not a case worth generating.
    let mut taken: Vec<String> = plans[index]
        .exports
        .iter()
        .map(|e| e.name.clone())
        .collect();
    if plans[index].exports.iter().any(|e| e.form == "bump") {
        taken.extend(
            plans[index]
                .exports
                .iter()
                .map(|e| format!("bump{index}_{}", e.name)),
        );
    }

    for _ in 0..rng.below(4) {
        let target = rng.below(plans.len());
        if target == index && !rng.chance(20) {
            continue;
        }
        let plan = &plans[target];
        let file = file_name(target, plan.kind);
        let alias = format!("i{index}_{}", imports.lines().count());
        match plan.kind {
            Kind::Esm => match rng.below(7) {
                0 if !plan.exports.is_empty() => {
                    let export = rng.pick(&plan.exports);
                    // Sometimes import under the written name, to collide.
                    let local = if rng.chance(40) && !taken.contains(&export.name) {
                        export.name.clone()
                    } else {
                        alias.clone()
                    };
                    taken.push(local.clone());
                    let _ = writeln!(
                        imports,
                        "import {{ {} as {local} }} from \"./{file}\";",
                        export.name
                    );
                    local_names.push(local.clone());
                    probes.push((format!("{index}:{}", export.name), local));
                }
                1 if plan.has_default => {
                    let _ = writeln!(imports, "import {alias} from \"./{file}\";");
                    probes.push((format!("{index}:default{target}"), alias));
                }
                2 => {
                    let _ = writeln!(imports, "import * as {alias} from \"./{file}\";");
                    probes.push((format!("{index}:ns{target}"), alias));
                }
                3 if !plan.exports.is_empty() => {
                    let export = rng.pick(&plan.exports);
                    let name = format!("re{index}_{}", export.name);
                    if taken.contains(&name) {
                        continue;
                    }
                    taken.push(name);
                    let _ = writeln!(
                        imports,
                        "export {{ {} as re{index}_{} }} from \"./{file}\";",
                        export.name, export.name
                    );
                }
                4 => {
                    let _ = writeln!(imports, "export * from \"./{file}\";");
                }
                5 => {
                    let name = format!("star{target}");
                    if taken.contains(&name) {
                        continue;
                    }
                    taken.push(name);
                    let _ = writeln!(imports, "export * as star{target} from \"./{file}\";");
                }
                _ => {
                    let _ = writeln!(imports, "import \"./{file}\";");
                }
            },
            Kind::Cjs { .. } => match rng.below(3) {
                0 if !plan.exports.is_empty() => {
                    let export = rng.pick(&plan.exports);
                    let _ = writeln!(
                        imports,
                        "import {{ {} as {alias} }} from \"./{file}\";",
                        export.name
                    );
                    probes.push((format!("{index}:cjs.{}", export.name), alias));
                }
                1 => {
                    let _ = writeln!(imports, "import * as {alias} from \"./{file}\";");
                    probes.push((format!("{index}:cjsns{target}"), alias));
                }
                _ => {
                    let _ = writeln!(imports, "import {alias} from \"./{file}\";");
                    probes.push((format!("{index}:cjsdefault{target}"), alias));
                }
            },
        }
    }

    let _ = writeln!(body, "console.log(\"eval m{index}\");");
    // Probes run now, during evaluation, where a cycle partner may not have
    // run yet, and again later through a function, after it has.
    for (label, local) in &probes {
        let _ = writeln!(body, "probe({label:?}, () => {local});");
    }
    let plan = &plans[index];
    for export in &plan.exports {
        let name = &export.name;
        let text = match export.form {
            "let" => format!("export let {name} = \"{name} of m{index}\";"),
            "const" => format!("export const {name} = () => \"{name} of m{index}\";"),
            "var" => format!("export var {name} = class {{}};"),
            "function" => {
                format!("export function {name}() {{ return \"{name}() of m{index}\"; }}")
            }
            "class" => format!("export class {name} {{ static who = {name}.name; }}"),
            _ => format!(
                "export let {name} = 0;\nexport function bump{index}_{name}() {{ {name}++; }}"
            ),
        };
        let _ = writeln!(body, "{text}");
    }
    // Unexported names from the same pool, to collide.
    for _ in 0..rng.below(3) {
        let name = rng.pick(NAMES);
        if taken.iter().any(|n| n == name) {
            continue;
        }
        taken.push(name.to_string());
        local_names.push(name.to_string());
        let _ = writeln!(
            body,
            "const {name} = function () {{}};\nprobe(\"{index}:own {name}\", () => {name});"
        );
    }
    if plan.has_default {
        match rng.below(3) {
            0 => {
                let _ = writeln!(
                    body,
                    "export default function () {{ return \"default of m{index}\"; }}"
                );
            }
            1 => {
                let _ = writeln!(body, "export default class {{}}");
            }
            _ => {
                let _ = writeln!(body, "export default () => \"arrow default of m{index}\";");
            }
        }
    }
    for export in plan.exports.iter().filter(|e| e.form == "bump") {
        let _ = writeln!(body, "bump{index}_{}();", export.name);
    }
    let _ = writeln!(body, "export function late{index}() {{");
    for (label, local) in &probes {
        let _ = writeln!(body, "  probe(\"late {label}\", () => {local});");
    }
    let _ = writeln!(body, "}}");
    if index == 0 {
        // The entry probes everything late, then imports a few modules
        // dynamically, after all synchronous work, so promise timing does
        // not interleave with anything the program logs.
        let _ = writeln!(body, "late0();");
        // One chain, so the imports settle in a fixed order; sometimes
        // awaited at the top level instead.
        let awaited = rng.chance(15);
        let mut chain = String::from("Promise.resolve()");
        for _ in 0..rng.below(4) {
            let target = rng.below(plans.len());
            let file = file_name(target, plans[target].kind);
            if awaited {
                let _ = writeln!(
                    body,
                    "{{ const ns = await import(\"./{file}\"); console.log(\"dynamic {file}\", Object.keys(ns).join(\",\")); }}"
                );
            } else {
                let _ = write!(
                    chain,
                    ".then(() => import(\"./{file}\")).then((ns) => console.log(\"dynamic {file}\", Object.keys(ns).join(\",\")))"
                );
            }
        }
        if !awaited {
            let _ = writeln!(body, "{chain};");
        }
    }
    format!("{PRELUDE}{imports}{body}")
}

fn cjs_module(index: usize, strict: bool, plans: &[Plan], rng: &mut Rng) -> String {
    let mut text = String::new();
    if strict {
        text.push_str("\"use strict\";\n");
    }
    text.push_str(PRELUDE);
    let _ = writeln!(text, "console.log(\"eval m{index}\");");
    for export in &plans[index].exports {
        let name = &export.name;
        let _ = writeln!(
            text,
            "exports.{name} = function {name}() {{ return \"{name} of m{index}\"; }};"
        );
    }
    for _ in 0..rng.below(3) {
        let target = rng.below(plans.len());
        if let Kind::Cjs { .. } = plans[target].kind {
            let file = file_name(target, plans[target].kind);
            let _ = writeln!(
                text,
                "probe(\"{index}:require {file}\", () => require(\"./{file}\"));"
            );
        }
    }
    if rng.chance(30) {
        let _ = writeln!(
            text,
            "probe(\"{index}:this\", () => this === module.exports);"
        );
    }
    text
}

/// What one case concluded.
#[derive(Debug)]
pub enum Verdict {
    Equivalent,
    /// Refused with a code, and Node also fails to link the program.
    Refused(String),
    /// Refused with a code the contract lists for a program Node runs:
    /// allowed (a refusal always is), but counted, since each one is a
    /// program the bundler could have bundled.
    Conservative(String, String),
    Diverged(String),
}

/// Writes the program, bundles it, and runs both. `work` is scratch space.
pub fn run_case(case: &Generated, work: &Path, node: &Node) -> Verdict {
    let program = work.join("program");
    let _ = std::fs::remove_dir_all(&program);
    if let Err(error) = case.write(&program) {
        return Verdict::Diverged(format!("could not write the program: {error}"));
    }
    let scratch = work.join("oracle");
    let original = match node.observe(&program, &program.join(&case.entry), &scratch) {
        Ok(observation) => observation,
        Err(error) => return Verdict::Diverged(format!("original: {error}")),
    };
    let bundled = std::panic::catch_unwind(|| build(&program, &case.entry));
    let bundle = match bundled {
        Err(_) => return Verdict::Diverged("the bundler panicked".to_string()),
        Ok(Err((code, message))) => {
            // A refusal is right only if Node also fails before running.
            let failed_to_link = original
                .uncaught
                .as_ref()
                .is_some_and(|u| u.name.as_deref() == Some("SyntaxError"))
                && original.stdout.is_empty();
            return if failed_to_link {
                Verdict::Refused(code)
            } else if matches!(code.as_str(), "E-TLA" | "E-PARSE" | "E-LINK" | "E-DYNAMIC") {
                Verdict::Conservative(code, message)
            } else {
                Verdict::Diverged(format!(
                    "refused {code}, but Node runs the program: {message}"
                ))
            };
        }
        Ok(Ok(bundle)) => bundle,
    };
    let observed = match node.observe(&program, &bundle, &scratch) {
        Ok(observation) => observation,
        Err(error) => return Verdict::Diverged(format!("bundle: {error}")),
    };
    match oracle::compare(&original, &observed) {
        Ok(()) => Verdict::Equivalent,
        Err(divergence) => Verdict::Diverged(divergence.to_string()),
    }
}

fn build(program: &Path, entry: &str) -> Result<PathBuf, (String, String)> {
    use opal_core::graph::resolve;
    use opal_core::path::NormalizedPath;
    let options = crate::BuildOptions::default();
    let root = NormalizedPath::from_native(&std::fs::canonicalize(program).unwrap()).unwrap();
    let resolver = options.target.resolver_options(None);
    let resolution = resolve(&root, &NormalizedPath::new(entry), &resolver)
        .map_err(|error| ("E-IO".to_string(), error.to_string()))?;
    let out = root.as_path().join("dist");
    let mut output = crate::build(&resolution.graph, &out, &options)
        .map_err(|d| (d.code.to_string(), d.to_string()))?;
    crate::write(&out, &mut output).map_err(|d| (d.code.to_string(), d.to_string()))?;
    Ok(out.join(&output.file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_seed_is_a_reproducible_program() {
        let a = generate(42);
        let b = generate(42);
        assert_eq!(a.files, b.files);
        assert_ne!(generate(43).files, a.files);
    }
}
