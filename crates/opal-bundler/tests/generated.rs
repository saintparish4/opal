//! Generated module graphs (src/generate.rs), run unbundled and bundled
//! under Node. The long runs are `examples/generated-graphs.rs` and the
//! `bundle_graph` fuzz target; this is the fixed set CI runs.
//!
//! ```text
//! cargo test -p opal-bundler --test generated -- --ignored --nocapture
//! ```
//!
//! `OPAL_GENERATED_CASES` sets how many seeds (default 400).

use opal_bundler::generate::{Verdict, generate, run_case};
use opal_bundler::oracle::Node;

#[test]
#[ignore = "needs node; run with --ignored"]
fn test_generated_graphs_bundle_equivalently() {
    let cases: u64 = std::env::var("OPAL_GENERATED_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    let node = Node::default();
    let next = std::sync::atomic::AtomicU64::new(0);
    let diverged = std::sync::Mutex::new(Vec::new());
    let counts = std::sync::Mutex::new((0u64, 0u64));
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                let work = tempfile::tempdir().unwrap();
                loop {
                    let seed = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if seed >= cases {
                        break;
                    }
                    match run_case(&generate(seed), work.path(), &node) {
                        Verdict::Equivalent => counts.lock().unwrap().0 += 1,
                        Verdict::Refused(_) | Verdict::Conservative(..) => {
                            counts.lock().unwrap().1 += 1
                        }
                        Verdict::Diverged(detail) => diverged.lock().unwrap().push((seed, detail)),
                    }
                }
            });
        }
    });
    let (equivalent, refused) = counts.into_inner().unwrap();
    let diverged = diverged.into_inner().unwrap();
    eprintln!(
        "{cases} programs: {equivalent} equivalent, {refused} refused, {} diverged",
        diverged.len()
    );
    for (seed, detail) in diverged.iter().take(5) {
        eprintln!("seed {seed}:\n{detail}");
    }
    assert!(
        diverged.is_empty(),
        "replay one with: cargo run --release -p opal-bundler --features oracle --example generated-graphs -- --first-seed <seed> --cases 1"
    );
}
