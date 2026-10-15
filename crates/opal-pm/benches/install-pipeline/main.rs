//! `opal install`, timed.
//!
//! Per `testing_strategy.md` §7 this tracks numbers, it does not gate on them:
//! nothing here has a threshold, and the exit code never depends on a
//! measurement. A flaky perf gate teaches people to ignore red CI.
//!
//! "Install time" is four different numbers, and collapsing them is how a
//! ten-minute `create-next-app` install once looked ordinary. Each scenario
//! isolates one stage by varying only what is already on disk:
//!
//! | Scenario  | Store | `opal.lock` | `node_modules` | Measures                  |
//! | --------- | ----- | ----------- | -------------- | ------------------------- |
//! | `cold`    | empty | absent      | absent         | the whole pipeline        |
//! | `resolve` | warm  | absent      | present        | packument fetch and parse |
//! | `link`    | warm  | present     | absent         | CAS to `node_modules`     |
//! | `noop`    | warm  | present     | present        | the reconciler finding    |
//! |           |       |             |                | nothing to do             |
//!
//! Everything runs offline against a fixture registry over `file://`, so the
//! numbers are reproducible and the only network cost in them is the one
//! `--rtt-ms` puts there deliberately. Run with a non-zero RTT to price the
//! round trips; run with the default zero to measure local work.
//!
//! Name the target: `cargo bench` otherwise passes these options to every
//! test binary in the package too, and libtest rejects them.
//!
//! ```text
//! cargo bench -p opal-pm --bench install-pipeline -- --rtt-ms 25
//! cargo bench -p opal-pm --bench install-pipeline -- --packages 440 --versions 60
//! cargo bench -p opal-pm --bench install-pipeline -- --json > target/bench.json
//! ```

mod wire;
mod workload;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use opal_core::cache::CacheRoot;
use opal_pm::fixtures::write_project;
use opal_pm::install::{self, InstallOptions, InstallReport};
use opal_pm::link;
use opal_pm::lockfile;
use opal_pm::package::PackageStore;
use opal_pm::packuments::PackumentCache;
use opal_pm::progress::Silent;
use opal_pm::projects::ProjectIndex;
use opal_pm::registry::{HttpTransport, NpmRegistry, Registry};

use wire::{Counters, Meter, MeteredTransport};
use workload::{Shape, Workload};

const USAGE: &str = "\
opal install benchmark

usage: cargo bench -p opal-pm --bench install-pipeline -- [options]

  --packages N     distinct packages in the fixture registry (default 64)
  --versions N     published versions per package (default 12)
  --files N        files in each package's newest version (default 6)
  --fanout N       dependencies each package declares (default 2)
  --roots N        dependencies the project declares (default 8)
  --iterations N   timed runs per scenario (default 5)
  --rtt-ms N       simulated per-request round-trip latency (default 0)
  --bandwidth-mbit N
                   simulated link speed, shared by every request (default:
                   unlimited, so a download costs a round trip and nothing more)
  --workload NAME  default; scaffold, the Next.js scaffold's package count,
                   file count, and bytes (about 155 MB of tarballs), to pair
                   with --rtt-ms and --bandwidth-mbit; wide-N, N packages the
                   project depends on directly; deep-N, a chain N long;
                   dedup-heavy, about 10,000 edges onto 1,200 packages;
                   diamond and conflicting-ranges, which ask where versions
                   go rather than how fast. Give it before any of --packages,
                   --files and the rest, which then adjust it
  --scenario LIST  comma-separated subset of cold,resolve,link,noop
  --json           emit one JSON record instead of a report
  --no-packument-cache
                   resolve without the on-disk metadata cache, for before/after
  --help
";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scenario {
    Cold,
    Resolve,
    Link,
    NoOp,
}

