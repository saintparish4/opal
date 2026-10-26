//! esbuild and rollup on the same fixtures as opal, every bundle held to
//! the same Node run of the unbundled program.
//!
//! For each fixture, each bundler's output is run under Node and compared
//! with the original by the oracle, exactly as opal's is. Where the bundlers
//! disagree, the original's run is the arbiter: the bundler whose output
//! matches it is right. A fixture opal refuses is one Node also fails to
//! link (or one the contract lists); another bundler's output for it is
//! run all the same, so the table shows what bundling it anyway does.
//!
//! ```text
//! cargo test -p opal-bundler --test arbiters -- --ignored --nocapture
//! OPAL_WRITE_ARBITERS=1 cargo test -p opal-bundler --test arbiters -- --ignored   # rewrite tests/arbiters/RESULTS.md
//! ```
//!
//! Needs node and the tools in crates/opal-bundler/tools. Each divergent
//! bundle is kept under tests/arbiters/snapshots/<fixture>/ with what the
//! oracle saw, so the evidence is readable without rerunning anything.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use opal_bundler::oracle::{self, Node};
use support::{Expect, Fixture, OUT_DIR};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Opal,
    Esbuild,
    Rollup,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Opal => "opal",
            Tool::Esbuild => "esbuild",
            Tool::Rollup => "rollup",
        }
    }
}

enum Verdict {
    Equivalent,
    KnownDivergence,
    Refused(String),
    NotBuilt(String),
    Diverged(String),
}

impl Verdict {
    fn cell(&self) -> String {
        match self {
            Verdict::Equivalent => "equivalent".to_string(),
            Verdict::KnownDivergence => "known divergence (listed)".to_string(),
            Verdict::Refused(code) => format!("refused `{code}`"),
            Verdict::NotBuilt(reason) => format!("not built: {}", clip(reason)),
            Verdict::Diverged(detail) => format!("**diverged**: {}", clip(detail)),
        }
    }
}

fn clip(text: &str) -> String {
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // An ANSI color sequence: ESC [ ... m.
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    let line = plain
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let line: String = line.chars().take(110).collect();
    line.replace('|', "\\|")
}

fn build_other(tool: Tool, fixture: &Fixture, program: &Path) -> Result<PathBuf, String> {
    let out = program.join(OUT_DIR).join(format!("{}.mjs", tool.name()));
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let options = support::options(fixture);
    let browser = options.target == opal_bundler::Target::Browser;
    let output = match tool {
        Tool::Esbuild => {
            let mut command = Command::new(support::tool("esbuild"));
            command
                .arg(&fixture.entry)
                .args(["--bundle", "--format=esm", "--log-level=error"])
                .arg(if browser { "--platform=browser" } else { "--platform=node" })
                .arg(format!("--outfile={}", out.display()))
                .current_dir(program);
            for external in &options.externals {
                command.arg(format!("--external:{external}"));
            }
            for define in &options.defines {
                command.arg(format!("--define:{}={}", define.path.join("."), define.value));
            }
            command.output()
        }
        Tool::Rollup => {
            let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/node_modules/@rollup");
            let config = program.join(OUT_DIR).join("rollup.config.mjs");
            let externals: Vec<String> = options.externals.iter().map(|e| format!("{e:?}")).collect();
            std::fs::write(
                &config,
                format!(
                    "import resolve from {resolve:?};\nimport commonjs from {commonjs:?};\nimport json from {json:?};\nexport default {{ input: {entry:?}, external: (id) => id.startsWith('node:') || [{externals}].some((e) => id === e || id.startsWith(e + '/')), output: {{ file: {out:?}, format: 'es', inlineDynamicImports: true }}, plugins: [resolve({{ preferBuiltins: true, exportConditions: [{conditions}] }}), commonjs(), json()], onwarn() {{}} }};\n",
                    resolve = plugins.join("plugin-node-resolve/dist/es/index.js").display().to_string(),
                    commonjs = plugins.join("plugin-commonjs/dist/es/index.js").display().to_string(),
                    json = plugins.join("plugin-json/dist/es/index.js").display().to_string(),
                    entry = fixture.entry,
                    externals = externals.join(", "),
                    out = out.display().to_string(),
                    conditions = if browser { "'browser'" } else { "'node'" },
                ),
            )
            .unwrap();
            Command::new(support::tool("rollup"))
                .arg("--config")
                .arg(&config)
                .arg("--silent")
                .current_dir(program)
                .output()
        }
        Tool::Opal => unreachable!(),
    }
    .map_err(|error| error.to_string())?;
    if !output.status.success() || !out.is_file() {
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        // A tool that crashes, as rollup's parser does on deep nesting,
        // prints nothing, and its exit status is the only reason there is.
        if printed.trim().is_empty() {
            return Err(output.status.to_string());
        }
        return Err(printed);
    }
    Ok(out)
}

