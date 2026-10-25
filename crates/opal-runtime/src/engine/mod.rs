//! Running a JavaScript file on V8, from the startup snapshot build.rs made.
//!
//! Starting is: initialize V8 on its single-threaded platform, create an
//! isolate from the embedded snapshot (which already holds console, process
//! and the module loader), and call the entry points the bootstrap left in
//! the snapshot's context data. Nothing is compiled or run for the runtime
//! itself before the user's code.
//!
//! The event loop is driven from here, in Node's phase order, minus the
//! phases that wait on I/O, of which there is none yet: run the main module;
//! then, repeatedly, every due timer, then the immediates queued so far, then
//! sleep until the next timer is due. `process.nextTick` callbacks and the
//! microtask queue are drained after the main module and after every
//! callback, as in Node.
//! When async I/O arrives, readiness comes from epoll/kqueue (mio or libc),
//! never tokio (deny.toml; benchmarks/engine-spikes/README.md).

// Before the modules: macro_rules! are visible only after their definition.
include!("shared.rs");

mod esm;
mod fatal;
mod ops;
mod resolution;
mod typescript;

use std::path::Path;
use std::time::Duration;

use v8::MapFnTo;

use crate::trace;
use ops::Rejections;

static SNAPSHOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/snapshot.bin"));

include!(concat!(env!("OUT_DIR"), "/builtins.rs"));

/// The embedded source of built-in module `id`.
fn builtin_source(id: &str) -> Option<&'static str> {
    BUILTINS
        .binary_search_by(|(name, _)| (*name).cmp(id))
        .ok()
        .map(|index| BUILTINS[index].1)
}

macro_rules! op_references {
    ($($name:ident),* $(,)?) => {
        fn op_references() -> Vec<v8::ExternalReference> {
            vec![$(v8::ExternalReference { function: ops::$name.map_fn_to() }),*]
        }
    };
}
for_each_op!(op_references);

macro_rules! type_references {
    ($($name:ident: $check:expr),* $(,)?) => {
        fn type_references() -> Vec<v8::ExternalReference> {
            vec![$(v8::ExternalReference { function: ops::$name.map_fn_to() }),*]
        }
    };
}
for_each_type_check!(type_references);

// The ops build.rs stands in for: the runtime's own.
use resolution::{
    package_type as package_type_op, resolve as resolve_op,
    take_warnings as take_resolution_warnings_op,
};

/// ops::error_arrow, which build.rs stands in for.
fn error_arrow_op(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let message = v8::Exception::create_message(scope, args.get(0));
    if let Some(arrow) = fatal::arrow(scope, message) {
        rv.set(v8::String::new(scope, &arrow).unwrap().into());
    }
}

/// ops::strip_types, which build.rs stands in for.
fn strip_types_op(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let source = args.get(0).to_rust_string_lossy(scope);
    match typescript::strip(&source) {
        typescript::Stripped::Code(code) => {
            if let Some(code) = v8::String::new(scope, &code) {
                rv.set(code.into());
            }
        }
        typescript::Stripped::Error(error) => {
            let (line, snippet) = typescript::snippet(&source, error.start, error.end);
            let values = [
                v8::String::new(scope, error.kind.code()).unwrap().into(),
                v8::String::new(scope, &error.message).unwrap().into(),
                v8::Integer::new(scope, line as i32).into(),
                v8::String::new(scope, &snippet).unwrap().into(),
            ];
            rv.set(v8::Array::new_with_elements(scope, &values).into());
        }
    }
}

/// The native functions the snapshot refers to, in the order build.rs
/// registered them: the ops, then the type checks.
fn external_references() -> std::borrow::Cow<'static, [v8::ExternalReference]> {
    let mut references = op_references();
    references.extend(type_references());
    references.into()
}

extern "C" fn on_promise_reject(message: v8::PromiseRejectMessage) {
    v8::callback_scope!(unsafe scope, &message);
    let promise = message.get_promise();
    match message.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
            let reason = message
                .get_value()
                .unwrap_or_else(|| v8::undefined(scope).into());
            let entry = (
                v8::Global::new(scope, promise),
                v8::Global::new(scope, reason),
            );
            if let Some(rejections) = scope.get_slot_mut::<Rejections>() {
                rejections.0.push(entry);
            }
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
            let promise = v8::Global::new(scope, promise);
            if let Some(rejections) = scope.get_slot_mut::<Rejections>() {
                rejections.0.retain(|(p, _)| *p != promise);
            }
        }
        _ => {}
    }
}

/// main.js's prepareStackTrace, which V8 calls through
/// [`prepare_stack_trace`] the first time an error's stack is read.
struct PrepareStackTrace(v8::Global<v8::Function>);