impl Scenario {
    const ALL: [Self; 4] = [Self::Cold, Self::Resolve, Self::Link, Self::NoOp];

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "cold" => Self::Cold,
            "resolve" => Self::Resolve,
            "link" => Self::Link,
            "noop" => Self::NoOp,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Resolve => "resolve",
            Self::Link => "link",
            Self::NoOp => "noop",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::Cold => "empty store, no lockfile — resolve, fetch, ingest, link",
            Self::Resolve => "warm store, lockfile deleted — re-resolve against the registry",
            Self::Link => "warm store and lockfile, node_modules deleted — materialize only",
            Self::NoOp => "everything present — the reconciler finds nothing to do",
        }
    }

    /// Whether the store may carry over from a previous run.
    fn warm(self) -> bool {
        self != Self::Cold
    }
}

struct Options {
    shape: Shape,
    iterations: usize,
    rtt: Duration,
    /// Bytes per second through one link that every request shares.
    bandwidth: Option<u64>,
    workload: String,
    scenarios: Vec<Scenario>,
    json: bool,
    /// Off measures what an install cost before metadata was kept between
    /// runs. Two runs of one binary, minutes apart on one machine, is a far
    /// better before/after than two runs of two binaries.
    packument_cache: bool,
    /// Set in the child process that measures one install's peak memory: the
    /// sandbox it installs into, and the registry it installs from.
    peak_of: Option<(PathBuf, String)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            shape: Shape::default(),
            iterations: 5,
            rtt: Duration::ZERO,
            bandwidth: None,
            workload: "default".to_string(),
            scenarios: Scenario::ALL.to_vec(),
            json: false,
            packument_cache: true,
            peak_of: None,
        }
    }
}

