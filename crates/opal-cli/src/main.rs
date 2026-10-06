//! The `opal` binary.
//!
//! Implements: resolving a module graph, installing dependencies, adding and
//! removing them, inspecting the shared cache, and upgrading itself. `run`,
//! `build`, and `test` are not here, because a command that exists and does
//! nothing is worse than one that does not exist. The same goes for `update`
//! and the analysis commands — they arrive with the phase that implements
//! them.

mod progress;
mod style;
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
use opal_pm::edit::{AddRequest, Group};
use opal_pm::gc as package_gc;
use opal_pm::install::{self, Change, InstallOptions, InstallReport, UnrunScripts};
use opal_pm::locks::CacheLock;
use opal_pm::package::PackageStore;
use opal_pm::packuments::PackumentCache;
use opal_pm::projects::ProjectIndex;
use opal_pm::registry::{Freshness, HttpTransport, NpmRegistry, RetryPolicy, RetryingTransport};
use opal_pm::resolve::PackageId;
use opal_pm::semver::Version;
use style::Paint;
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
    /// Install the dependencies in package.json, or add the packages named.
    Install(InstallArgs),
    /// Add dependencies to package.json and install them.
    Add(AddArgs),
    /// Remove dependencies from package.json and from node_modules.
    #[command(visible_alias = "rm", alias = "uninstall")]
    Remove(RemoveArgs),
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

/// Which project, and what an install of it draws on. The same for every
/// command that ends in an install.
#[derive(Args)]
struct ProjectArgs {
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
    /// Resolve from cached registry metadata only; never reach the network.
    #[arg(long)]
    offline: bool,
    /// Use cached registry metadata however old it is, and only fetch what is
    /// missing.
    #[arg(long, conflicts_with = "offline")]
    prefer_offline: bool,
}

/// Where in package.json an added package goes, and how it is written. The
/// `--save-*` spellings are npm's, accepted so a habit carries over.
#[derive(Args)]
struct SaveArgs {
    /// Add to devDependencies.
    #[arg(
        short = 'D',
        long,
        short_alias = 'd',
        alias = "save-dev",
        requires = "packages"
    )]
    dev: bool,
    /// Add to optionalDependencies.
    #[arg(
        short = 'O',
        long,
        alias = "save-optional",
        conflicts_with = "dev",
        requires = "packages"
    )]
    optional: bool,
    /// Save the exact version installed instead of a ^ range on it.
    #[arg(short = 'E', long, alias = "save-exact", requires = "packages")]
    exact: bool,
}

impl SaveArgs {
    fn group(&self) -> Option<Group> {
        match (self.dev, self.optional) {
            (true, _) => Some(Group::Development),
            (_, true) => Some(Group::Optional),
            _ => None,
        }
    }
}

#[derive(Args)]
struct InstallArgs {
    /// Packages to add to package.json first, exactly as `opal add` does.
    /// With none, installs what package.json already lists.
    packages: Vec<String>,
    #[command(flatten)]
    project: ProjectArgs,
    #[command(flatten)]
    save: SaveArgs,
    /// For CI: fail instead of re-resolving when opal.lock is missing or does not
    /// match package.json.
    #[arg(long, conflicts_with = "packages")]
    frozen_lockfile: bool,
}

#[derive(Args)]
struct AddArgs {
    /// What to add: a name, name@version, name@range, name@tag, or
    /// alias@npm:name@range. A bare name means the latest release.
    #[arg(required = true)]
    packages: Vec<String>,
    #[command(flatten)]
    project: ProjectArgs,
    #[command(flatten)]
    save: SaveArgs,
}

#[derive(Args)]
struct RemoveArgs {
    /// Dependencies to remove, by the name package.json lists them under.
    #[arg(required = true)]
    packages: Vec<String>,
    #[command(flatten)]
    project: ProjectArgs,
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
            eprintln!("{} {error}", Paint::for_stderr().red("opal:"));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, Failure> {
    match cli.command {
        Command::Graph(args) => graph(args),
        Command::Install(args) => {
            let change = adding(&args.packages, &args.save)?;
            install_command("install", args.project, args.frozen_lockfile, change)
        }
        Command::Add(args) => {
            let change = adding(&args.packages, &args.save)?;
            install_command("add", args.project, false, change)
        }
        Command::Remove(args) => {
            let change = Change::Remove {
                names: args.packages,
            };
            install_command("remove", args.project, false, Some(change))
        }
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

/// The change `opal add` makes for these arguments, or none when there are
/// no arguments, which is a plain install.
fn adding(packages: &[String], save: &SaveArgs) -> Result<Option<Change>, Failure> {
    if packages.is_empty() {
        return Ok(None);
    }
    let requests = packages
        .iter()
        .map(|argument| AddRequest::parse(argument))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Change::Add {
        requests,
        group: save.group(),
        exact: save.exact,
    }))
}

/// An install, after `change` if there is one. `command` is the name it was
/// run under, for the header.
fn install_command(
    command: &str,
    args: ProjectArgs,
    frozen_lockfile: bool,
    change: Option<Change>,
) -> Result<ExitCode, Failure> {
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
        frozen_lockfile,
        ..InstallOptions::default()
    };

