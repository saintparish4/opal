// What build.rs and the crate must agree on, included by both.

/// Flags V8 is started with, in the build script that makes the snapshot and
/// in the runtime that loads it: V8 refuses a snapshot made under other
/// flags. The single-threaded platform starts no worker threads, so V8 must
/// not post background work (compiles, GC) to any.
const V8_FLAGS: &str = "--single-threaded";

/// The parameters a built-in module's source is compiled as the body of, as
/// in Node (lib/internal/bootstrap/realm.js, `compileForInternalLoader`).
const BUILTIN_PARAMS: [&str; 6] = [
    "exports",
    "require",
    "module",
    "process",
    "internalBinding",
    "primordials",
];

// The native functions the bootstrap is handed, by name, in the one order
// both sides register them in. A snapshot refers to a native function by its
// index in the external reference table, so the table the snapshot was made
// with and the table the runtime starts with have to list the same functions
// in the same order. `$m` is called with the list. Adding an op is one name
// here and one function in ops.rs; the compiler rejects a name with no
// function.
macro_rules! for_each_op {
    ($m:ident) => {
        $m! {
            write,
            exit,
            cwd,
            env,
            process_info,
            isatty,
            hrtime,
            sleep,
            run_microtasks,
            take_unhandled_rejection,
            compile_function,
            compile_builtin,
            read_file,
            os_constants,
            guess_handle_type,
            own_non_index_properties,
            promise_details,
            proxy_details,
            preview_entries,
            constructor_name,
            buffer_slice,
            buffer_write,
            buffer_byte_length_utf8,
            buffer_compare,
            buffer_compare_offset,
            buffer_copy,
            buffer_fill,
            buffer_is_utf8,
            buffer_is_ascii,
            buffer_index_of_string,
            buffer_index_of_buffer,
            buffer_index_of_number,
            buffer_swap16,
            buffer_swap32,
            buffer_swap64,
            buffer_btoa,
            buffer_atob,
            strip_types,
            resolve,
            take_resolution_warnings,
            package_type,
        }
    };
}

// internalBinding('types'): one native per V8 type predicate, named as Node
// names them (node_types.cc) once snake_case is camelCased. They follow the
// ops in the external reference table.
macro_rules! for_each_type_check {
    ($m:ident) => {
        $m! {
            is_external: |v| v.is_external(),
            is_date: |v| v.is_date(),
            is_arguments_object: |v| v.is_arguments_object(),
            is_big_int_object: |v| v.is_big_int_object(),
            is_boolean_object: |v| v.is_boolean_object(),
            is_number_object: |v| v.is_number_object(),
            is_string_object: |v| v.is_string_object(),
            is_symbol_object: |v| v.is_symbol_object(),
            is_native_error: |v| v.is_native_error(),
            is_reg_exp: |v| v.is_reg_exp(),
            is_async_function: |v| v.is_async_function(),
            is_generator_function: |v| v.is_generator_function(),
            is_generator_object: |v| v.is_generator_object(),
            is_promise: |v| v.is_promise(),
            is_map: |v| v.is_map(),
            is_set: |v| v.is_set(),
            is_map_iterator: |v| v.is_map_iterator(),
            is_set_iterator: |v| v.is_set_iterator(),
            is_weak_map: |v| v.is_weak_map(),
            is_weak_set: |v| v.is_weak_set(),
            is_array_buffer: |v| v.is_array_buffer(),
            is_data_view: |v| v.is_data_view(),
            is_shared_array_buffer: |v| v.is_shared_array_buffer(),
            is_proxy: |v| v.is_proxy(),
            is_module_namespace_object: |v| v.is_module_namespace_object(),
            is_any_array_buffer: |v| v.is_array_buffer() || v.is_shared_array_buffer(),
            is_boxed_primitive: |v| {
                v.is_number_object()
                    || v.is_string_object()
                    || v.is_boolean_object()
                    || v.is_big_int_object()
                    || v.is_symbol_object()
            },
        }
    };
}

/// Compiles a built-in module's source as Node does: the body of a function
/// of `params` (BUILTIN_PARAMS for a module; per-context scripts take
/// others), named `node:<id>` in stack traces.
fn compile_builtin_source<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    id: &str,
    source: v8::Local<'s, v8::String>,
    params: &[v8::Local<'s, v8::String>],
) -> Option<v8::Local<'s, v8::Function>> {
    let resource = v8::String::new(scope, &format!("node:{id}"))?;
    let origin = v8::ScriptOrigin::new(
        scope,
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
    let mut source = v8::script_compiler::Source::new(source, Some(&origin));
    v8::script_compiler::compile_function(
        scope,
        &mut source,
        params,
        &[],
        v8::script_compiler::CompileOptions::NoCompileOptions,
        v8::script_compiler::NoCacheReason::NoReason,
    )
}