fn judge(tool: Tool, fixture: &Fixture, node: &Node, snapshots: Option<&Path>) -> Verdict {
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("program");
    let scratch = temp.path().join("oracle");
    support::copy_program(fixture, &program);
    let bundle = match tool {
        Tool::Opal => match support::build_fixture(fixture, &program) {
            Ok(bundle) => bundle,
            Err((code, _)) => return Verdict::Refused(code),
        },
        other => match build_other(other, fixture, &program) {
            Ok(bundle) => bundle,
            Err(reason) => return Verdict::NotBuilt(reason),
        },
    };
    let mut node = node.clone();
    if let Some(env) = fixture.config.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env {
            node = node.with_env(key, value.as_str().unwrap_or_default());
        }
    }
    let flags: Vec<String> = fixture
        .config
        .get("node_flags")
        .and_then(|v| v.as_array())
        .map(|flags| {
            flags
                .iter()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let original_file = match support::original_program(fixture, &program) {
        Ok(file) => file,
        Err(error) => return Verdict::NotBuilt(format!("the original: {error}")),
    };
    let original = match node
        .clone()
        .with_flags(&flags)
        .observe(&program, &original_file, &scratch)
    {
        Ok(observation) => observation,
        Err(error) => return Verdict::NotBuilt(error.to_string()),
    };
    let bundled = match node.observe(&program, &bundle, &scratch) {
        Ok(observation) => observation,
        Err(error) => return Verdict::NotBuilt(error.to_string()),
    };
    match oracle::compare(&original, &bundled) {
        Ok(()) => Verdict::Equivalent,
        Err(_)
            if tool == Tool::Opal && matches!(fixture.expect, Expect::KnownDivergence { .. }) =>
        {
            Verdict::KnownDivergence
        }
        Err(divergence) => {
            if let Some(dir) = snapshots {
                let dir = dir.join(&fixture.name);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::copy(&bundle, dir.join(format!("{}.mjs", tool.name()))).unwrap();
                // The temporary directory's name changes every run.
                let text = divergence
                    .to_string()
                    .replace(&temp.path().display().to_string(), "<tmp>");
                std::fs::write(dir.join(format!("{}.diverged.txt", tool.name())), text).unwrap();
            }
            Verdict::Diverged(divergence.to_string())
        }
    }
}

#[test]
#[ignore = "needs node and the tools; run with --ignored"]
fn test_arbiters() {
    let node = Node::default();
    let write = std::env::var("OPAL_WRITE_ARBITERS").is_ok();
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/arbiters");
    let snapshots = here.join("snapshots");
    if write {
        let _ = std::fs::remove_dir_all(&snapshots);
    }
    let fixtures: Vec<Fixture> = support::load_all()
        .into_iter()
        .filter(|f| !f.dir.join("hand-bundle.mjs").is_file())
        .collect();
    let tools = [Tool::Opal, Tool::Esbuild, Tool::Rollup];
    let rows: Vec<(String, Vec<Verdict>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = fixtures
            .iter()
            .map(|fixture| {
                let node = &node;
                let snapshots = write.then_some(snapshots.as_path());
                scope.spawn(move || {
                    let verdicts = tools
                        .iter()
                        .map(|tool| judge(*tool, fixture, node, snapshots))
                        .collect();
                    (fixture.name.clone(), verdicts)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut table = String::from("| Fixture | opal | esbuild | rollup |\n|---|---|---|---|\n");
    let mut opal_wrong = Vec::new();
    for (name, verdicts) in &rows {
        table.push_str(&format!(
            "| `{name}` | {} | {} | {} |\n",
            verdicts[0].cell(),
            verdicts[1].cell(),
            verdicts[2].cell()
        ));
        if matches!(verdicts[0], Verdict::Diverged(_)) {
            opal_wrong.push(name.clone());
        }
    }
    let count = |tool: usize, test: &dyn Fn(&Verdict) -> bool| {
        rows.iter().filter(|(_, v)| test(&v[tool])).count()
    };
    let summary: Vec<String> = tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            format!(
                "| {} | {} | {} | {} | {} |",
                tool.name(),
                count(i, &|v| matches!(
                    v,
                    Verdict::Equivalent | Verdict::KnownDivergence
                )),
                count(i, &|v| matches!(v, Verdict::Diverged(_))),
                count(i, &|v| matches!(v, Verdict::Refused(_))),
                count(i, &|v| matches!(v, Verdict::NotBuilt(_))),
            )
        })
        .collect();
    eprintln!("{table}");
    if write {
        let versions = |tool: &str| {
            Command::new(support::tool(tool))
                .arg("--version")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default()
        };
        let text = format!(
            "# esbuild and rollup on the same fixtures\n\nGenerated by `OPAL_WRITE_ARBITERS=1 cargo test -p opal-bundler --test arbiters -- --ignored` ([`tests/arbiters.rs`](../arbiters.rs)), with node {}, esbuild {}, and {} (with @rollup/plugin-node-resolve, -commonjs, and -json). Every bundle is run under Node and compared with the unbundled program by the same oracle as opal's: the original's run decides who is right. A divergent bundle, and what the oracle saw, is kept under [`snapshots/`](./snapshots/). Where a fixture's original is what `tsc` emits (rule 13), every bundler is held to that. Each bundler runs with its defaults for one ES module (esbuild: `--bundle --format=esm --platform=node`, which leaves out `--keep-names`; rollup: node-resolve, commonjs, and json, and no TypeScript plugin). opal's oracle self-tests are left out.\n\n| Bundler | Equivalent (or a divergence SEMANTICS.md lists) | Diverged | Refused | Not built |\n|---|---|---|---|---|\n{}\n\n{table}\n## What this shows\n\n- **The disagreements are about semantics, not syntax.** Where esbuild or rollup diverge, it is on what this contract is about: evaluation order in a cycle (`esm-cycle-order`), the TDZ in a lazy cycle and link-before-evaluate (`lazy-cycle-tdz`, `lazy-link-before-evaluate`, esbuild), `.name` after renaming (`esm-name-collisions`), which names a namespace holds when `export *` reaches one twice (`esm-star-namespace-cells`, and `esm-namespace-nested-ambiguous` for esbuild), CommonJS interop and `require()` of builtins from ES module output (`cjs-loader`, `esm-imports-cjs`, `json-modules`), `import.meta.url` and `__filename` naming the bundle (`builtins`, `import-meta`), how `require()` reads a package's `main` (`cjs-nested-package-main`), and sloppy mode (`cjs-sloppy`, which neither builds).\n- **Where opal refuses, bundling anyway mostly changes the program.** Of the fixtures opal refuses, the other two bundlers' output diverges from Node's run on most: the program fails to link in Node, and the bundle runs. The exception is `refuse-deep-nesting`, which Node cannot compile either: esbuild's bundle overflows the stack as the program does, and rollup crashes building it.\n- **Some refusals are conservative.** `refuse-tla-outside-entry` (both), `refuse-direct-eval` (esbuild), `refuse-lazy-top-level-arguments` (rollup), and `refuse-star-cycle-ambiguous` (rollup) bundle into something equivalent on these programs: a top-level `await` with no sibling to interleave, an `eval` that reads a name no renaming touched, a module rollup does not run in a function, and an ambiguous name that V8 gives whichever namespace of an `export *` cycle it builds second, which with this program's import order is the one rollup gives it. opal refuses the class of program, because it cannot tell these instances from the ones where it would matter.\n",
            node.version().unwrap_or_default(),
            versions("esbuild"),
            versions("rollup"),
            summary.join("\n"),
        );
        std::fs::create_dir_all(&here).unwrap();
        std::fs::write(here.join("RESULTS.md"), text).unwrap();
    }
    assert!(opal_wrong.is_empty(), "opal diverged on {opal_wrong:?}");
}
