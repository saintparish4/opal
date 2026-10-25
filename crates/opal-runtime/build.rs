//! Makes the V8 startup snapshot `opal run` starts from, and the table of
//! built-in modules the binary embeds.
//!
//! The bootstrap (js/bootstrap/*.js) runs here, once, on the same V8 and the
//! same native functions (src/engine/ops.rs) the runtime links, and the heap
//! it leaves is serialized into OUT_DIR/snapshot.bin, which the crate embeds.
//! So starting a script deserializes a heap that already has console,
//! process and the module loader in it, instead of running their JavaScript
//! again: in the spikes, 256 KB of bootstrap cost +14.4 ms per start when run
//! and +2.2 ms when snapshotted (benchmarks/engine-spikes/README.md).
//!
//! That +2.2 ms is why most built-in modules (js/node/, Node's own library,
//! and js/opal/, Opal's versions of Node modules) are not in the snapshot:
//! they are embedded as source and compiled the first time something
//! requires them. SNAPSHOT_BUILTINS names the ones that are.
//!
//! The bootstrap only defines things. It must not read the environment, the
//! clock or the filesystem, because whatever it reads here would be baked
//! into every run. Per-run state reaches it through the entry points it
//! leaves in the snapshot's context data, which the runtime calls with argv.
//!
//! A snapshot is specific to the V8 build and the CPU architecture it was made
//! on, so this cannot cross-compile: the release builds each target on a
//! machine of that architecture.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use v8::MapFnTo;

include!("src/engine/shared.rs");

#[path = "src/engine/ops.rs"]
#[allow(dead_code)]
mod ops;

/// The bootstrap's files, in the order they run. Each is a function
/// expression called with `(internals, ops, primordials)`.
const BOOTSTRAP: &[&str] = &[
    "realm", "process", "console", "timers", "loader", "esm", "main",
];

/// Built-in modules loaded while the snapshot is made, so they cost
/// deserialization on every start instead of compilation on first use.
/// None, on measurement: with events (and the internal/util, errors,
/// validators and inspect it brings) in the snapshot, hello.js started
/// 5.7 ms slower than the bare spike; process and console load them on
/// first use instead (js/bootstrap/process.js). The bootstrap itself still
/// loads internal/fixed_queue, which is small.
const SNAPSHOT_BUILTINS: &[&str] = &[];

/// Where the built-in modules come from, by directory: js/node is Node's
/// library, copied unmodified (js/node/update.sh); js/opal is Opal's own
/// versions of the modules Node's files require but that need Node's C++.
const BUILTIN_DIRS: &[&str] = &["node", "opal"];

static SOURCES: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();

fn builtin_source(id: &str) -> Option<&'static str> {
    SOURCES
        .get()?
        .iter()
        .find(|(name, _)| name == id)
        .map(|(_, source)| *source)
}

fn leave(code: i32) -> ! {
    std::process::exit(code)
}

/// Making the snapshot strips no TypeScript and resolves no modules, so the
/// build has neither; the runtime's are in src/engine/.
macro_rules! unavailable_at_build_time {
    ($($name:ident),*) => {
        $(
            pub fn $name<'s>(
                scope: &mut v8::PinScope<'s, '_>,
                _: v8::FunctionCallbackArguments<'s>,
                _: v8::ReturnValue<'s, v8::Value>,
            ) {
                let text = concat!(stringify!($name), " is not available while the snapshot is made");
                let message = v8::String::new(scope, text).unwrap();
                let error = v8::Exception::error(scope, message);
                scope.throw_exception(error);
            }
        )*
    };
}
unavailable_at_build_time!(
    strip_types_op,
    resolve_op,
    take_resolution_warnings_op,
    package_type_op,
    error_arrow_op
);

/// The ES module ops' runtime side (src/engine/esm.rs), which making the
/// snapshot never calls.
mod esm {
    #![allow(dead_code)]
    unavailable_at_build_time!(
        compile,
        synthetic,
        set_export,
        link,
        instantiate,
        evaluate,
        namespace,
        status
    );
}

macro_rules! op_table {
    ($($name:ident),* $(,)?) => {
        const OP_NAMES: &[&str] = &[$(stringify!($name)),*];
        fn op_callbacks() -> Vec<v8::FunctionCallback> {
            vec![$(ops::$name.map_fn_to()),*]
        }
    };
}
for_each_op!(op_table);