fn prepare_stack_trace<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    error: v8::Local<'s, v8::Value>,
    sites: v8::Local<'s, v8::Array>,
) -> v8::Local<'s, v8::Value> {
    let function = scope
        .get_slot::<PrepareStackTrace>()
        .map(|prepare| v8::Local::new(scope, &prepare.0));
    if let Some(function) = function {
        let global = scope.get_current_context().global(scope);
        v8::tc_scope!(let tc, scope);
        let undefined = v8::undefined(tc).into();
        if let Some(stack) = function.call(tc, undefined, &[global.into(), error, sites.into()]) {
            return v8::Local::new(tc, v8::Global::new(tc, stack));
        }
    }
    // The formatter threw (or is missing): what V8 itself would print.
    let mut text = error.to_rust_string_lossy(scope);
    for i in 0..sites.length() {
        if let Some(site) = sites.get_index(scope, i) {
            text.push_str("\n    at ");
            text.push_str(&site.to_rust_string_lossy(scope));
        }
    }
    v8::String::new(scope, &text).unwrap().into()
}

/// Runs `entry` as the main module with `args` as its arguments, then the
/// event loop until nothing is left, and exits the process with the code the
/// script left. Never returns: the isolate is not torn down, because the
/// process is ending and teardown would only add to the time it takes.
pub fn run_main(entry: &Path, args: &[String]) -> ! {
    trace::checkpoint("engine start");
    v8::V8::set_flags_from_string(V8_FLAGS);
    v8::V8::initialize_platform(v8::new_single_threaded_default_platform(false).make_shared());
    v8::V8::initialize();
    trace::checkpoint("v8 initialized");

    let params = v8::Isolate::create_params()
        .snapshot_blob(SNAPSHOT.into())
        .external_references(external_references());
    let isolate = &mut v8::Isolate::new(params);
    isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
    isolate.set_promise_reject_callback(on_promise_reject);
    isolate.set_slot(Rejections::default());

    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let main = scope
        .get_context_data_from_snapshot_once::<v8::Object>(0)
        .expect("the snapshot has no entry points");
    let key = v8::String::new(scope, "prepareStackTrace").unwrap();
    let prepare = main.get(scope, key.into()).unwrap();
    let prepare = v8::Local::<v8::Function>::try_from(prepare).expect("main.prepareStackTrace");
    let prepare = PrepareStackTrace(v8::Global::new(scope, prepare));
    scope.set_slot(prepare);
    scope.set_prepare_stack_trace_callback(prepare_stack_trace);
    let hook = |scope: &mut v8::PinScope, name: &str| {
        let key = v8::String::new(scope, name).unwrap();
        let value = main.get(scope, key.into()).unwrap();
        let function = v8::Local::<v8::Function>::try_from(value).expect("an ES module hook");
        v8::Global::new(scope, function)
    };
    let hooks = esm::Hooks {
        import_meta: hook(scope, "initializeImportMeta"),
        import_dynamic: hook(scope, "importDynamic"),
    };
    scope.set_slot(hooks);
    scope.set_host_initialize_import_meta_object_callback(esm::import_meta);
    scope.set_host_import_module_dynamically_callback(esm::import_dynamic);
    trace::checkpoint("isolate from snapshot");

    let exec_path = std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "opal".into());
    let mut argv = vec![exec_path, entry.to_string_lossy().into_owned()];
    argv.extend(args.iter().cloned());
    let argv: Vec<v8::Local<v8::Value>> = argv
        .iter()
        .map(|arg| v8::String::new(scope, arg).unwrap().into())
        .collect();
    let argv = v8::Array::new_with_elements(scope, &argv);

    let argv0 = std::env::args_os().next().unwrap_or_default();
    let argv0 = v8::String::new(scope, &argv0.to_string_lossy()).unwrap();

    let main = Loop {
        main: v8::Global::new(scope, main),
    };
    main.call(scope, "start", &[argv.into(), argv0.into()]);
    trace::checkpoint("main module ran");
    loop {
        // Each turn of the loop gets its own handle scope, so that what a
        // turn creates is freed when it ends rather than when the process
        // does: a script with an interval runs for as long as it likes.
        v8::scope!(let scope, scope);
        main.drain(scope);
        // Timers phase: every due timer, draining after each.
        while main
            .call(scope, "runTimer", &[])
            .is_some_and(|ran| ran.is_true())
        {
            main.drain(scope);
        }
        // Check phase: the immediates queued before it began.
        main.call(scope, "beginCheck", &[]);
        while main
            .call(scope, "runImmediate", &[])
            .is_some_and(|ran| ran.is_true())
        {
            main.drain(scope);
        }
        let wait = main.call(scope, "nextTimer", &[]);
        let wait = wait.and_then(|w| w.number_value(scope)).unwrap_or(-1.0);
        if wait < 0.0 {
            // Nothing is pending. 'beforeExit' listeners may schedule more.
            let more = main.call(scope, "beforeExit", &[]);
            main.drain(scope);
            if more.is_some_and(|more| more.is_true()) {
                continue;
            }
            break;
        }
        if wait > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(wait / 1000.0));
        }
    }
    let code = main.call(scope, "exit", &[]);
    let code = code.and_then(|c| c.int32_value(scope)).unwrap_or(1);
    leave(code)
}

