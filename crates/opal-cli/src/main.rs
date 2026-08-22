//! The `opal` binary.
//!
//! Implements: resolving a module graph,
//! installing dependencies, and inspecting the shared cache. `run`, `build`, and
//! `test` are not here, because a command that exists and does nothing is worse
//! than one that does not exist. The same goes for `add`, `remove`, `update`,
//! and the analysis command — they arrive with the phase that
//! implements them.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use opal_core::cache::CacheRoot;
use opal_core::cas::gc::GcOptions;
use opal_core::graph::{ResolverOptions, resolve_cached};
use opal_core::path::NormalizedPath;
use opal_pm::diagnose::{self, Severity};
use opal_pm::gc as package_gc;
use opal_pm::install::{self, InstallOptions};
use opal_pm::locks::CacheLock;
use opal_pm::package::PackageStore;
use opal_pm::projects::ProjectIndex;
use opal_pm::registry::NpmRegistry;

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
    /// Install the dependencies in package.json.
    Install(InstallArgs),
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

#[derive(Args)]
struct InstallArgs {
    /// Project directory (default: the current directory).
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// Registry base URL. Defaults to $OPAL_REGISTRY, else the public registry.
    #[arg(long)]
    registry: Option<String>,
    /// Skip devDependencies.
    #[arg(long)]
    production: bool,
    /// Fail instead of re-resolving when opal.lock does not match package.json.
    #[arg(long)]
    frozen_lockfile: bool,
}

#[derive(Subcommand)]
enum CacheCommand {
    /// Re-hash every object and check it against its key.
    Verify {
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// Remove objects nothing points at, plus stale temp files.
    Gc {
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        /// Report what would be removed without removing it.
        #[arg(long)]
        dry_run: bool,
        /// Also treat this project's opal.lock as live, without recording it.
        /// Repeatable — for CI, where the cache outlives the checkout.
        #[arg(long = "project")]
        projects: Vec<PathBuf>,
    },
    /// Print the cache location.
    Path {
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
}

type Failure = Box<dyn std::error::Error>;

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("opal: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, Failure> {
    match cli.command {
        Command::Graph(args) => graph(args),
        Command::Install(args) => install_command(args),
        Command::Cache { command } => match command {
            CacheCommand::Verify { cache_dir } => verify(cache_dir),
            CacheCommand::Gc {
                cache_dir,
                dry_run,
                projects,
            } => collect(cache_dir, dry_run, &projects),
            CacheCommand::Path { cache_dir } => {
                println!("{}", cache_root(cache_dir)?.path().display());
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

fn graph(args: GraphArgs) -> Result<ExitCode, Failure> {
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

    // An unresolved specifier means nothing on its own: an absent optional peer
    // looks exactly like a broken tree until the manifests are consulted.
    let findings = diagnose::classify(&resolved.graph, root.as_path());
    if !findings.is_empty() {
        println!("unresolved specifiers: {}", findings.len());
        for finding in findings {
            let label = match finding.severity {
                Severity::Informational => "note ",
                Severity::Error => "error",
            };
            println!("  {label} {}: {}", finding.importer, finding.explain());
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn install_command(args: InstallArgs) -> Result<ExitCode, Failure> {
    let root = match args.root {
        Some(root) => root,
        None => std::env::current_dir()?,
    };
    let cache = cache_root(args.cache_dir)?;
    let store = PackageStore::open(cache.open_cas()?, cache.path())?;
    let projects = ProjectIndex::new(cache.path().join("projects"))?;
    let registry = match args.registry {
        Some(url) => NpmRegistry::new(url),
        None => NpmRegistry::discover(),
    };
    let options = InstallOptions {
        include_development: !args.production,
        frozen_lockfile: args.frozen_lockfile,
    };

    let started = Instant::now();
    let report = install::install(&root, &registry, &store, &projects, &options)?;
    let elapsed = started.elapsed();

    println!(
        "{} packages {} in {:.1?}",
        report.packages,
        if report.resolved {
            "resolved"
        } else {
            "from opal.lock"
        },
        elapsed
    );
    println!(
        "store:  {} fetched, {} already present",
        report.fetched, report.already_stored
    );
    println!(
        "link:   {} added, {} unchanged, {} removed ({} hardlinked, {} copied, {} bins)",
        report.link.added,
        report.link.unchanged,
        report.link.removed,
        report.link.files_linked,
        report.link.files_copied,
        report.link.bins
    );
    for (name, reason) in &report.skipped {
        println!("skipped {name}: {reason}");
    }
    Ok(ExitCode::SUCCESS)
}

fn verify(cache_dir: Option<PathBuf>) -> Result<ExitCode, Failure> {
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
    extra_projects: &[PathBuf],
) -> Result<ExitCode, Failure> {
    let root = cache_root(cache_dir)?;
    let cache = root.open()?;
    let store = PackageStore::open(cache.cas().clone(), root.path())?;
    let projects = ProjectIndex::new(root.path().join("projects"))?;

    // Collection waits for in-flight installs. Probing first only decides
    // whether to say so — `package_gc::collect` takes the real lock itself.
    if CacheLock::try_exclusive(root.path())?.is_none() {
        println!("waiting for in-flight installs to finish...");
    }

    let options = GcOptions {
        dry_run,
        ..GcOptions::default()
    };
    let memo_live: BTreeSet<_> = cache.live_outputs()?;
    let outcome = package_gc::collect(&store, &projects, extra_projects, &memo_live, &options)?;
    let (marks, sweep) = (outcome.marks, outcome.sweep);

    println!(
        "projects: {} tracked, {} forgotten",
        marks.projects,
        marks.forgotten.len()
    );
    println!(
        "packages: {} live ({} in a lockfile but never fetched here)",
        marks.packages, marks.unfetched
    );
    println!(
        "{} of {} objects {}, {:.1} MiB",
        sweep.objects_removed,
        sweep.objects_scanned,
        if dry_run { "collectable" } else { "removed" },
        sweep.bytes_reclaimed as f64 / (1024.0 * 1024.0)
    );
    println!("pointers: {} pruned", outcome.pointers_pruned);
    println!(
        "temp files: {} swept, {} still in flight",
        sweep.temp_files_removed, sweep.temp_files_kept
    );
    for (path, error) in &marks.unreadable {
        eprintln!("warning: {}: {error}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn cache_root(explicit: Option<PathBuf>) -> Result<CacheRoot, Failure> {
    match explicit {
        Some(path) => Ok(CacheRoot::at(path)),
        None => Ok(CacheRoot::discover()?),
    }
}

fn absolute(path: &std::path::Path) -> Result<NormalizedPath, Failure> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(NormalizedPath::from_native(&path)?)
}