    let paint = Paint::for_stdout();
    let paint_stderr = Paint::for_stderr();
    // On stderr with the progress it introduces: stdout is the result.
    eprintln!("{}", install_header(command, paint_stderr));
    let started = Instant::now();
    let reporter = progress::reporter();
    let report = match &change {
        Some(change) => install::change(
            &root,
            change,
            &registry,
            &store,
            &projects,
            &options,
            reporter.as_ref(),
        ),
        None => install::install(
            &root,
            &registry,
            &store,
            &projects,
            &options,
            reporter.as_ref(),
        ),
    }?;
    let elapsed = started.elapsed();

    // What was asked for by name is listed whether or not the tree changed:
    // a package already installed as something else's dependency is still a
    // new dependency of the project.
    let added = match report.requested.is_empty() {
        true => &report.added_direct,
        false => &report.requested,
    };
    for line in added_lines(added, paint) {
        println!("{line}");
    }
    for name in &report.removed_direct {
        println!("{} {}", paint.red("-"), paint.bold(name));
    }
    for line in install_summary(&report, elapsed, paint) {
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
        println!("{}", paint.dim(line));
    }

    // Warnings go last, after the result, so they are the last thing on screen
    // rather than buried above it.
    let warning = paint_stderr.yellow("warning:");
    for (id, message) in &report.deprecated {
        eprintln!("{warning} {id} is deprecated: {message}");
    }
    // Names the scripts and stops there. Any remedy offered now either does
    // nothing yet (`trustedDependencies`) or runs a script in place, which
    // writes through hardlinks into the store every project shares.
    if let Some(scripts) = &report.project_scripts_not_run {
        eprintln!(
            "{warning} this project's own install scripts were not run (opal does not run \
             install scripts): {}",
            describe_scripts(scripts)
        );
    }
    if !report.scripts_not_run.is_empty() {
        let count = report.scripts_not_run.len();
        eprintln!(
            "{warning} install scripts were not run for {count} {} (opal does not run \
             install scripts), so anything they build or download is missing:",
            if count == 1 { "package" } else { "packages" },
        );
        for (id, scripts) in &report.scripts_not_run {
            eprintln!("  {id}: {}", describe_scripts(scripts));
        }
    }
    if let Some(reason) = &report.link.hardlink_fallback {
        eprintln!("{warning} {reason}");
        // The library states the fact; naming the remedy needs to know about
        // the environment variable, which is this crate's business.
        eprintln!(
            "{} copying {} files is most of an install's time. Set {}=<dir> to a directory \
             on the same filesystem as this project to restore hardlinking.",
            paint_stderr.dim("note:"),
            report.link.files_copied,
            opal_core::cache::CACHE_DIR_ENV,
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn install_header(command: &str, paint: Paint) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let build = match env!("OPAL_COMMIT") {
        "" => format!("v{version}"),
        commit => format!("v{version} ({commit})"),
    };
    format!(
        "{} {}",
        paint.bold(format!("opal {command}")),
        paint.dim(build)
    )
}

/// How many added dependencies get a line of their own. A new project adds
/// all of them at once, and the result has to stay on screen under the list.
const ADDED_SHOWN: usize = 5;

/// The project's own dependencies a run added, one per line, with the rest
/// counted on the last line.
fn added_lines(added: &[(String, PackageId)], paint: Paint) -> Vec<String> {
    let mut lines: Vec<String> = added
        .iter()
        .take(ADDED_SHOWN)
        .map(|(name, id)| {
            format!(
                "{} {}{}",
                paint.green("+"),
                paint.bold(name),
                paint.dim(format!("@{}", id.version))
            )
        })
        .collect();
    let hidden = added.len().saturating_sub(ADDED_SHOWN);
    if hidden > 0
        && let Some(last) = lines.last_mut()
    {
        last.push(' ');
        last.push_str(&paint.dim(format!("(+ {hidden} more)")));
    }
    lines
}

/// A package count with its number in green, the colour of a `+` line.
fn counted(count: usize, paint: Paint) -> String {
    let noun = if count == 1 { "package" } else { "packages" };
    format!("{} {noun}", paint.green(count))
}

/// How long the run took, with the brackets dimmed so the number stands out.
fn bracketed(elapsed: Duration, paint: Paint) -> String {
    format!(
        "{}{}{}",
        paint.dim("["),
        paint.bold(format_duration(elapsed)),
        paint.dim("]")
    )
}

/// What an install did, in as few lines as say it: a headline, then at most one
/// detail line, and only when it tells the reader something the headline
/// doesn't. That a re-run kept most of the tree is the one worth keeping above
/// the others: it's what converging after a killed install looks like.
fn install_summary(report: &InstallReport, elapsed: Duration, paint: Paint) -> Vec<String> {
    let link = &report.link;
    let packages = counted(report.packages, paint);
    let took = bracketed(elapsed, paint);
    if !report.resolved && report.fetched == 0 && link.added == 0 && link.removed == 0 {
        return vec![format!("{packages} already installed {took}")];
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
        "{packages} installed {took}  {}",
        paint.dim(format!("({})", phases.join(", ")))
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
        lines.push(paint.dim(changes.join(", ")));
    } else if report.already_stored > 0 {
        lines.push(paint.dim(if report.fetched == 0 {
            all_or_some(true, report.already_stored, "already in the store")
        } else {
            format!(
                "{} downloaded, {} already in the store",
                report.fetched, report.already_stored
            )
        }));
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
        install_summary(&report, ms(elapsed), Paint::plain())
    }

    fn added(names: &[&str]) -> Vec<(String, PackageId)> {
        names
            .iter()
            .map(|name| {
                let version = Version::parse("1.2.3").expect("a version");
                (name.to_string(), PackageId::new(name.to_string(), version))
            })
            .collect()
    }

    #[test]
    fn test_nothing_added_lists_nothing() {
        assert!(added_lines(&[], Paint::plain()).is_empty());
    }

    #[test]
    fn test_up_to_five_added_dependencies_each_get_a_line() {
        assert_eq!(
            added_lines(&added(&["a", "b", "c", "d", "e"]), Paint::plain()),
            [
                "+ a@1.2.3",
                "+ b@1.2.3",
                "+ c@1.2.3",
                "+ d@1.2.3",
                "+ e@1.2.3"
            ]
        );
    }

    #[test]
    fn test_added_dependencies_past_the_fifth_are_counted() {
        assert_eq!(
            added_lines(&added(&["a", "b", "c", "d", "e", "f", "g"]), Paint::plain()).last(),
            Some(&"+ e@1.2.3 (+ 2 more)".to_string())
        );
    }

    #[test]
    fn test_on_a_terminal_the_plus_is_green_and_the_version_dim() {
        assert_eq!(
            added_lines(&added(&["a"]), Paint::coloured()),
            ["\x1b[32m+\x1b[0m \x1b[1ma\x1b[0m\x1b[2m@1.2.3\x1b[0m"]
        );
    }

    #[test]
    fn test_colour_changes_no_wording() {
        let report = InstallReport {
            packages: 364,
            resolved: true,
            fetched: 359,
            link: LinkReport {
                added: 364,
                ..LinkReport::default()
            },
            ..InstallReport::default()
        };
        let strip = |line: &String| {
            let mut plain = String::new();
            let mut rest = line.as_str();
            while let Some(start) = rest.find('\x1b') {
                plain.push_str(&rest[..start]);
                let end = rest[start..]
                    .find('m')
                    .expect("an unterminated escape code");
                rest = &rest[start + end + 1..];
            }
            plain.push_str(rest);
            plain
        };
        let coloured = install_summary(&report, ms(1_200), Paint::coloured());
        assert!(coloured[0].contains('\x1b'), "{coloured:?}");
        assert_eq!(
            coloured.iter().map(strip).collect::<Vec<_>>(),
            install_summary(&report, ms(1_200), Paint::plain())
        );
    }

    #[test]
    fn test_an_alias_is_listed_under_the_name_the_project_requires() {
        let version = Version::parse("4.2.3").expect("a version");
        let aliased = [(
            "width-cjs".to_string(),
            PackageId::new("width".to_string(), version),
        )];
        assert_eq!(added_lines(&aliased, Paint::plain()), ["+ width-cjs@4.2.3"]);
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
            ["364 packages installed [1m53s]  (resolve 26.9s, fetch 1m25s, link 840ms)"]
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
                "364 packages installed [92ms]  (fetch 3ms, link 76ms)",
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
                "68 packages installed [86ms]  (fetch 3ms, link 52ms)",
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
        assert_eq!(summary(run, 21), ["68 packages already installed [21ms]"]);
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
        assert_eq!(summary(run, 5), ["1 package already installed [5ms]"]);
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
