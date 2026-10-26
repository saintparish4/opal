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
            umask,
            credentials,
            chdir,
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
            error_source_positions,
            call_sites,
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
            fs_setup,
            fs_constants,
            uv_errors,
            fs_access,
            fs_exists_sync,
            fs_open,
            fs_close,
            fs_read,
            fs_read_buffers,
            fs_write_buffer,
            fs_write_string,
            fs_write_buffers,
            fs_read_file_utf8,
            fs_write_file_utf8,
            fs_rename,
            fs_ftruncate,
            fs_rmdir,
            fs_rm_sync,
            fs_fdatasync,
            fs_fsync,
            fs_mkdir,
            fs_readdir,
            fs_internal_module_stat,
            fs_stat,
            fs_lstat,
            fs_fstat,
            fs_statfs,
            fs_readlink,
            fs_symlink,
            fs_link,
            fs_unlink,
            fs_chmod,
            fs_fchmod,
            fs_chown,
            fs_fchown,
            fs_lchown,
            fs_utimes,
            fs_futimes,
            fs_lutimes,
            fs_realpath,
            fs_mkdtemp,
            fs_copy_file,
            strip_types,
            resolve,
            take_resolution_warnings,
            package_type,
            esm_compile,
            esm_synthetic,
            esm_set_export,
            esm_link,
            esm_instantiate,
            esm_evaluate,
            esm_namespace,
            esm_status,
            error_arrow,
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

/// A built-in's embedded source as a V8 string. An ASCII one, which nearly
/// all are, V8 reads in place as a one-byte string; any other (acorn's, with
/// its Unicode tables) is copied into a string of V8's own.
fn builtin_source_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: &'static str,
) -> Option<v8::Local<'s, v8::String>> {
    if source.is_ascii() {
        v8::String::new_external_onebyte_static(scope, source.as_bytes())
    } else {
        v8::String::new(scope, source)
    }
}

/// How many code caches V8 has refused (one made by another V8, or under
/// other flags). The module compiles from source then, so a refusal costs
/// time, not correctness; the startup trace reports the count.
static CODE_CACHES_REJECTED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Compiles a built-in module's source as Node does: the body of a function
/// of `params` (BUILTIN_PARAMS for a module; per-context scripts take
/// others), named `node:<id>` in stack traces. With `cache`, the code cache
/// build.rs made for it (only ever for BUILTIN_PARAMS), V8 deserializes the
/// compiled code instead of parsing and compiling the source.
fn compile_builtin_source<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    id: &str,
    source: v8::Local<'s, v8::String>,
    params: &[v8::Local<'s, v8::String>],
    cache: Option<&'static [u8]>,
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
    use v8::script_compiler::{CachedData, CompileOptions, NoCacheReason, Source};
    let (mut source, options) = match cache {
        Some(bytes) => (
            Source::new_with_cached_data(source, Some(&origin), CachedData::new(bytes)),
            CompileOptions::ConsumeCodeCache,
        ),
        None => (Source::new(source, Some(&origin)), CompileOptions::NoCompileOptions),
    };
    let function = v8::script_compiler::compile_function(
        scope,
        &mut source,
        params,
        &[],
        options,
        NoCacheReason::NoReason,
    );
    if source.get_cached_data().is_some_and(|data| data.rejected()) {
        CODE_CACHES_REJECTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    function
}
