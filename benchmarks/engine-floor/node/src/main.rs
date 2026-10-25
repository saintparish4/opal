//! Engine-floor prototype B: opal as a Rust orchestrator over Node child
//! processes. std only.
//!
//! ```text
//! engine-node run FILE
//! engine-node [--snapshot BLOB] test [--isolate MODE] FILE...
//! ```
//!
//! `--isolate`:
//!
//! - `shared`: one Node process, one context for every file.
//! - `context`: one Node process, a fresh `vm` context per file (Jest's model).
//! - `process`: one Node process per file, as many at once as there are CPUs
//!   (node --test's model, and Jest's with its worker pool).
//!
//! `--snapshot` starts each Node from a user-land startup snapshot built from
//! `driver.cjs` (`node --snapshot-blob BLOB --build-snapshot driver.cjs`), so
//! the harness is deserialized instead of evaluated.

use std::io::Write as _;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

const DRIVER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/driver.cjs");

/// One file's captured stdout and whether its Node exited 0.
type FileResult = Option<(Vec<u8>, bool)>;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let snapshot = take_flag(&mut args, "--snapshot");
    let isolate = take_flag(&mut args, "--isolate").unwrap_or_else(|| "shared".into());
    let code = match args.first().map(String::as_str) {
        Some("run") => code_of(
            Command::new("node")
                .arg(&args[1])
                .status()
                .expect("spawn node"),
        ),
        Some("test") => test(snapshot.as_deref(), &isolate, &args[1..]),
        _ => {
            eprintln!(
                "usage: engine-node [--snapshot BLOB] (run FILE | test [--isolate shared|context|process] FILE...)"
            );
            2
        }
    };
    std::process::exit(code);
}

fn take_flag(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    let v = args.remove(i + 1);
    args.remove(i);
    Some(v)
}

fn code_of(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

fn node(snapshot: Option<&str>, mode: &str, files: &[String]) -> Command {
    let mut cmd = Command::new("node");
    match snapshot {
        Some(blob) => cmd.arg("--snapshot-blob").arg(blob),
        None => cmd.arg(DRIVER),
    };
    cmd.arg(mode).args(files);
    cmd
}

fn test(snapshot: Option<&str>, isolate: &str, files: &[String]) -> i32 {
    if isolate != "process" {
        return code_of(node(snapshot, isolate, files).status().expect("spawn node"));
    }
    // One process per file. Output is buffered per file and printed whole, in
    // input order, so parallel runs don't interleave lines.
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<FileResult>> = Mutex::new(vec![None; files.len()]);
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(files.len());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(file) = files.get(i) else { break };
                    let out = node(snapshot, "shared", std::slice::from_ref(file))
                        .stderr(Stdio::inherit())
                        .output()
                        .expect("spawn node");
                    results.lock().unwrap()[i] = Some((out.stdout, out.status.success()));
                }
            });
        }
    });
    let mut ok = true;
    let mut stdout = std::io::stdout().lock();
    for (out, success) in results.into_inner().unwrap().into_iter().flatten() {
        let _ = stdout.write_all(&out);
        ok &= success;
    }
    i32::from(!ok)
}
