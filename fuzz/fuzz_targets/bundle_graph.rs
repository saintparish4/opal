//! Generated module graphs, from the fuzzer's bytes, run unbundled and
//! bundled under Node: a divergence is a crash. Needs `node` on PATH (or
//! OPAL_NODE). The generator is crates/opal-bundler/src/generate.rs; the
//! input's first eight bytes are the seed, so a crash is a seed to replay.

#![no_main]

use libfuzzer_sys::fuzz_target;
use opal_bundler::generate::{Verdict, generate, run_case};
use opal_bundler::oracle::Node;

fuzz_target!(|data: &[u8]| {
    let mut seed = [0u8; 8];
    for (slot, byte) in seed.iter_mut().zip(data) {
        *slot = *byte;
    }
    let seed = u64::from_le_bytes(seed);
    let work = tempfile::tempdir().unwrap();
    if let Verdict::Diverged(detail) = run_case(&generate(seed), work.path(), &Node::default()) {
        panic!("seed {seed} diverged:\n{detail}");
    }
});
