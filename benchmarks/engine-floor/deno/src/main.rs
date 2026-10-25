//! Engine-floor prototype A: V8 embedded through deno_core.
//!
//! ```text
//! engine-deno snapshot OUT                 write a startup snapshot holding the harness
//! engine-deno [--snapshot S] run FILE      evaluate FILE as a classic script
//! engine-deno [--snapshot S] test [--isolate MODE] FILE...
//! ```
//!
//! `--isolate` picks what one test file can see of another:
//!
//! - `shared`: one V8 context for every file. Globals leak between files
//!   (bun test's model). The harness is reset between files.
//! - `context`: one isolate, a fresh V8 context per file with the harness
//!   evaluated again inside it (Jest's model: a new global per file, one
//!   process).
//! - `runtime`: a fresh `JsRuntime`, so a fresh isolate, per file. With
//!   `--snapshot` it deserializes the harness instead of evaluating it.
//!
//! The snapshot is read from a file instead of `include_bytes!` from a build
//! script so one binary measures both with and without. A shipped binary would
//! embed it; reading a few MB costs well under a millisecond here.

use deno_core::{JsRuntime, JsRuntimeForSnapshot, PollEventLoopOptions, RuntimeOptions, v8};

const HARNESS: &str = include_str!("../../harness.js");
const PRELUDE: &str = r#"globalThis.__opal_print = (s) => Deno.core.print(s + "\n");"#;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let snapshot: Option<&'static [u8]> = take_flag(&mut args, "--snapshot")
        .map(|p| &*Box::leak(std::fs::read(p).expect("read snapshot").into_boxed_slice()));
    let isolate = take_flag(&mut args, "--isolate").unwrap_or_else(|| "shared".into());

    let code = match args.first().map(String::as_str) {
        Some("snapshot") => {
            let mut rt = JsRuntimeForSnapshot::new(RuntimeOptions::default());
            rt.execute_script("prelude", PRELUDE).unwrap();
            rt.execute_script("harness.js", HARNESS).unwrap();
            std::fs::write(&args[1], rt.snapshot()).expect("write snapshot");
            0
        }
        Some("run") => block_on(async {
            let mut rt = new_runtime(snapshot, false);
            let src = std::fs::read_to_string(&args[1]).expect("read file");
            if let Err(e) = rt.execute_script("main", src) {
                eprintln!("{e}");
                return 1;
            }
            rt.run_event_loop(PollEventLoopOptions::default())
                .await
                .unwrap();
            0
        }),
        Some("test") => block_on(test(snapshot, &isolate, &args[1..])),
        _ => {
            eprintln!(
                "usage: engine-deno [--snapshot S] (snapshot OUT | run FILE | test [--isolate shared|context|runtime] FILE...)"
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

fn block_on<F: Future<Output = i32>>(f: F) -> i32 {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(f)
}

/// A runtime with the harness loaded, from the snapshot when there is one.
fn new_runtime(snapshot: Option<&'static [u8]>, harness: bool) -> JsRuntime {
    let mut rt = JsRuntime::new(RuntimeOptions {
        startup_snapshot: snapshot,
        ..Default::default()
    });
    if snapshot.is_none() && harness {
        rt.execute_script("prelude", PRELUDE).unwrap();
        rt.execute_script("harness.js", HARNESS).unwrap();
    }
    rt
}

async fn test(snapshot: Option<&'static [u8]>, isolate: &str, files: &[String]) -> i32 {
    let mut failed = 0;
    match isolate {
        "shared" => {
            let mut rt = new_runtime(snapshot, true);
            for f in files {
                rt.execute_script("reset", "__opal_reset()").unwrap();
                failed += run_file(&mut rt, f).await;
            }
        }
        "runtime" => {
            for f in files {
                let mut rt = new_runtime(snapshot, true);
                failed += run_file(&mut rt, f).await;
            }
        }
        "context" => {
            let mut rt = new_runtime(snapshot, false);
            for f in files {
                failed += run_file_in_fresh_context(&mut rt, f);
            }
        }
        other => panic!("unknown --isolate {other}"),
    }
    i32::from(failed > 0)
}

/// A test file's source with module scope, as a CommonJS wrapper gives it.
/// Evaluated bare, two classic scripts that each declare a top-level `let`
/// collide in one context, so every mode wraps, to keep the modes comparable.
fn read_wrapped(file: &str) -> String {
    let src = std::fs::read_to_string(file).expect("read test file");
    format!("(function () {{\n{src}\n}})();")
}

async fn run_file(rt: &mut JsRuntime, file: &str) -> i64 {
    let src = read_wrapped(file);
    if let Err(e) = rt.execute_script("test", src) {
        eprintln!("{file}: {e}");
        return 1;
    }
    let call = format!("__opal_run({})", js_string(file));
    let promise = rt.execute_script("run", call).unwrap();
    let resolve = rt.resolve(promise);
    let value = rt
        .with_event_loop_promise(resolve, PollEventLoopOptions::default())
        .await
        .unwrap();
    deno_core::scope!(scope, rt);
    let value = v8::Local::new(scope, value);
    value.integer_value(scope).unwrap_or(1)
}

/// Jest's model in one isolate: a new context (so a new global object) per file.
/// The harness is evaluated again in it, as Jest re-evaluates its environment
/// per file. The tests' async work is promise jobs only, so a microtask
/// checkpoint drains it; a real implementation would drive the event loop.
fn run_file_in_fresh_context(rt: &mut JsRuntime, file: &str) -> i64 {
    let src = read_wrapped(file);
    let isolate = rt.v8_isolate();
    v8::scope!(let hs, isolate);
    let global = v8::ObjectTemplate::new(hs);
    global.set(
        v8::String::new(hs, "__opal_print").unwrap().into(),
        v8::FunctionTemplate::new(hs, print_callback).into(),
    );
    let ctx = v8::Context::new(
        hs,
        v8::ContextOptions {
            global_template: Some(global),
            ..Default::default()
        },
    );
    let scope = &mut v8::ContextScope::new(hs, ctx);
    let call = format!(
        "globalThis.__opal_result = __opal_run({}).then((n) => (globalThis.__opal_failed = n));",
        js_string(file)
    );
    for code in [HARNESS, &src, &call] {
        let code = v8::String::new(scope, code).unwrap();
        v8::tc_scope!(let tc, scope);
        let ok = v8::Script::compile(tc, code, None)
            .and_then(|s| s.run(tc))
            .is_some();
        if !ok {
            let msg = tc
                .exception()
                .map(|e| e.to_rust_string_lossy(tc))
                .unwrap_or_default();
            eprintln!("{file}: {msg}");
            return 1;
        }
    }
    scope.perform_microtask_checkpoint();
    let key = v8::String::new(scope, "__opal_failed").unwrap();
    let failed = ctx.global(scope).get(scope, key.into()).unwrap();
    if failed.is_undefined() {
        1
    } else {
        failed.integer_value(scope).unwrap_or(1)
    }
}

fn print_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue,
) {
    use std::io::Write as _;
    let s = args.get(0).to_rust_string_lossy(scope);
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{s}");
}

fn js_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
