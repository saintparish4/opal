//! Writes one file into a CAS, and nothing else.
//!
//! `tests/cas-crash-safety.rs` runs this with `OPAL_INTERNAL_FAULT_INJECT` set
//! so it parks inside the write, then SIGKILLs it. It exists as a separate
//! process because that is the only way to test a kill that runs no destructor,
//! flushes no buffer, and cleans up nothing

use std::path::Path;
use std::process::ExitCode;

use opal_core::cas::Cas;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [cache_root, payload] = arguments.as_slice() else {
        eprintln!("usage: cas-crash-probe <cas-root> <payload-file>");
        return ExitCode::from(2);
    };

    let store = match Cas::open(cache_root) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("open: {error}");
            return ExitCode::FAILURE;
        }
    };

    match store.put_file(Path::new(payload)) {
        Ok(hash) => {
            println!("{hash}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("put: {error}");
            ExitCode::FAILURE
        }
    }
}