macro_rules! type_table {
    ($($name:ident: $check:expr),* $(,)?) => {
        const TYPE_NAMES: &[&str] = &[$(stringify!($name)),*];
        fn type_callbacks() -> Vec<v8::FunctionCallback> {
            vec![$(ops::$name.map_fn_to()),*]
        }
    };
}
for_each_type_check!(type_table);

fn main() {
    let host = std::env::var("HOST").unwrap();
    let target = std::env::var("TARGET").unwrap();
    assert!(
        host == target,
        "opal-runtime cannot be cross-compiled ({host} -> {target}): its V8 snapshot is made by \
         running V8 for the target, so build on a {target} machine"
    );

    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=js");
    println!("cargo:rerun-if-changed=src/engine/shared.rs");
    println!("cargo:rerun-if-changed=src/engine/ops.rs");

    let builtins = discover_builtins(&dir.join("js"));
    write_builtin_table(&out, &builtins);
    SOURCES
        .set(
            builtins
                .iter()
                .map(|(id, path)| {
                    let source = std::fs::read_to_string(path).unwrap();
                    (id.clone(), &*Box::leak(source.into_boxed_str()))
                })
                .collect(),
        )
        .unwrap();

    v8::V8::set_flags_from_string(V8_FLAGS);
    v8::V8::initialize_platform(v8::new_single_threaded_default_platform(false).make_shared());
    v8::V8::initialize();

    let callbacks: Vec<v8::FunctionCallback> =
        op_callbacks().into_iter().chain(type_callbacks()).collect();
    let references: Vec<v8::ExternalReference> = callbacks
        .iter()
        .map(|&function| v8::ExternalReference { function })
        .collect();
    let mut creator = v8::Isolate::snapshot_creator(Some(references.into()), None);
    {
        v8::scope!(let scope, &mut creator);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);

        let ops = v8::Object::new(scope);
        let names = OP_NAMES.iter().chain(TYPE_NAMES);
        for (name, callback) in names.zip(&callbacks) {
            let function = v8::FunctionTemplate::builder_raw(*callback)
                .build(scope)
                .get_function(scope)
                .unwrap();
            let key = v8::String::new(scope, name).unwrap();
            function.set_name(key);
            ops.set(scope, key.into(), function.into());
        }

        // Node's primordials, made the way Node makes them: its own
        // per-context script run on a null-prototype object.
        let primordials = v8::Object::new(scope);
        let null = v8::null(scope).into();
        primordials.set_prototype(scope, null);
        let source = builtin_source("internal/per_context/primordials").unwrap();
        let function = compile(
            scope,
            "internal/per_context/primordials",
            source,
            &["primordials"],
        );
        let undefined = v8::undefined(scope).into();
        call(
            scope,
            "primordials",
            function,
            undefined,
            &[primordials.into()],
        );

        let internals = v8::Object::new(scope);
        let preload: Vec<v8::Local<v8::Value>> = SNAPSHOT_BUILTINS
            .iter()
            .map(|id| v8::String::new(scope, id).unwrap().into())
            .collect();
        let preload = v8::Array::new_with_elements(scope, &preload);
        let key = v8::String::new(scope, "snapshotBuiltins").unwrap();
        internals.set(scope, key.into(), preload.into());

        for name in BOOTSTRAP {
            let path = dir.join("js/bootstrap").join(format!("{name}.js"));
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let function = evaluate(scope, &format!("opal:internal/{name}"), &source);
            let function = v8::Local::<v8::Function>::try_from(function)
                .unwrap_or_else(|_| panic!("js/bootstrap/{name}.js is not a function expression"));
            let undefined = v8::undefined(scope).into();
            let args = [internals.into(), ops.into(), primordials.into()];
            call(scope, name, function, undefined, &args);
        }

        // main.js leaves the runtime's entry points on `internals.main`.
        let key = v8::String::new(scope, "main").unwrap();
        let main = internals.get(scope, key.into()).unwrap();
        let main = v8::Local::<v8::Object>::try_from(main).expect("internals.main");

        // Run what every start runs (formatting values for console.log), so
        // that its compiled code is in the snapshot: V8 keeps compiled code
        // only for functions that ran while the snapshot was made.
        let key = v8::String::new(scope, "warmup").unwrap();
        let warmup = main.get(scope, key.into()).unwrap();
        let warmup = v8::Local::<v8::Function>::try_from(warmup).expect("main.warmup");
        call(scope, "warmup", warmup, main.into(), &[]);
        main.delete(scope, key.into());

        let index = scope.add_context_data(context, main);
        assert_eq!(
            index, 0,
            "the runtime reads the entry points from context data 0"
        );
        scope.set_default_context(context);
    }
    let blob = creator
        .create_blob(v8::FunctionCodeHandling::Keep)
        .expect("V8 could not create the snapshot");
    std::fs::write(out.join("snapshot.bin"), &*blob).unwrap();
}

