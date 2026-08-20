//! The `opal` binary.
//!
//! ships only what we implement: resolving a graph and inspecting
//! the shared cache. `install`, `run`, `build`, and `test` are not here, because
//! a command that exists and does nothing is worse than one that does not exist

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use opal_core::cache::CacheRoot;
use opal_core::cas::gc::{self, GcOptions};
use opal_core::graph::{ResolverOptions, resolve_cached};
use opal_core::path::NormalizedPath;

#[derive(Parser)]
#[command(
    name = "opal",
    version,
    about = "JavaScript toolkit built on one shared module graph"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Resolve a module graph from an entry file.
    Graph(GraphArgs),
    /// Inspect the shared content-addressed cache.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
}

#[derive(Args)]
struct GraphArgs {
    /// Entry file to walk from.
    entry: PathBuf,
    /// Project root; module paths are reported relative to it.
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// Print the resolved graph as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum CacheCommand {
    /// Re-hash every object and check it against its key.
    Verify {
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// Remove objects no memo record points at, plus stale temp files.
    Gc {
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        /// Report what would be removed without removing it.
        #[arg(long)]
        dry_run: bool,
    },
    /// Print the cache location.
    Path {
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("opal: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Command::Graph(args) => graph(args),
        Command::Cache { command } => match command {
            CacheCommand::Verify { cache_dir } => verify(cache_dir),
            CacheCommand::Gc { cache_dir, dry_run } => collect(cache_dir, dry_run),
            CacheCommand::Path { cache_dir } => {
                println!("{}", cache_root(cache_dir)?.path().display());
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

fn graph(args: GraphArgs) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let entry = absolute(&args.entry)?;
    let root = match &args.root {
        Some(root) => absolute(root)?,
        None => entry.parent().unwrap_or_else(|| entry.clone()),
    };
    let relative_entry = entry.relative_to(&root).unwrap_or_else(|| entry.clone());

    let cache = cache_root(args.cache_dir)?.open()?;
    let started = Instant::now();
    let resolved = resolve_cached(&cache, &root, &relative_entry, &ResolverOptions::default())?;
    let elapsed = started.elapsed();

    if args.json {
        println!("{}", resolved.graph.to_json());
        return Ok(ExitCode::SUCCESS);
    }

    println!(
        "{} modules, {} edges in {:.1?}",
        resolved.graph.len(),
        resolved.graph.edge_count(),
        elapsed
    );
    println!("cache:  {}", resolved.status);
    println!("digest: {}", resolved.graph.digest());
    println!("graph:  {}", resolved.output);

    let unresolved = resolved.graph.unresolved().count();
    if unresolved > 0 {
        println!("unresolved specifiers: {unresolved}");
        for diagnostic in resolved.graph.diagnostics() {
            println!("  {}: {}", diagnostic.module, diagnostic.message);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn verify(cache_dir: Option<PathBuf>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let cas = cache_root(cache_dir)?.open_cas()?;
    let report = cas.audit()?;

    println!(
        "{} objects, {:.1} MiB",
        report.objects,
        report.bytes as f64 / (1024.0 * 1024.0)
    );
    println!("temp files: {}", report.temp_files);

    if report.is_clean() {
        println!("all objects match their hash keys");
        return Ok(ExitCode::SUCCESS);
    }
    for hash in &report.corrupt {
        eprintln!("corrupt: {hash}");
    }
    for (hash, error) in &report.unreadable {
        eprintln!("unreadable: {hash}: {error}");
    }
    for path in &report.stray_files {
        eprintln!("stray: {}", path.display());
    }
    Ok(ExitCode::FAILURE)
}

fn collect(
    cache_dir: Option<PathBuf>,
    dry_run: bool,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let root = cache_root(cache_dir)?;
    let cache = root.open()?;
    let live: BTreeSet<_> = cache.live_outputs()?;
    let options = GcOptions {
        dry_run,
        ..GcOptions::default()
    };
    let report = gc::collect(cache.cas(), &live, &options)?;

    println!(
        "{} of {} objects {}, {:.1} MiB",
        report.objects_removed,
        report.objects_scanned,
        if dry_run { "collectable" } else { "removed" },
        report.bytes_reclaimed as f64 / (1024.0 * 1024.0)
    );
    println!(
        "temp files: {} swept, {} still in flight",
        report.temp_files_removed, report.temp_files_kept
    );
    Ok(ExitCode::SUCCESS)
}

fn cache_root(explicit: Option<PathBuf>) -> Result<CacheRoot, Box<dyn std::error::Error>> {
    match explicit {
        Some(path) => Ok(CacheRoot::at(path)),
        None => Ok(CacheRoot::discover()?),
    }
}

fn absolute(path: &std::path::Path) -> Result<NormalizedPath, Box<dyn std::error::Error>> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(NormalizedPath::from_native(&path)?)
}
