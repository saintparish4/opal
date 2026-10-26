//! Runs generated module graphs unbundled and bundled under Node, for as
//! long as asked, on every core, and saves each divergent program.
//!
//! ```text
//! cargo run --release -p opal-bundler --features oracle --example generated-graphs -- \
//!     --seconds 3600 --jobs 6 --first-seed 0 --failures target/generated-failures
//! ```
//!
//! It prints the seeds run, the verdicts, and the refusal codes, and exits
//! non-zero if any program diverged. A seed is a reproducible program:
//! `--first-seed N --cases 1` replays one.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use opal_bundler::generate::{Verdict, generate, run_case};
use opal_bundler::oracle::Node;

fn main() {
    let mut seconds = 60u64;
    let mut jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut first_seed = 0u64;
    let mut cases: Option<u64> = None;
    let mut failures = PathBuf::from("target/generated-failures");
    let mut save_refused: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value after the flag");
        match arg.as_str() {
            "--seconds" => seconds = value().parse().unwrap(),
            "--jobs" => jobs = value().parse().unwrap(),
            "--first-seed" => first_seed = value().parse().unwrap(),
            "--cases" => cases = Some(value().parse().unwrap()),
            "--failures" => failures = PathBuf::from(value()),
            "--save-refused" => save_refused = Some(PathBuf::from(value())),
            other => panic!("unknown flag {other}"),
        }
    }
    let node = Node::default();
    eprintln!("node {}", node.version().unwrap_or_default());
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let next = AtomicU64::new(first_seed);
    let stop = AtomicBool::new(false);
    let tally = Mutex::new((
        0u64,
        0u64,
        0u64,
        std::collections::BTreeMap::<String, u64>::new(),
    ));
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                let work = tempfile::tempdir().unwrap();
                loop {
                    if stop.load(Ordering::Relaxed) || Instant::now() > deadline {
                        break;
                    }
                    let seed = next.fetch_add(1, Ordering::Relaxed);
                    if cases.is_some_and(|cases| seed >= first_seed + cases) {
                        break;
                    }
                    let case = generate(seed);
                    let verdict = run_case(&case, work.path(), &node);
                    let mut tally = tally.lock().unwrap();
                    match &verdict {
                        Verdict::Equivalent => tally.0 += 1,
                        Verdict::Refused(code) => {
                            tally.1 += 1;
                            *tally.3.entry(code.clone()).or_default() += 1;
                        }
                        Verdict::Conservative(code, message) => {
                            tally.1 += 1;
                            *tally.3.entry(format!("{code} (Node runs it)")).or_default() += 1;
                            if let Some(dir) = &save_refused {
                                let dir = dir.join(format!("seed-{seed}"));
                                let _ = case.write(&dir);
                                let _ = std::fs::write(dir.join("REFUSED.txt"), message);
                            }
                        }
                        Verdict::Diverged(detail) => {
                            tally.2 += 1;
                            let dir = failures.join(format!("seed-{seed}"));
                            let _ = case.write(&dir);
                            let _ = std::fs::write(dir.join("DIVERGED.txt"), detail);
                            eprintln!("seed {seed} diverged:\n{detail}");
                        }
                    }
                }
            });
        }
    });
    let (equivalent, refused, diverged, codes) = tally.into_inner().unwrap();
    println!(
        "{} programs in {:.0?}: {equivalent} equivalent, {refused} refused {codes:?}, {diverged} diverged",
        equivalent + refused + diverged,
        started.elapsed()
    );
    if diverged > 0 {
        std::process::exit(1);
    }
}