/// The bootstrap's entry points (main.js), called by name.
struct Loop {
    main: v8::Global<v8::Object>,
}

impl Loop {
    /// Calls `main[name](...args)`. An exception that escapes it is
    /// uncaught: it goes to 'uncaughtException' listeners, or ends the
    /// process the way Node does.
    fn call<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        name: &str,
        args: &[v8::Local<'s, v8::Value>],
    ) -> Option<v8::Local<'s, v8::Value>> {
        let (function, receiver) = self.function(scope, name);
        let (result, failure) = {
            v8::tc_scope!(let tc, scope);
            let result = function.call(tc, receiver, args);
            let result = result.map(|r| v8::Global::new(tc, r));
            let failure = match result {
                Some(_) => None,
                None => tc.exception().map(|exception| {
                    let message = tc.message().map(|m| v8::Global::new(tc, m));
                    (v8::Global::new(tc, exception), message)
                }),
            };
            (result, failure)
        };
        if let Some((exception, message)) = failure {
            let exception = v8::Local::new(scope, exception);
            let message = message.map(|m| v8::Local::new(scope, m));
            self.uncaught(scope, exception, message);
        }
        result.map(|r| v8::Local::new(scope, r))
    }

    /// Runs process.nextTick callbacks and microtasks until both are empty,
    /// then reports the first promise still rejected with no handler, as
    /// Node's default `--unhandled-rejections=throw` does.
    fn drain(&self, scope: &mut v8::PinScope) {
        loop {
            v8::scope!(let scope, scope);
            let Some(rejected) = self.call(scope, "drain", &[]) else {
                continue;
            };
            let Ok(rejected) = v8::Local::<v8::Array>::try_from(rejected) else {
                return;
            };
            let reason = rejected.get_index(scope, 0).unwrap();
            let message = v8::Exception::create_message(scope, reason);
            self.uncaught(scope, reason, Some(message));
        }
    }

    fn uncaught<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        error: v8::Local<'s, v8::Value>,
        message: Option<v8::Local<'s, v8::Message>>,
    ) {
        let handled = {
            let (function, receiver) = self.function(scope, "uncaught");
            v8::tc_scope!(let tc, scope);
            match function.call(tc, receiver, &[error]) {
                Some(handled) => handled.is_true(),
                None => {
                    // A listener threw: Node prints that error and exits 7.
                    let thrown = tc.exception();
                    let thrown_message = tc.message();
                    if let (Some(thrown), Some(m)) = (thrown, thrown_message) {
                        let formatted = thrown.to_rust_string_lossy(tc);
                        fatal::report(tc, thrown, Some(m), &formatted);
                    }
                    leave(7)
                }
            }
        };
        if handled {
            return;
        }
        // As in Node: exit code 1 and 'exit' listeners first, then the
        // report (lib/internal/process/execution.js, then node_errors.cc).
        let code = {
            let (function, receiver) = self.function(scope, "fatalExit");
            v8::tc_scope!(let tc, scope);
            match function.call(tc, receiver, &[]) {
                Some(code) => code.int32_value(tc).unwrap_or(1),
                None => 7,
            }
        };
        let formatted = {
            let (function, receiver) = self.function(scope, "inspectFatal");
            v8::tc_scope!(let tc, scope);
            function
                .call(tc, receiver, &[error])
                .map(|text| text.to_rust_string_lossy(tc))
                .unwrap_or_else(|| error.to_rust_string_lossy(tc))
        };
        let arrow = self
            .call(scope, "hasArrow", &[error])
            .is_some_and(|a| a.is_true());
        let message = message.unwrap_or_else(|| v8::Exception::create_message(scope, error));
        fatal::report(scope, error, arrow.then_some(message), &formatted);
        leave(code)
    }

    /// `main[name]`, and `main` to call it on.
    fn function<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        name: &str,
    ) -> (v8::Local<'s, v8::Function>, v8::Local<'s, v8::Value>) {
        let main = v8::Local::new(scope, &self.main);
        let key = v8::String::new(scope, name).unwrap();
        let value = main.get(scope, key.into()).unwrap();
        let function = v8::Local::<v8::Function>::try_from(value)
            .unwrap_or_else(|_| panic!("the bootstrap has no entry point {name}"));
        (function, main.into())
    }
}

/// Ends the process with `code`, after printing the startup trace.
fn leave(code: i32) -> ! {
    trace::checkpoint("exit");
    trace::finish();
    std::process::exit(code)
}
