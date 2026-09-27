//! The `opal` binary.
//!
//! Implements: resolving a module graph,
//! installing dependencies, inspecting the shared cache, and upgrading itself. `run`, `build`, and
//! `test` are not here, because a command that exists and does nothing is worse
//! than one that does not exist. The same goes for `add`, `remove`, `update`,
//! and the analysis command — they arrive with the phase that
//! implements them.

mod progress;
mod upgrade;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use opal_core::cache::CacheRoot;
use opal_core::cas::gc::GcOptions;
use opal_core::graph::{ResolverOptions, resolve_cached};
use opal_core::path::NormalizedPath;
use opal_pm::diagnose::{self, Severity};
use opal_pm::gc as package_gc;
use opal_pm::install::{self, InstallOptions, InstallReport, UnrunScripts};
use opal_pm::locks::CacheLock;
use opal_pm::package::PackageStore;
use opal_pm::packuments::PackumentCache;
use opal_pm::projects::ProjectIndex;
use opal_pm::registry::{Freshness, HttpTransport, NpmRegistry, RetryPolicy, RetryingTransport};
use opal_pm::semver::Version;
use upgrade::{Outcome, Releases};

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
    /// Upgrade opal to the latest release, or to a given version.
    Upgrade(UpgradeArgs),
}

#[derive(Args)]
struct UpgradeArgs {
    /// Install this version instead of the latest, e.g. 0.3.1. An older one
    /// works too.
    version: Option<String>,
}

#[derive(Args)]
struct GraphArgs {
    /// Entry file to walk from.
    entry: PathBuf,
    /// Project root; module paths are reported relative to it.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Cache location. Defaults to $OPAL_CACHE_DIR, else the platform cache directory.
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
    /// Cache location. Defaults to $OPAL_CACHE_DIR, else the platform cache directory.
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// Registry base URL. Defaults to $OPAL_REGISTRY, else the public registry.
    #[arg(long)]
    registry: Option<String>,
    /// Skip devDependencies.
    #[arg(long)]
    production: bool,
    /// For CI: fail instead of re-resolving when opal.lock is missing or does not
    /// match package.json.
    #[arg(long)]
    frozen_lockfile: bool,
    /// Resolve from cached registry metadata only; never reach the network.
    #[arg(long)]
    offline: bool,
    /// Use cached registry metadata however old it is, and only fetch what is
    /// missing.
    #[arg(long, conflicts_with = "offline")]
    prefer_offline: bool,
}