/// Every js/<dir>/**/*.js under BUILTIN_DIRS, by its builtin id (its path
/// under the directory, without `.js`), sorted. An id in two directories is
/// an error: which one ran would depend on the order they were read.
fn discover_builtins(js: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for dir in BUILTIN_DIRS {
        let root = js.join(dir);
        let mut stack = vec![root.clone()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(&current).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "js") {
                    let relative = path.strip_prefix(&root).unwrap().with_extension("");
                    let id = relative.to_string_lossy().replace('\\', "/");
                    let source = std::fs::read(&path).unwrap();
                    assert!(
                        source.is_ascii(),
                        "{}: built-in sources must be ASCII, so V8 can use them in place",
                        path.display()
                    );
                    if let Some((_, other)) = found.iter().find(|(name, _)| *name == id) {
                        panic!(
                            "built-in {id} is both {} and {}",
                            other.display(),
                            path.display()
                        );
                    }
                    found.push((id, path));
                }
            }
        }
    }
    found.sort();
    found
}

fn write_builtin_table(out: &Path, builtins: &[(String, PathBuf)]) {
    let mut table = String::from(
        "// Generated by build.rs: every built-in module's id and source.\n\
         static BUILTINS: &[(&str, &str)] = &[\n",
    );
    for (id, path) in builtins {
        let path = path.display().to_string();
        table.push_str(&format!("    ({id:?}, include_str!({path:?})),\n"));
    }
    table.push_str("];\n");
    std::fs::write(out.join("builtins.rs"), table).unwrap();
}

fn compile<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    id: &str,
    source: &str,
    params: &[&str],
) -> v8::Local<'s, v8::Function> {
    v8::tc_scope!(let tc, scope);
    let code = v8::String::new(tc, source).unwrap();
    let resource = v8::String::new(tc, &format!("node:{id}")).unwrap();
    let origin = v8::ScriptOrigin::new(
        tc,
        resource.into(),
        0,
        0,
        false,
        -1,
        None,
        false,
        false,
        false,
        None,
    );
    let params: Vec<v8::Local<v8::String>> = params
        .iter()
        .map(|p| v8::String::new(tc, p).unwrap())
        .collect();
    let mut source = v8::script_compiler::Source::new(code, Some(&origin));
    match v8::script_compiler::compile_function(
        tc,
        &mut source,
        &params,
        &[],
        v8::script_compiler::CompileOptions::NoCompileOptions,
        v8::script_compiler::NoCacheReason::NoReason,
    ) {
        Some(function) => function,
        None => {
            let exception = tc.exception();
            panic!("{id}: {}", describe(tc, exception))
        }
    }
}

fn evaluate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: &str,
    source: &str,
) -> v8::Local<'s, v8::Value> {
    v8::tc_scope!(let tc, scope);
    let code = v8::String::new(tc, source).unwrap();
    let resource = v8::String::new(tc, name).unwrap();
    let origin = v8::ScriptOrigin::new(
        tc,
        resource.into(),
        0,
        0,
        false,
        0,
        None,
        false,
        false,
        false,
        None,
    );
    match v8::Script::compile(tc, code, Some(&origin)).and_then(|script| script.run(tc)) {
        Some(value) => value,
        None => {
            let exception = tc.exception();
            panic!("{name}: {}", describe(tc, exception))
        }
    }
}

fn call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: &str,
    function: v8::Local<'s, v8::Function>,
    receiver: v8::Local<'s, v8::Value>,
    args: &[v8::Local<'s, v8::Value>],
) {
    v8::tc_scope!(let tc, scope);
    if function.call(tc, receiver, args).is_none() {
        let exception = tc.exception();
        panic!("{name}: {}", describe(tc, exception));
    }
}

fn describe(scope: &v8::PinScope, exception: Option<v8::Local<v8::Value>>) -> String {
    let Some(exception) = exception else {
        return "terminated".into();
    };
    if let Ok(object) = v8::Local::<v8::Object>::try_from(exception) {
        let key = v8::String::new(scope, "stack").unwrap();
        if let Some(stack) = object.get(scope, key.into())
            && stack.is_string()
        {
            return stack.to_rust_string_lossy(scope);
        }
    }
    exception.to_rust_string_lossy(scope)
}
