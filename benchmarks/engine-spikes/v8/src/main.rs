//! V8 through the v8 crate: the same bootstrap as the QuickJS spike, either
//! run on every start or baked into a startup snapshot made with
//! v8::SnapshotCreator, then the script named on the command line.
//!
//!     spike-v8 <script.js>
//!     spike-v8 --write-snapshot <out.bin>

use std::io::Write;

use v8::MapFnTo;

const BOOTSTRAP: &str = include_str!(concat!(env!("OUT_DIR"), "/bootstrap.js"));
static SNAPSHOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/snapshot.bin"));

fn print(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue<v8::Value>) {
    let line = args.get(0).to_rust_string_lossy(scope);
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(line.as_bytes());
    let _ = out.write_all(b"\n");
}

/// Native functions a snapshot refers to, in the same order on both sides.
fn external_references() -> std::borrow::Cow<'static, [v8::ExternalReference]> {
    vec![v8::ExternalReference { function: print.map_fn_to() }].into()
}

fn run(scope: &mut v8::PinScope, source: &str) -> bool {
    v8::tc_scope!(let tc, scope);
    let code = v8::String::new(tc, source).expect("source string");
    let ok = v8::Script::compile(tc, code, None).and_then(|script| script.run(tc)).is_some();
    if !ok {
        let message = tc
            .exception()
            .map(|e| e.to_rust_string_lossy(tc))
            .unwrap_or_else(|| "unknown error".into());
        eprintln!("{message}");
    }
    ok
}

fn install(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) {
    let function = v8::FunctionTemplate::new(scope, print).get_function(scope).expect("print");
    let key = v8::String::new(scope, "__print").unwrap();
    context.global(scope).set(scope, key.into(), function.into());
    assert!(run(scope, BOOTSTRAP), "bootstrap failed");
}

fn main() {
    // The single-threaded platform has no worker threads, so V8 has to be
    // told not to post background work (compiles, GC) to any: without
    // --single-threaded a large script or a snapshot write crashes.
    // Starting no worker threads is the point, for start-up time.
    v8::V8::set_flags_from_string(&format!("--single-threaded {}", env!("SPIKE_V8_FLAGS")));
    v8::V8::initialize_platform(v8::new_single_threaded_default_platform(false).make_shared());
    v8::V8::initialize();

    let mut args = std::env::args().skip(1);
    let first = args.next().expect("usage: spike-v8 <script.js> | --write-snapshot <out>");
    if first == "--write-snapshot" {
        let out = args.next().expect("--write-snapshot <out>");
        let mut creator = v8::Isolate::snapshot_creator(Some(external_references()), None);
        {
            v8::scope!(let scope, &mut creator);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            install(scope, context);
            scope.set_default_context(context);
        }
        let blob = creator.create_blob(v8::FunctionCodeHandling::Keep).expect("blob");
        std::fs::write(out, &*blob).expect("write snapshot");
        return;
    }

    let source = std::fs::read_to_string(&first).expect("read script");
    let mut params = v8::Isolate::create_params();
    if !SNAPSHOT.is_empty() {
        params = params.snapshot_blob(SNAPSHOT.into()).external_references(external_references());
    }
    let isolate = &mut v8::Isolate::new(params);
    let ok = {
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        if SNAPSHOT.is_empty() {
            install(scope, context);
        }
        let ok = run(scope, &source);
        scope.perform_microtask_checkpoint();
        ok
    };
    let _ = std::io::stdout().flush();
    // process::exit runs no destructors, so the isolate's heap is never torn
    // down: the process is ending, and the teardown would only add to the
    // time measured.
    std::process::exit(i32::from(!ok));
}