#[derive(Subcommand)]
enum CacheCommand {
    /// Re-hash every object and check it against its key.
    Verify {
        /// Cache location. Defaults to $OPAL_CACHE_DIR, else the platform cache directory.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// Remove objects nothing points at, plus stale temp files.
    Gc {
        /// Cache location. Defaults to $OPAL_CACHE_DIR, else the platform cache directory.
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
        /// Cache location. Defaults to $OPAL_CACHE_DIR, else the platform cache directory.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
}

/// Registry metadata untouched for this long is dropped by `opal cache gc`.
/// Long enough that a project returned to after a fortnight still resolves
/// offline; short enough that the directory does not track every package ever
/// looked at.
const PACKUMENT_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

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
        Command::Upgrade(args) => upgrade_command(args),
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
    let freshness = match (args.offline, args.prefer_offline) {
        (true, _) => Freshness::Offline,
        (_, true) => Freshness::PreferOffline,
        _ => Freshness::Revalidate,
    };
    let registry = match args.registry {
        Some(url) => NpmRegistry::new(url),
        None => NpmRegistry::discover(),
    }
    // Packument metadata outlives the process here, so a re-resolve against a
    // warm store does not re-download what built it.
    .with_packument_cache(PackumentCache::new(cache.path().join("packuments")))
    .with_freshness(freshness);
    let options = InstallOptions {
        include_development: !args.production,
        frozen_lockfile: args.frozen_lockfile,
        ..InstallOptions::default()
    };

    let started = Instant::now();
    let reporter = progress::reporter();
    let report = install::install(
        &root,
        &registry,
        &store,
        &projects,
        &options,
        reporter.as_ref(),
    )?;
    let elapsed = started.elapsed();

    for line in install_summary(&report, elapsed) {
        println!("{line}");
    }
    if report.lockfile_upgraded {
        println!("opal.lock was written by an older build and has been re-resolved");
    }
    // Each of these names a dependency the project asked for and did not get,
    // so every one stays on its own line.
    for (name, reason) in &report.skipped {
        println!("skipped {name}: {reason}");
    }
    // These are the expected case, not a problem: a native package publishes
    // one build per platform, and a Next.js app alone skips 66 of them. One
    // line says it happened; `opal.lock` still records every one.
    if let Some(line) = platform_skip_summary(report.platform_skipped.len()) {
        println!("{line}");
    }

    // Warnings go last, after the result, so they are the last thing on screen
    // rather than buried above it.
    for (id, message) in &report.deprecated {
        eprintln!("warning: {id} is deprecated: {message}");
    }
    // Names the scripts and stops there. Any remedy offered now either does
    // nothing yet (`trustedDependencies`) or runs a script in place, which
    // writes through hardlinks into the store every project shares.
    if let Some(scripts) = &report.project_scripts_not_run {
        eprintln!(
            "warning: this project's own install scripts were not run (opal does not run \
             install scripts): {}",
            describe_scripts(scripts)
        );
    }
    if !report.scripts_not_run.is_empty() {
        let count = report.scripts_not_run.len();
        eprintln!(
            "warning: install scripts were not run for {count} {} (opal does not run \
             install scripts), so anything they build or download is missing:",
            if count == 1 { "package" } else { "packages" },
        );
        for (id, scripts) in &report.scripts_not_run {
            eprintln!("  {id}: {}", describe_scripts(scripts));
        }
    }
    if let Some(reason) = &report.link.hardlink_fallback {
        eprintln!("warning: {reason}");
        // The library states the fact; naming the remedy needs to know about
        // the environment variable, which is this crate's business.
        eprintln!(
            "note: copying {} files is most of an install's time. Set {}=<dir> to a directory \
             on the same filesystem as this project to restore hardlinking.",
            report.link.files_copied,
            opal_core::cache::CACHE_DIR_ENV,
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// What an install did, in as few lines as say it: a headline, then at most one
/// detail line, and only when it tells the reader something the headline
/// doesn't. That a re-run kept most of the tree is the one worth keeping above
/// the others: it's what converging after a killed install looks like.
fn install_summary(report: &InstallReport, elapsed: Duration) -> Vec<String> {
    let link = &report.link;
    let packages = plural(report.packages, "package", "packages");
    if !report.resolved && report.fetched == 0 && link.added == 0 && link.removed == 0 {
        return vec![format!(
            "{packages} already installed ({})",
            format_duration(elapsed)
        )];
    }

    let mut phases = Vec::new();
    if report.resolved {
        phases.push(format!(
            "resolve {}",
            format_duration(report.timings.resolve)
        ));
    }
    phases.push(format!("fetch {}", format_duration(report.timings.fetch)));
    phases.push(format!("link {}", format_duration(report.timings.link)));
    let mut lines = vec![format!(
        "{packages} installed in {}  ({})",
        format_duration(elapsed),
        phases.join(", ")
    )];

    if link.unchanged > 0 || link.removed > 0 {
        let mut changes = Vec::new();
        if link.added > 0 {
            changes.push(format!("{} added", link.added));
        }
        if link.removed > 0 {
            changes.push(format!("{} removed", link.removed));
        }
        if link.unchanged > 0 {
            changes.push(all_or_some(
                changes.is_empty(),
                link.unchanged,
                "already in place",
            ));
        }
        lines.push(changes.join(", "));
    } else if report.already_stored > 0 {
        lines.push(if report.fetched == 0 {
            all_or_some(true, report.already_stored, "already in the store")
        } else {
            format!(
                "{} downloaded, {} already in the store",
                report.fetched, report.already_stored
            )
        });
    }
    lines
}

/// "all 43 already in place", "43 already in place", or, for a single one,
/// just "already in place".
fn all_or_some(all: bool, count: usize, state: &str) -> String {
    match (all, count) {
        (true, 1) => state.to_string(),
        (true, _) => format!("all {count} {state}"),
        (false, _) => format!("{count} {state}"),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// Milliseconds under a second, tenths of a second under a minute, then
/// minutes and seconds. Truncated, never rounded up, so 59.96s can't print as
/// "60.0s".
fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds == 0 {
        format!("{}ms", duration.as_millis())
    } else if seconds < 60 {
        format!("{seconds}.{}s", duration.subsec_millis() / 100)
    } else {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    }
}

fn platform_skip_summary(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some("skipped 1 optional package built for another platform".to_string()),
        _ => Some(format!(
            "skipped {count} optional packages built for other platforms"
        )),
    }
}

fn upgrade_command(args: UpgradeArgs) -> Result<ExitCode, Failure> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    // The install's own client: the same timeouts, and the same retries on a
    // dropped connection or a 5xx.
    let transport = RetryingTransport::new(HttpTransport::new(), RetryPolicy::default());
    let outcome = upgrade::upgrade(
        &Releases::from_env(),
        &transport,
        args.version.as_deref(),
        &current,
        &std::env::current_exe()?,
        (std::env::consts::OS, std::env::consts::ARCH),
        &mut |step| eprintln!("{step}"),
    )?;
    match outcome {
        Outcome::Upgraded { from, to } => println!("opal {from} → {to}"),
        Outcome::AlreadyLatest(version) => {
            println!("opal {version} is already the latest release");
        }
        Outcome::AlreadyInstalled(version) => println!("opal {version} is already installed"),
        Outcome::NewerThanLatest { current, latest } => {
            println!("opal {current} is newer than the latest release ({latest})");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The scripts in the order npm would have run them. An implied
/// `node-gyp rebuild` only exists when no `install` or `preinstall` does, so
/// it always comes first.
fn describe_scripts(scripts: &UnrunScripts) -> String {
    let mut named: Vec<&str> = scripts.events.clone();
    if scripts.implicit_node_gyp {
        named.insert(0, "install (node-gyp rebuild, implied by binding.gyp)");
    }
    named.join(", ")
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

    // Pruned before the mark, so a record dropped here stops pinning its graph
    // object in the same pass rather than the next one.
    let records_pruned = cache.prune_records(dry_run)?;
    let packuments_pruned = PackumentCache::new(root.path().join("packuments")).prune(
        PACKUMENT_MAX_AGE,
        std::time::SystemTime::now(),
        dry_run,
    );

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
        "records:  {records_pruned} graph, {packuments_pruned} metadata {}",
        if dry_run { "collectable" } else { "pruned" }
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use opal_pm::install::Timings;
    use opal_pm::link::LinkReport;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    struct Run {
        packages: usize,
        resolved: bool,
        fetched: usize,
        already_stored: usize,
        added: usize,
        unchanged: usize,
        removed: usize,
        timings: (u64, u64, u64),
    }

    fn summary(run: Run, elapsed: u64) -> Vec<String> {
        let report = InstallReport {
            packages: run.packages,
            resolved: run.resolved,
            fetched: run.fetched,
            already_stored: run.already_stored,
            link: LinkReport {
                added: run.added,
                unchanged: run.unchanged,
                removed: run.removed,
                ..LinkReport::default()
            },
            timings: Timings {
                resolve: ms(run.timings.0),
                fetch: ms(run.timings.1),
                link: ms(run.timings.2),
            },
            ..InstallReport::default()
        };
        install_summary(&report, ms(elapsed))
    }

    #[test]
    fn test_a_fresh_install_is_one_line() {
        let run = Run {
            packages: 364,
            resolved: true,
            fetched: 359,
            already_stored: 0,
            added: 364,
            unchanged: 0,
            removed: 0,
            timings: (26_900, 85_400, 840),
        };
        assert_eq!(
            summary(run, 113_200),
            ["364 packages installed in 1m53s  (resolve 26.9s, fetch 1m25s, link 840ms)"]
        );
    }

    #[test]
    fn test_a_second_project_says_the_store_had_everything() {
        let run = Run {
            packages: 364,
            resolved: false,
            fetched: 0,
            already_stored: 359,
            added: 364,
            unchanged: 0,
            removed: 0,
            timings: (0, 3, 76),
        };
        assert_eq!(
            summary(run, 92),
            [
                "364 packages installed in 92ms  (fetch 3ms, link 76ms)",
                "all 359 already in the store",
            ]
        );
    }

    #[test]
    fn test_a_rerun_after_a_kill_shows_what_it_kept() {
        let run = Run {
            packages: 68,
            resolved: false,
            fetched: 0,
            already_stored: 66,
            added: 25,
            unchanged: 43,
            removed: 0,
            timings: (0, 3, 52),
        };
        assert_eq!(
            summary(run, 86),
            [
                "68 packages installed in 86ms  (fetch 3ms, link 52ms)",
                "25 added, 43 already in place",
            ]
        );
    }

    #[test]
    fn test_nothing_to_do_is_one_line() {
        let run = Run {
            packages: 68,
            resolved: false,
            fetched: 0,
            already_stored: 66,
            added: 0,
            unchanged: 68,
            removed: 0,
            timings: (0, 1, 3),
        };
        assert_eq!(summary(run, 21), ["68 packages already installed (21ms)"]);
    }

    #[test]
    fn test_a_partial_download_names_both_sources() {
        let run = Run {
            packages: 364,
            resolved: true,
            fetched: 10,
            already_stored: 349,
            added: 364,
            unchanged: 0,
            removed: 0,
            timings: (1_200, 2_300, 700),
        };
        assert_eq!(
            summary(run, 4_300)[1],
            "10 downloaded, 349 already in the store"
        );
    }

    #[test]
    fn test_removing_a_dependency_counts_it() {
        let run = Run {
            packages: 66,
            resolved: true,
            fetched: 0,
            already_stored: 66,
            added: 0,
            unchanged: 66,
            removed: 2,
            timings: (400, 2, 30),
        };
        assert_eq!(summary(run, 450)[1], "2 removed, 66 already in place");
    }

    #[test]
    fn test_one_package_reads_in_the_singular() {
        let run = Run {
            packages: 1,
            resolved: false,
            fetched: 0,
            already_stored: 1,
            added: 0,
            unchanged: 1,
            removed: 0,
            timings: (0, 0, 1),
        };
        assert_eq!(summary(run, 5), ["1 package already installed (5ms)"]);
    }

    #[test]
    fn test_durations_read_at_the_right_precision() {
        assert_eq!(format_duration(ms(0)), "0ms");
        assert_eq!(format_duration(ms(840)), "840ms");
        assert_eq!(format_duration(ms(26_950)), "26.9s");
        assert_eq!(format_duration(ms(59_960)), "59.9s");
        assert_eq!(format_duration(ms(85_400)), "1m25s");
    }
}