fn main() -> ExitCode {
    let options = match parse_arguments(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("bench: {message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    if let Some((sandbox, registry)) = &options.peak_of {
        return peak_of(sandbox, registry, &options);
    }

    let started = Instant::now();
    let workload = Workload::generate(options.shape);
    let setup = started.elapsed();

    let measurements: Vec<Measurement> = options
        .scenarios
        .iter()
        .map(|scenario| measure(*scenario, &workload, &options))
        .collect();

    if options.json {
        println!("{}", json(&workload, &options, setup, &measurements));
    } else {
        report(&workload, &options, setup, &measurements);
    }
    ExitCode::SUCCESS
}

fn parse_arguments(arguments: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        // `cargo bench` passes libtest's own flags through to a `harness =
        // false` target. Ignoring the one it always sends keeps a bare
        // `cargo bench` working.
        if argument == "--bench" {
            continue;
        }
        if argument == "--help" || argument == "-h" {
            return Ok(None);
        }
        if argument == "--json" {
            options.json = true;
            continue;
        }
        if argument == "--no-packument-cache" {
            options.packument_cache = false;
            continue;
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{argument} needs a value"))?;
        let number = || {
            value
                .parse::<usize>()
                .map_err(|_| format!("{argument} needs a number, got {value:?}"))
        };
        match argument.as_str() {
            "--packages" => options.shape.packages = number()?,
            "--versions" => options.shape.versions = number()?,
            "--files" => options.shape.files = number()?,
            "--fanout" => options.shape.fanout = number()?,
            "--roots" => options.shape.roots = number()?,
            "--iterations" => options.iterations = number()?,
            "--rtt-ms" => options.rtt = Duration::from_millis(number()? as u64),
            "--bandwidth-mbit" => {
                let megabits = number()?;
                if megabits == 0 {
                    return Err("--bandwidth-mbit must be at least 1".to_string());
                }
                options.bandwidth = Some(megabits as u64 * 125_000);
            }
            "--workload" => {
                options.shape =
                    Shape::named(&value).ok_or_else(|| format!("unknown workload {value:?}"))?;
                options.workload = value;
            }
            // Internal: how the benchmark asks a copy of itself for one
            // install's peak memory. See `peak_rss`.
            "--peak-of" => {
                let (sandbox, registry) = value.split_once('\n').ok_or_else(|| {
                    format!("--peak-of needs a sandbox and a registry, got {value:?}")
                })?;
                options.peak_of = Some((PathBuf::from(sandbox), registry.to_string()));
            }
            "--scenario" => {
                options.scenarios = value
                    .split(',')
                    .map(|name| {
                        Scenario::parse(name.trim())
                            .ok_or_else(|| format!("unknown scenario {name:?}"))
                    })
                    .collect::<Result<_, _>>()?;
            }
            _ => return Err(format!("unknown option {argument:?}")),
        }
    }

    if options.shape.packages == 0 || options.shape.versions == 0 {
        return Err("--packages and --versions must be at least 1".to_string());
    }
    if options.iterations == 0 {
        return Err("--iterations must be at least 1".to_string());
    }
    Ok(Some(options))
}

/// A project, a cache, and a store, all disposable.
struct Sandbox {
    /// Absent in the child that measures peak memory: the sandbox is the
    /// parent's, and so is deleting it.
    _directory: Option<tempfile::TempDir>,
    root: PathBuf,
    project: PathBuf,
    cache: CacheRoot,
    store: PackageStore,
    projects: ProjectIndex,
}

impl Sandbox {
    fn new(workload: &Workload) -> Self {
        let directory = tempfile::tempdir().expect("temp dir");
        write_project(&directory.path().join("project"), workload.manifest.clone());
        let mut sandbox = Self::open(directory.path());
        sandbox._directory = Some(directory);
        sandbox
    }

    fn open(root: &std::path::Path) -> Self {
        let cache = CacheRoot::at(root.join("cache"));
        let store = PackageStore::open(cache.open_cas().expect("cas"), cache.path())
            .expect("package store");
        let projects = ProjectIndex::new(cache.path().join("projects")).expect("project index");
        Self {
            _directory: None,
            root: root.to_path_buf(),
            project: root.join("project"),
            cache,
            store,
            projects,
        }
    }

    /// The client an `opal install` would build: metadata cached under this
    /// sandbox's cache root, unless the run is measuring what life was like
    /// without it.
    fn registry(
        &self,
        registry_url: &str,
        options: &Options,
        rtt: Duration,
        bandwidth: Option<u64>,
    ) -> Client {
        let counters = Arc::new(Counters::default());
        let transport =
            MeteredTransport::new(HttpTransport::new(), rtt, bandwidth, Arc::clone(&counters));
        let mut registry = NpmRegistry::with_transport(registry_url, Box::new(transport));
        if options.packument_cache {
            registry = registry
                .with_packument_cache(PackumentCache::new(self.cache.path().join("packuments")));
        }
        Client {
            registry,
            counters,
            rtt,
            bandwidth,
        }
    }

    fn install(&self, registry: &dyn Registry) -> InstallReport {
        install::install(
            &self.project,
            registry,
            &self.store,
            &self.projects,
            &InstallOptions::default(),
            &Silent,
        )
        .expect("install")
    }

    fn prepare(&self, scenario: Scenario) {
        if matches!(scenario, Scenario::Resolve) {
            std::fs::remove_file(lockfile::path_in(&self.project)).expect("remove lockfile");
        }
        if matches!(scenario, Scenario::Link) {
            std::fs::remove_dir_all(self.project.join(link::NODE_MODULES))
                .expect("remove node_modules");
        }
    }
}

/// A registry client and the counters underneath it.
struct Client {
    registry: NpmRegistry,
    counters: Arc<Counters>,
    rtt: Duration,
    bandwidth: Option<u64>,
}

impl Client {
    fn meter(&self) -> Meter {
        self.counters.meter(self.rtt, self.bandwidth)
    }
}

struct Measurement {
    scenario: Scenario,
    /// One per iteration, sorted.
    wall: Vec<Duration>,
    report: InstallReport,
    meter: Meter,
    /// Of one more install, in its own process. `None` where it couldn't be
    /// measured.
    peak_rss: Option<u64>,
}

impl Measurement {
    fn median(&self) -> Duration {
        self.wall[self.wall.len() / 2]
    }

    fn min(&self) -> Duration {
        self.wall[0]
    }

    fn max(&self) -> Duration {
        self.wall[self.wall.len() - 1]
    }
}

fn measure(scenario: Scenario, workload: &Workload, options: &Options) -> Measurement {
    // A warm scenario installs once, untimed and without simulated latency, and
    // then reuses that store and cache across every iteration — which is what
    // makes it warm. A cold one gets a new one each time.
    let warm = scenario.warm().then(|| {
        let sandbox = Sandbox::new(workload);
        // Untimed, with no latency and no bandwidth limit, but through the
        // same client, so whatever a
        // real first install would leave behind — a warm store, and now warm
        // metadata — is what the timed runs start from.
        let client = sandbox.registry(workload.registry_url(), options, Duration::ZERO, None);
        sandbox.install(&client.registry);
        sandbox
    });

    let mut wall = Vec::with_capacity(options.iterations);
    let mut last = None;

    for _ in 0..options.iterations {
        let cold;
        let sandbox = match &warm {
            Some(sandbox) => sandbox,
            None => {
                cold = Sandbox::new(workload);
                &cold
            }
        };
        sandbox.prepare(scenario);

        // A fresh client per iteration, because the in-process packument cache
        // is per-process and a reused one would hand the second iteration a
        // warm memory no real `opal install` ever starts with. What *does*
        // carry over is the on-disk cache, which is the point.
        let client = sandbox.registry(
            workload.registry_url(),
            options,
            options.rtt,
            options.bandwidth,
        );
        let started = Instant::now();
        let report = sandbox.install(&client.registry);
        wall.push(started.elapsed());
        last = Some((report, client.meter()));
    }

    wall.sort_unstable();
    let (report, meter) = last.expect("at least one iteration");

    let cold;
    let sandbox = match &warm {
        Some(sandbox) => sandbox,
        None => {
            cold = Sandbox::new(workload);
            &cold
        }
    };
    sandbox.prepare(scenario);
    let peak_rss = peak_rss(sandbox, workload, options);

    Measurement {
        scenario,
        wall,
        report,
        meter,
        peak_rss,
    }
}

/// The peak memory of one install in `sandbox`, prepared for its scenario.
///
/// Measured by a copy of this benchmark that does that install and nothing
/// else. In this process the peak would be the highest of everything it has
/// done, generating the workload included, since `ru_maxrss` never comes
/// down; resetting it is Linux-only (`/proc/self/clear_refs`), and memory
/// this process freed but kept would hide what the install allocated.
fn peak_rss(sandbox: &Sandbox, workload: &Workload, options: &Options) -> Option<u64> {
    let mut command = std::process::Command::new(std::env::current_exe().ok()?);
    command
        .arg("--peak-of")
        .arg(format!(
            "{}\n{}",
            sandbox.root.display(),
            workload.registry_url()
        ))
        .arg("--rtt-ms")
        .arg(options.rtt.as_millis().to_string());
    if let Some(rate) = options.bandwidth {
        command
            .arg("--bandwidth-mbit")
            .arg((rate / 125_000).to_string());
    }
    if !options.packument_cache {
        command.arg("--no-packument-cache");
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        eprintln!(
            "bench: peak memory not measured: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// The child's half of [`peak_rss`]: install, then print this process's peak.
fn peak_of(sandbox: &std::path::Path, registry_url: &str, options: &Options) -> ExitCode {
    let sandbox = Sandbox::open(sandbox);
    let client = sandbox.registry(registry_url, options, options.rtt, options.bandwidth);
    sandbox.install(&client.registry);

    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `getrusage` fills the whole struct for `RUSAGE_SELF` and
    // touches nothing else; the zeroed value is a valid `rusage` regardless.
    let usage = unsafe {
        libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr());
        usage.assume_init()
    };
    // Kilobytes on Linux, bytes on macOS.
    let unit = if cfg!(target_os = "macos") { 1 } else { 1024 };
    println!("{}", usage.ru_maxrss as u64 * unit);
    ExitCode::SUCCESS
}

fn report(workload: &Workload, options: &Options, setup: Duration, measurements: &[Measurement]) {
    let shape = &workload.shape;
    println!("opal install — Phase 1 benchmark\n");
    println!(
        "workload   {}: {} packages, {} versions each, {} files per package",
        options.workload, shape.packages, shape.versions, shape.files,
    );
    println!(
        "tree       {} placements under node_modules",
        measurements
            .first()
            .map_or(0, |measurement| measurement.report.packages),
    );
    println!(
        "registry   file:// fixture, {} of packument JSON, {} of tarballs, generated in {}",
        bytes(workload.packument_bytes),
        bytes(workload.tarball_bytes),
        duration(setup),
    );
    println!(
        "latency    {} simulated per round-trip",
        duration(options.rtt)
    );
    match options.bandwidth {
        Some(rate) => println!(
            "bandwidth  {} Mbit/s simulated, shared by every request",
            rate / 125_000
        ),
        None => println!("bandwidth  unlimited: a download costs a round trip and nothing more"),
    }
    println!(
        "samples    {} iterations per scenario\n",
        options.iterations
    );

    for measurement in measurements {
        let meter = &measurement.meter;
        let link = &measurement.report.link;
        println!(
            "{}  — {}",
            measurement.scenario.name(),
            measurement.scenario.summary()
        );
        println!(
            "  wall       {}  (min {}, max {})",
            duration(measurement.median()),
            duration(measurement.min()),
            duration(measurement.max()),
        );
        let timings = &measurement.report.timings;
        println!(
            "  phases     resolve {}, fetch {}, link {} (last iteration)",
            duration(timings.resolve),
            duration(timings.fetch),
            duration(timings.link),
        );
        println!(
            "  registry   {} packument round-trips ({} revalidated), {} tarballs, {}",
            meter.packuments.round_trips,
            meter.packuments.revalidations,
            meter.tarballs.round_trips,
            bytes(meter.tarball_bytes),
        );
        if !meter.stalled().is_zero() {
            println!(
                "  serialized {} is what {} round-trips cost one at a time ({}% of wall)",
                duration(meter.stalled()),
                meter.packuments.round_trips + meter.tarballs.round_trips,
                percent(meter.stalled(), measurement.median()),
            );
        }
        if !meter.transfer().is_zero() {
            println!(
                "  transfer   {} to move {} at this bandwidth, however many requests overlap ({}% of wall)",
                duration(meter.transfer()),
                bytes(meter.packument_bytes + meter.tarball_bytes),
                percent(meter.transfer(), measurement.median()),
            );
        }
        println!(
            "  store      {} fetched, {} already present",
            measurement.report.fetched, measurement.report.already_stored
        );
        if let Some(peak) = measurement.peak_rss {
            println!(
                "  memory     {} peak RSS, one more install in its own process",
                bytes(peak)
            );
        }
        println!(
            "  link       {} added, {} unchanged, {} removed ({} hardlinked, {} copied, {} bins)",
            link.added,
            link.unchanged,
            link.removed,
            link.files_linked,
            link.files_copied,
            link.bins
        );
        // A cross-device project root — `/mnt/c` under WSL2, one of v1's own
        // targets — fails every `hard_link` and copies instead. It is not an
        // error, and it is the difference between a fast run and a slow one.
        if link.files_copied > 0 && link.files_linked == 0 && link.added > 0 {
            println!("  note       every file was copied: hardlinks are unavailable here");
        }
        println!();
    }
}

fn json(
    workload: &Workload,
    options: &Options,
    setup: Duration,
    measurements: &[Measurement],
) -> String {
    let scenarios: Vec<serde_json::Value> = measurements
        .iter()
        .map(|measurement| {
            let meter = &measurement.meter;
            let link = &measurement.report.link;
            serde_json::json!({
                "scenario": measurement.scenario.name(),
                "wall_ms": {
                    "median": millis(measurement.median()),
                    "min": millis(measurement.min()),
                    "max": millis(measurement.max()),
                },
                "phases_ms": {
                    "resolve": millis(measurement.report.timings.resolve),
                    "fetch": millis(measurement.report.timings.fetch),
                    "link": millis(measurement.report.timings.link),
                },
                "stalled_ms": millis(meter.stalled()),
                "transfer_ms": millis(meter.transfer()),
                "packument_bytes": meter.packument_bytes,
                "packument_round_trips": meter.packuments.round_trips,
                "packument_revalidations": meter.packuments.revalidations,
                "tarball_round_trips": meter.tarballs.round_trips,
                "tarball_bytes": meter.tarball_bytes,
                "packages": measurement.report.packages,
                "peak_rss_bytes": measurement.peak_rss,
                "fetched": measurement.report.fetched,
                "already_stored": measurement.report.already_stored,
                "link": {
                    "added": link.added,
                    "unchanged": link.unchanged,
                    "removed": link.removed,
                    "files_linked": link.files_linked,
                    "files_copied": link.files_copied,
                    "bins": link.bins,
                },
            })
        })
        .collect();

    serde_json::json!({
        "commit": commit(),
        "workload": {
            "name": options.workload,
            "file_bytes": workload.shape.file_bytes,
            "heavy_every": workload.shape.heavy_every,
            "heavy_bytes": workload.shape.heavy_bytes,
            "dense": workload.shape.dense,
            "packages": workload.shape.packages,
            "versions": workload.shape.versions,
            "files": workload.shape.files,
            "fanout": workload.shape.fanout,
            "roots": workload.shape.roots,
            "packument_bytes": workload.packument_bytes,
            "tarball_bytes": workload.tarball_bytes,
            "setup_ms": millis(setup),
        },
        "iterations": options.iterations,
        "rtt_ms": millis(options.rtt),
        "bandwidth_mbit": options.bandwidth.map(|rate| rate / 125_000),
        "scenarios": scenarios,
    })
    .to_string()
}

/// The commit measured, so an artifact can be tied to the build it timed.
///
/// CI names the one it checked out. Locally it's `HEAD`, marked `-dirty` when
/// the tree has changes, since then `HEAD` alone is not what was measured.
fn commit() -> Option<String> {
    if let Ok(sha) = std::env::var("GITHUB_SHA")
        && !sha.is_empty()
    {
        return Some(sha);
    }
    let git = |arguments: &[&str]| std::process::Command::new("git").args(arguments).output();
    let head = git(&["rev-parse", "HEAD"])
        .ok()
        .filter(|output| output.status.success())?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let dirty = git(&["diff", "--quiet", "HEAD"]).is_ok_and(|output| !output.status.success());
    Some(if dirty { format!("{head}-dirty") } else { head })
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn duration(duration: Duration) -> String {
    let ms = millis(duration);
    if ms >= 1000.0 {
        format!("{:.3}s", ms / 1000.0)
    } else {
        format!("{ms:.1}ms")
    }
}

fn percent(part: Duration, whole: Duration) -> String {
    if whole.is_zero() {
        return "0".to_string();
    }
    format!("{:.0}", 100.0 * part.as_secs_f64() / whole.as_secs_f64())
}

fn bytes(count: u64) -> String {
    let kib = count as f64 / 1024.0;
    if kib >= 1024.0 {
        format!("{:.1} MiB", kib / 1024.0)
    } else {
        format!("{kib:.0} KiB")
    }
}
