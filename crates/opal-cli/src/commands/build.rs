//! `opal build`: bundle an entry into one ES module, or say why a module
//! is in the bundle.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::Args;
use opal_bundler::{BuildOptions, Define, Target};
use opal_core::graph::{
    DependencyKind, DependencyTarget, ModuleGraph, ModuleId, resolve, resolve_cached,
};
use opal_core::path::NormalizedPath;

use crate::style::Paint;
use crate::{Failure, absolute, cache_root, on_parse_stack};

#[derive(Args)]
pub struct BuildArgs {
    /// Entry file to bundle.
    pub entry: PathBuf,
    /// Project root; module paths are reported relative to it (default: the
    /// entry's directory).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Where to write <entry>.mjs and its source map (default: <root>/dist).
    #[arg(long)]
    pub outdir: Option<PathBuf>,
    /// The runtime the bundle is for: node or browser.
    #[arg(long, default_value = "node")]
    pub target: String,
    /// Keep a package (or root-relative path) out of the bundle. Repeatable.
    #[arg(long = "external")]
    pub externals: Vec<String>,
    /// Replace a global path with an expression: --define process.env.NODE_ENV='"production"'. Repeatable.
    #[arg(long = "define")]
    pub defines: Vec<String>,
    /// Import non-JavaScript files as Vite does: a default import is the
    /// URL of a copy in <outdir>/assets, and a .css import links the
    /// stylesheet (browser only). See SEMANTICS.md rule 17.
    #[arg(long)]
    pub assets: bool,
    /// Print why a module (root-relative path) is in the bundle, and build nothing.
    #[arg(long)]
    pub why: Option<String>,
    /// Print how long each phase took.
    #[arg(long)]
    pub timings: bool,
    /// Cache location. Defaults to $OPAL_CACHE_DIR, else the platform cache directory.
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,
}

pub fn build(args: BuildArgs) -> Result<ExitCode, Failure> {
    on_parse_stack("opal-build", move || build_here(args))
}

fn build_here(args: BuildArgs) -> Result<ExitCode, Failure> {
    let entry = absolute(&args.entry)?;
    let root = match &args.root {
        Some(root) => absolute(root)?,
        None => entry.parent().unwrap_or_else(|| entry.clone()),
    };
    let relative_entry = entry.relative_to(&root).unwrap_or_else(|| entry.clone());
    let target = Target::parse(&args.target)
        .ok_or_else(|| format!("--target {:?}: expected node or browser", args.target))?;
    let mut options = BuildOptions {
        target,
        externals: args.externals.clone(),
        assets: args.assets,
        ..BuildOptions::default()
    };
    for define in &args.defines {
        options.defines.push(Define::parse(define)?);
    }

    let resolver = options.resolver_options(root.as_path(), entry.as_path());
    if let Some(module) = &args.why {
        // Uncached: the answer includes what resolution read and probed,
        // which only a fresh resolution records.
        let resolution = resolve(&root, &relative_entry, &resolver)?;
        return why(&resolution.graph, &resolution.trace, module);
    }

    let started = Instant::now();
    let cache = cache_root(args.cache_dir)?.open()?;
    let resolved = resolve_cached(&cache, &root, &relative_entry, &resolver)?;
    let resolve_time = started.elapsed();

    let outdir = match &args.outdir {
        Some(outdir) => absolute(outdir)?,
        None => root.join("dist"),
    };
    let built = opal_bundler::build(&resolved.graph, outdir.as_path(), &options)
        .and_then(|mut output| opal_bundler::write(outdir.as_path(), &mut output).map(|()| output));
    let output = match built {
        Ok(output) => output,
        Err(diagnostic) => {
            let paint = Paint::for_stderr();
            eprintln!("{} {diagnostic}", paint.red("opal: refused"));
            eprintln!(
                "  {} is listed under \"Refusals\" in crates/opal-bundler/SEMANTICS.md",
                diagnostic.code
            );
            return Ok(ExitCode::FAILURE);
        }
    };
    let total = started.elapsed();
    let shown = NormalizedPath::from_native(&std::env::current_dir()?)
        .ok()
        .and_then(|cwd| outdir.relative_to(&cwd))
        .filter(|relative| !relative.as_str().starts_with(".."))
        .unwrap_or_else(|| outdir.clone());
    println!(
        "{} modules into {}/{} ({} KB) in {:.1?}",
        output.modules,
        shown,
        output.file_name,
        output.code.len().div_ceil(1024),
        total
    );
    if !output.trusted.is_empty() {
        let statements: usize = output.trusted.iter().map(|(_, count)| count).sum();
        println!(
            "left out {statements} statements on the word of {} modules' \"sideEffects\" (crates/opal-bundler/SEMANTICS.md rule 15)",
            output.trusted.len()
        );
    }
    if args.timings {
        let t = &output.timings;
        println!(
            "resolve {:.1?}, parse {:.1?}, link {:.1?}, print {:.1?}, write {:.1?}",
            resolve_time, t.parse, t.link, t.print, t.write
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// The shortest chain of imports from the entry to `module`, and what the
/// resolver read and probed to find it.
fn why(
    graph: &ModuleGraph,
    trace: &opal_core::graph::ResolveTrace,
    module: &str,
) -> Result<ExitCode, Failure> {
    let target = NormalizedPath::new(module);
    let Some(target_id) = graph.id_of(&target) else {
        println!(
            "{module} is not in the module graph of {}",
            graph.module(graph.entry()).path
        );
        return Ok(ExitCode::FAILURE);
    };
    // Breadth-first from the entry, so the chain printed is a shortest one.
    let mut previous: Vec<Option<(ModuleId, usize)>> = vec![None; graph.len()];
    let mut seen = vec![false; graph.len()];
    let mut queue = std::collections::VecDeque::from([graph.entry()]);
    seen[graph.entry().index()] = true;
    while let Some(id) = queue.pop_front() {
        for (index, dependency) in graph.module(id).dependencies.iter().enumerate() {
            if let DependencyTarget::Module { id: next } = dependency.target
                && !seen[next.index()]
            {
                seen[next.index()] = true;
                previous[next.index()] = Some((id, index));
                queue.push_back(next);
            }
        }
    }
    let mut chain = Vec::new();
    let mut current = target_id;
    while let Some((from, index)) = previous[current.index()] {
        chain.push((from, index));
        current = from;
    }
    chain.reverse();
    println!("{target} is in the bundle because:");
    if chain.is_empty() {
        println!("  it is the entry");
    }
    for (from, index) in &chain {
        let dependency = &graph.module(*from).dependencies[*index];
        let how = match dependency.kind {
            DependencyKind::StaticImport => "imports",
            DependencyKind::ExportFrom => "re-exports from",
            DependencyKind::DynamicImport => "dynamically imports",
            DependencyKind::Require => "requires",
            DependencyKind::TsImportEquals => "import-equals requires",
        };
        println!(
            "  {} {how} {:?}",
            graph.module(*from).path,
            dependency.specifier
        );
    }
    // What resolution read and probed next to the module: the facts the
    // answer rests on, from opal-core's ResolveTrace.
    println!("resolved from:");
    if let Some(hash) = trace.files.get(&target) {
        println!("  read   {target} ({})", &hash.to_hex()[..12]);
    }
    let directory = target.parent();
    let stem = target
        .as_str()
        .rsplit_once('.')
        .map_or(target.as_str(), |(stem, _)| stem);
    for missing in &trace.missing {
        if missing.parent() == directory && missing.as_str().starts_with(stem) {
            println!("  absent {missing}");
        }
    }
    Ok(ExitCode::SUCCESS)
}
