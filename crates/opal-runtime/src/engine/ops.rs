//! The native functions the bootstrap calls, one per name in `shared.rs`'s
//! `for_each_op!` and `for_each_type_check!`.
//!
//! build.rs compiles this same file and runs the bootstrap on it to make the
//! snapshot, so it depends on nothing but v8, libc and std, plus two
//! functions each side defines for itself: `leave(code)` (how the process
//! ends) and `builtin_source(id)` (the embedded built-in modules). Arguments
//! are checked by the JavaScript that calls these, which is all internal, so
//! a wrong type here is a bug in js/, and is reported as a TypeError rather
//! than a crash.

use std::io;

use super::{
    BUILTIN_PARAMS, builtin_code_cache, builtin_source, builtin_source_string,
    compile_builtin_source, error_arrow_op, esm, leave, package_type_op, resolve_op,
    strip_types_op, take_resolution_warnings_op,
};

// Explicit, so build.rs (which includes this file by path) and the crate
// find the same file.
#[path = "ops/buffer.rs"]
mod buffer;
pub use buffer::*;
#[path = "ops/fs.rs"]
mod fs;
pub use fs::*;

type Args<'a> = v8::FunctionCallbackArguments<'a>;
type Ret<'a> = v8::ReturnValue<'a, v8::Value>;

/// Promises rejected with no handler, oldest first, until a handler is added
/// or the event loop reports them. An isolate slot, filled by the runtime's
/// promise-reject callback.
#[derive(Default)]
pub struct Rejections(pub Vec<(v8::Global<v8::Promise>, v8::Global<v8::Value>)>);

fn string<'s>(scope: &v8::PinScope<'s, '_>, text: &str) -> v8::Local<'s, v8::String> {
    v8::String::new(scope, text).unwrap()
}

fn type_error(scope: &mut v8::PinScope, message: &str) {
    let message = string(scope, message);
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
}

fn set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
    value: v8::Local<'s, v8::Value>,
) {
    let key = string(scope, key);
    object.set(scope, key.into(), value);
}

/// Throws the error Node throws for a failed system call: `ENOENT: no such
/// file or directory, open '/x'`, with `errno`, `code`, `syscall` and `path`.
pub fn throw_io(scope: &mut v8::PinScope, error: &io::Error, syscall: &str, path: &str) {
    let errno = error.raw_os_error().unwrap_or(0);
    let (code, description) = fs::uv_error(-errno);
    let text = match path {
        "" => format!("{code}: {description}, {syscall}"),
        _ => format!("{code}: {description}, {syscall} '{path}'"),
    };
    let message = string(scope, &text);
    let exception = v8::Exception::error(scope, message);
    let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
    let errno = v8::Integer::new(scope, -errno).into();
    set(scope, object, "errno", errno);
    let code = string(scope, code).into();
    set(scope, object, "code", code);
    let syscall = string(scope, syscall).into();
    set(scope, object, "syscall", syscall);
    if !path.is_empty() {
        let path = string(scope, path).into();
        set(scope, object, "path", path);
    }
    scope.throw_exception(exception);
}

/// `write(fd, data)`: writes all of `data` (a string, as UTF-8, or the bytes
/// of a typed array) to fd 1 or 2, unbuffered, the way Node writes to a file
/// or pipe: synchronously, one write per call.
pub fn write(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let fd = args.get(0).int32_value(scope).unwrap_or(1);
    let data = args.get(1);
    let result = match v8::Local::<v8::ArrayBufferView>::try_from(data) {
        Ok(view) => {
            let mut bytes = vec![0; view.byte_length()];
            view.copy_contents(&mut bytes);
            write_all(fd, &bytes)
        }
        Err(_) => write_all(fd, data.to_rust_string_lossy(scope).as_bytes()),
    };
    if let Err(error) = result {
        // A closed stdout (`opal run x.js | head -1`) is EPIPE. Node's
        // process.stdout reports it as an 'error' event that nothing listens
        // to by default, and the process carries on, so this ignores it too.
        if error.raw_os_error() != Some(libc::EPIPE) {
            throw_io(scope, &error, "write", "");
        }
    }
}

pub fn write_all(fd: i32, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        // SAFETY: bytes is a valid buffer of bytes.len() bytes.
        let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if n < 0 {
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => continue,
                Some(libc::EAGAIN) => {
                    // Non-blocking stdout (a parent set O_NONBLOCK on a shared
                    // pipe): wait until it can take more.
                    let mut poll = libc::pollfd {
                        fd,
                        events: libc::POLLOUT,
                        revents: 0,
                    };
                    // SAFETY: one valid pollfd.
                    unsafe { libc::poll(&mut poll, 1, -1) };
                    continue;
                }
                _ => return Err(error),
            }
        }
        bytes = &bytes[n as usize..];
    }
    Ok(())
}

/// `exit(code)`: ends the process now, without tearing the isolate down.
pub fn exit(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let code = args.get(0).int32_value(scope).unwrap_or(0);
    leave(code)
}

/// `cwd()`: the working directory, as Node's process.cwd() gives it.
pub fn cwd(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    match std::env::current_dir() {
        Ok(dir) => rv.set(string(scope, &dir.to_string_lossy()).into()),
        Err(error) => throw_io(scope, &error, "uv_cwd", ""),
    }
}

/// `env()`: every environment variable, as a plain object.
pub fn env(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    let object = v8::Object::new(scope);
    for (key, value) in std::env::vars_os() {
        let key = string(scope, &key.to_string_lossy());
        let value = string(scope, &value.to_string_lossy());
        object.set(scope, key.into(), value.into());
    }
    rv.set(object.into());
}

/// `processInfo()`: `[pid, ppid, platform, arch, v8 version]`, with platform
/// and arch spelled as Node spells them (`linux`, `darwin`; `x64`, `arm64`).
pub fn process_info(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    // SAFETY: getpid and getppid cannot fail.
    let (pid, ppid) = unsafe { (libc::getpid(), libc::getppid()) };
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let values = [
        v8::Integer::new(scope, pid).into(),
        v8::Integer::new(scope, ppid).into(),
        string(scope, platform).into(),
        string(scope, arch).into(),
        string(scope, v8::V8::get_version()).into(),
    ];
    rv.set(v8::Array::new_with_elements(scope, &values).into());
}

/// `isatty(fd)`.
pub fn isatty(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let fd = args.get(0).int32_value(scope).unwrap_or(-1);
    // SAFETY: isatty accepts any integer.
    rv.set_bool(unsafe { libc::isatty(fd) } == 1);
}

/// `hrtime()`: CLOCK_MONOTONIC in nanoseconds, as a BigInt.
pub fn hrtime(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid timespec to write into.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    let ns = now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64;
    rv.set(v8::BigInt::new_from_u64(scope, ns).into());
}

/// `credentials()`: [uid, euid, gid, egid, groups], for process.getuid and
/// the rest. groups ends with the effective gid when getgroups leaves it
/// out, as node_credentials.cc's GetGroups does.
pub fn credentials(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    // SAFETY: these calls can't fail.
    let (uid, euid, gid, egid) = unsafe {
        (
            libc::getuid(),
            libc::geteuid(),
            libc::getgid(),
            libc::getegid(),
        )
    };
    // SAFETY: a count of 0 asks only for the number of groups.
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) }.max(0) as usize;
    let mut groups = vec![0 as libc::gid_t; count];
    // SAFETY: groups has room for count entries.
    let got = unsafe { libc::getgroups(count as i32, groups.as_mut_ptr()) }.max(0) as usize;
    groups.truncate(got);
    if !groups.contains(&egid) {
        groups.push(egid);
    }
    let groups: Vec<_> = groups
        .iter()
        .map(|&g| v8::Integer::new_from_unsigned(scope, g).into())
        .collect();
    let groups = v8::Array::new_with_elements(scope, &groups);
    let items = [
        v8::Integer::new_from_unsigned(scope, uid).into(),
        v8::Integer::new_from_unsigned(scope, euid).into(),
        v8::Integer::new_from_unsigned(scope, gid).into(),
        v8::Integer::new_from_unsigned(scope, egid).into(),
        groups.into(),
    ];
    rv.set(v8::Array::new_with_elements(scope, &items).into());
}

/// `umask(mask)`: sets the file mode creation mask and returns the old
/// one, or with no mask returns the current one (node_process_methods.cc).
pub fn umask(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let mask = args.get(0);
    // SAFETY: umask can't fail.
    let old = if mask.is_undefined() {
        let old = unsafe { libc::umask(0) };
        unsafe { libc::umask(old) };
        old
    } else {
        let mask = mask.uint32_value(scope).unwrap_or(0) as libc::mode_t;
        unsafe { libc::umask(mask) }
    };
    rv.set_uint32(old as u32);
}

/// `chdir(directory)`: changes the working directory, or throws Node's
/// error, which names the old one and the new one.
pub fn chdir(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let directory = args.get(0).to_rust_string_lossy(scope);
    if let Err(error) = std::env::set_current_dir(&directory) {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let errno = -error.raw_os_error().unwrap_or(libc::EIO);
        fs::throw_uv(
            scope,
            errno,
            "chdir",
            Some(cwd.as_bytes()),
            Some(directory.as_bytes()),
        );
    }
}

/// `sleep(ms)`: internalBinding('util').sleep, which blocks the thread.
pub fn sleep(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let ms = args.get(0).number_value(scope).unwrap_or(0.0);
    if ms > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(ms / 1000.0));
    }
}

/// `runMicrotasks()`: runs V8's microtask queue until it is empty.
pub fn run_microtasks(scope: &mut v8::PinScope, _: Args, _: Ret) {
    scope.perform_microtask_checkpoint();
}

/// `takeUnhandledRejection()`: `[reason, promise]` for the oldest promise
/// rejected with no handler that still has none, or undefined.
pub fn take_unhandled_rejection(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    let next = scope
        .get_slot_mut::<Rejections>()
        .and_then(|rejections| (!rejections.0.is_empty()).then(|| rejections.0.remove(0)));
    if let Some((promise, reason)) = next {
        let pair = [
            v8::Local::new(scope, reason),
            v8::Local::new(scope, promise).into(),
        ];
        rv.set(v8::Array::new_with_elements(scope, &pair).into());
    }
}

/// `compileFunction(source, filename, params)`: compiles `source` as the body
/// of a function taking `params`, with stack traces and errors naming
/// `filename` at the right line and column. The CommonJS wrapper, without
/// gluing text around the source (which would shift column 1 of line 1).
pub fn compile_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let (Ok(source), Ok(filename), Ok(params)) = (
        v8::Local::<v8::String>::try_from(args.get(0)),
        v8::Local::<v8::String>::try_from(args.get(1)),
        v8::Local::<v8::Array>::try_from(args.get(2)),
    ) else {
        return type_error(scope, "compileFunction(source, filename, params)");
    };
    let mut names = Vec::with_capacity(params.length() as usize);
    for i in 0..params.length() {
        let Some(name) = params.get_index(scope, i) else {
            return;
        };
        let Ok(name) = v8::Local::<v8::String>::try_from(name) else {
            return type_error(scope, "compileFunction: params must be strings");
        };
        names.push(name);
    }
    let origin = v8::ScriptOrigin::new(
        scope,
        filename.into(),
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
    if let Some(function) = v8::script_compiler::compile_function(
        scope,
        &mut source,
        &names,
        &[],
        v8::script_compiler::CompileOptions::NoCompileOptions,
        v8::script_compiler::NoCacheReason::NoReason,
    ) {
        rv.set(function.into());
    }
}

/// `compileBuiltin(id, params?)`: the built-in module `id` (js/node/<id>.js
/// or js/opal/<id>.js), compiled as the body of a function of `params`
/// (BUILTIN_PARAMS by default), or undefined when there is no such module.
/// Its source is embedded in the binary, with the code cache build.rs made
/// for it, which is used when it is compiled as a module.
pub fn compile_builtin<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let id = args.get(0).to_rust_string_lossy(scope);
    let Some(source) = builtin_source(&id) else {
        return;
    };
    let Some(source) = builtin_source_string(scope, source) else {
        return type_error(scope, "built-in source is too large");
    };
    let mut params = Vec::new();
    match v8::Local::<v8::Array>::try_from(args.get(1)) {
        Ok(list) => {
            for i in 0..list.length() {
                let Some(Ok(name)) = list
                    .get_index(scope, i)
                    .map(v8::Local::<v8::String>::try_from)
                else {
                    return type_error(scope, "compileBuiltin: params must be strings");
                };
                params.push(name);
            }
        }
        Err(_) => params.extend(BUILTIN_PARAMS.iter().map(|name| string(scope, name))),
    }
    // A cache holds code compiled for BUILTIN_PARAMS, so a per-context
    // script, compiled with its own, gets none.
    let cache = if args.get(1).is_array() {
        None
    } else {
        builtin_code_cache(&id)
    };
    if let Some(function) = compile_builtin_source(scope, &id, source, &params, cache) {
        rv.set(function.into());
    }
}

/// `stripTypes(source)`: a TypeScript file's JavaScript, its types replaced
/// by whitespace, or `[code, message, line, snippet]` for TypeScript that
/// can't be stripped. The runtime's own function, not build.rs's: making the
/// snapshot never strips anything.
pub fn strip_types<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue<'s, v8::Value>,
) {
    strip_types_op(scope, args, rv)
}

/// `resolve(specifier, parent, mode)`: where `require` (mode 0) or `import`
/// (mode 1) of `specifier` from the module at `parent` leads, by Node's
/// rules (opal_core::graph::node_resolution): a path, or an array saying
/// why not. The runtime's own function, like stripTypes.
pub fn resolve<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue<'s, v8::Value>,
) {
    resolve_op(scope, args, rv)
}

/// `takeResolutionWarnings()`: the warnings resolution emitted since the
/// last call, as `[message, type, code]` arrays.
pub fn take_resolution_warnings<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue<'s, v8::Value>,
) {
    take_resolution_warnings_op(scope, args, rv)
}

/// `packageType(path)`: `[type, packageJsonPath]` for the package scope
/// `path` is in, type "module", "commonjs" or "none", which decides how a
/// .js file runs.
pub fn package_type<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue<'s, v8::Value>,
) {
    package_type_op(scope, args, rv)
}

// The ES module ops, the runtime's own (engine/esm.rs); build.rs stands in.
macro_rules! esm_ops {
    ($($name:ident => $target:ident),* $(,)?) => {
        $(
            pub fn $name<'s>(
                scope: &mut v8::PinScope<'s, '_>,
                args: v8::FunctionCallbackArguments<'s>,
                rv: v8::ReturnValue<'s, v8::Value>,
            ) {
                esm::$target(scope, args, rv)
            }
        )*
    };
}
esm_ops!(
    esm_compile => compile,
    esm_synthetic => synthetic,
    esm_set_export => set_export,
    esm_link => link,
    esm_instantiate => instantiate,
    esm_evaluate => evaluate,
    esm_namespace => namespace,
    esm_status => status,
);

/// `errorArrow(error)`: the `file:line`, source line and carets V8 has for
/// where `error` was thrown, as the fatal report prints them; for an error
/// whose stack should carry them (Node's decorateErrorStack).
pub fn error_arrow<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue<'s, v8::Value>,
) {
    error_arrow_op(scope, args, rv)
}

/// `readFile(path)`: a file's contents as a string, decoded as UTF-8 with
/// invalid sequences replaced, as `fs.readFileSync(path, 'utf8')` does.
pub fn read_file(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let path = args.get(0).to_rust_string_lossy(scope);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let Some(text) = v8::String::new(scope, &text) else {
                return type_error(scope, "file is too large for a string");
            };
            rv.set(text.into());
        }
        Err(error) => throw_io(scope, &error, "open", &path),
    }
}

macro_rules! constants {
    ($scope:ident, $object:ident, $($(#[$cfg:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$cfg])*
            {
                let value = v8::Integer::new($scope, libc::$name as i32).into();
                set($scope, $object, stringify!($name), value);
            }
        )*
    };
}

/// `osConstants()`: internalBinding('constants').os, the platform's signal
/// and errno numbers by name, in the order Node lists them
/// (node_constants.cc), with its priority and dlopen constants.
pub fn os_constants(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    let signals = v8::Object::new(scope);
    constants!(
        scope,
        signals,
        SIGHUP,
        SIGINT,
        SIGQUIT,
        SIGILL,
        SIGTRAP,
        SIGABRT,
        SIGIOT,
        SIGBUS,
        SIGFPE,
        SIGKILL,
        SIGUSR1,
        SIGSEGV,
        SIGUSR2,
        SIGPIPE,
        SIGALRM,
        SIGTERM,
        SIGCHLD,
        #[cfg(target_os = "linux")]
        SIGSTKFLT,
        SIGCONT,
        SIGSTOP,
        SIGTSTP,
        SIGTTIN,
        SIGTTOU,
        SIGURG,
        SIGXCPU,
        SIGXFSZ,
        SIGVTALRM,
        SIGPROF,
        SIGWINCH,
        SIGIO,
        #[cfg(target_os = "linux")]
        SIGPOLL,
        #[cfg(target_os = "linux")]
        SIGPWR,
        #[cfg(target_os = "macos")]
        SIGINFO,
        SIGSYS,
    );
    let errno = v8::Object::new(scope);
    constants!(
        scope,
        errno,
        E2BIG,
        EACCES,
        EADDRINUSE,
        EADDRNOTAVAIL,
        EAFNOSUPPORT,
        EAGAIN,
        EALREADY,
        EBADF,
        EBADMSG,
        EBUSY,
        ECANCELED,
        ECHILD,
        ECONNABORTED,
        ECONNREFUSED,
        ECONNRESET,
        EDEADLK,
        EDESTADDRREQ,
        EDOM,
        EDQUOT,
        EEXIST,
        EFAULT,
        EFBIG,
        EHOSTUNREACH,
        EIDRM,
        EILSEQ,
        EINPROGRESS,
        EINTR,
        EINVAL,
        EIO,
        EISCONN,
        EISDIR,
        ELOOP,
        EMFILE,
        EMLINK,
        EMSGSIZE,
        EMULTIHOP,
        ENAMETOOLONG,
        ENETDOWN,
        ENETRESET,
        ENETUNREACH,
        ENFILE,
        ENOBUFS,
        ENODATA,
        ENODEV,
        ENOENT,
        ENOEXEC,
        ENOLCK,
        ENOLINK,
        ENOMEM,
        ENOMSG,
        ENOPROTOOPT,
        ENOSPC,
        ENOSR,
        ENOSTR,
        ENOSYS,
        ENOTCONN,
        ENOTDIR,
        ENOTEMPTY,
        ENOTSOCK,
        ENOTSUP,
        ENOTTY,
        ENXIO,
        EOPNOTSUPP,
        EOVERFLOW,
        EPERM,
        EPIPE,
        EPROTO,
        EPROTONOSUPPORT,
        EPROTOTYPE,
        ERANGE,
        EROFS,
        ESPIPE,
        ESRCH,
        ESTALE,
        ETIME,
        ETIMEDOUT,
        ETXTBSY,
        EWOULDBLOCK,
        EXDEV,
    );
    let priority = v8::Object::new(scope);
    for (name, value) in [
        ("PRIORITY_LOW", 19),
        ("PRIORITY_BELOW_NORMAL", 10),
        ("PRIORITY_NORMAL", 0),
        ("PRIORITY_ABOVE_NORMAL", -7),
        ("PRIORITY_HIGH", -14),
        ("PRIORITY_HIGHEST", -20),
    ] {
        let value = v8::Integer::new(scope, value).into();
        set(scope, priority, name, value);
    }
    let dlopen = v8::Object::new(scope);
    constants!(
        scope,
        dlopen,
        RTLD_LAZY,
        RTLD_NOW,
        RTLD_GLOBAL,
        RTLD_LOCAL,
        #[cfg(target_os = "linux")]
        RTLD_DEEPBIND,
    );
    let os = v8::Object::new(scope);
    let udp = v8::Integer::new(scope, 4).into();
    set(scope, os, "UV_UDP_REUSEADDR", udp);
    set(scope, os, "dlopen", dlopen.into());
    set(scope, os, "errno", errno.into());
    set(scope, os, "signals", signals.into());
    set(scope, os, "priority", priority.into());
    rv.set(os.into());
}

/// `guessHandleType(fd)`: what libuv's uv_guess_handle says `fd` is, as
/// Node spells it: 'TTY', 'FILE', 'PIPE', 'TCP', 'UDP' or 'UNKNOWN'.
pub fn guess_handle_type(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let fd = args.get(0).int32_value(scope).unwrap_or(-1);
    // SAFETY: an all-zero stat is a valid value for fstat to overwrite.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat on any integer is defined; it fails on a bad one.
    let kind = if fd < 0 || unsafe { libc::fstat(fd, &mut stat) } != 0 {
        "UNKNOWN"
    } else {
        match stat.st_mode & libc::S_IFMT {
            // SAFETY: isatty accepts any integer.
            libc::S_IFCHR if unsafe { libc::isatty(fd) } == 1 => "TTY",
            libc::S_IFREG | libc::S_IFCHR => "FILE",
            libc::S_IFIFO => "PIPE",
            libc::S_IFSOCK => socket_kind(fd),
            _ => "UNKNOWN",
        }
    };
    rv.set(string(scope, kind).into());
}

fn socket_kind(fd: i32) -> &'static str {
    let mut kind: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: SO_TYPE writes one int into `kind`.
    let ok = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut len,
        )
    } == 0;
    match (ok, kind) {
        (true, libc::SOCK_STREAM) => "TCP",
        (true, libc::SOCK_DGRAM) => "UDP",
        _ => "UNKNOWN",
    }
}

/// `ownNonIndexProperties(object, filter)`: internalBinding('util')
/// .getOwnNonIndexProperties, the own keys that are not array indices,
/// filtered by V8's PropertyFilter bits (ONLY_ENUMERABLE is 2).
pub fn own_non_index_properties(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
        return;
    };
    let bits = args.get(1).uint32_value(scope).unwrap_or(0);
    let mut filter = v8::PropertyFilter::ALL_PROPERTIES;
    for flag in [
        v8::PropertyFilter::ONLY_WRITABLE,
        v8::PropertyFilter::ONLY_ENUMERABLE,
        v8::PropertyFilter::ONLY_CONFIGURABLE,
        v8::PropertyFilter::SKIP_STRINGS,
        v8::PropertyFilter::SKIP_SYMBOLS,
    ]
    .into_iter()
    .enumerate()
    .filter(|(bit, _)| bits & (1 << bit) != 0)
    .map(|(_, flag)| flag)
    {
        filter = filter | flag;
    }
    let names = object.get_property_names(
        scope,
        v8::GetPropertyNamesArgs {
            mode: v8::KeyCollectionMode::OwnOnly,
            property_filter: filter,
            index_filter: v8::IndexFilter::SkipIndices,
            key_conversion: v8::KeyConversionMode::KeepNumbers,
        },
    );
    if let Some(names) = names {
        rv.set(names.into());
    }
}

/// `promiseDetails(promise)`: internalBinding('util').getPromiseDetails,
/// `[state]` while pending and `[state, result]` after; state 0 is pending,
/// 1 fulfilled, 2 rejected.
pub fn promise_details(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Ok(promise) = v8::Local::<v8::Promise>::try_from(args.get(0)) else {
        return;
    };
    let state = promise.state();
    let code = match state {
        v8::PromiseState::Pending => 0,
        v8::PromiseState::Fulfilled => 1,
        v8::PromiseState::Rejected => 2,
    };
    let code = v8::Integer::new(scope, code).into();
    let details = match state {
        v8::PromiseState::Pending => v8::Array::new_with_elements(scope, &[code]),
        _ => {
            let result = promise.result(scope);
            v8::Array::new_with_elements(scope, &[code, result])
        }
    };
    rv.set(details.into());
}

/// `proxyDetails(value, showProxy = true)`: internalBinding('util')
/// .getProxyDetails: `[target, handler]`, or just the target when
/// `showProxy` is false, or undefined when `value` is not a proxy.
pub fn proxy_details(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Ok(proxy) = v8::Local::<v8::Proxy>::try_from(args.get(0)) else {
        return;
    };
    let target = proxy.get_target(scope);
    if args.length() > 1 && !args.get(1).boolean_value(scope) {
        rv.set(target);
        return;
    }
    let handler = proxy.get_handler(scope);
    rv.set(v8::Array::new_with_elements(scope, &[target, handler]).into());
}

/// `previewEntries(value, isIterator)`: internalBinding('util')
/// .previewEntries: the entries of a Map, Set or one of their iterators
/// without running it, as `[entries, isKeyValue]` when the second argument
/// is true and as `entries` otherwise.
pub fn preview_entries(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
        return;
    };
    let (entries, is_key_value) = object.preview_entries(scope);
    let Some(entries) = entries else {
        return;
    };
    if args.length() < 2 || !args.get(1).boolean_value(scope) {
        rv.set(entries.into());
        return;
    }
    let is_key_value = v8::Boolean::new(scope, is_key_value).into();
    rv.set(v8::Array::new_with_elements(scope, &[entries.into(), is_key_value]).into());
}

/// `errorSourcePositions(error)`: internalBinding('errors')
/// .getErrorSourcePositions, where V8 places an error: the line of source
/// it was thrown from, the script's name, and the line and start column
/// (node_errors.cc). assert.ok quotes the expression from it.
pub fn error_source_positions(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let message = v8::Exception::create_message(scope, args.get(0));
    let object = v8::Object::new(scope);
    let line: v8::Local<v8::Value> = match message.get_source_line(scope) {
        Some(line) => line.into(),
        None => v8::undefined(scope).into(),
    };
    set(scope, object, "sourceLine", line);
    let resource = message
        .get_script_resource_name(scope)
        .unwrap_or_else(|| v8::undefined(scope).into());
    set(scope, object, "scriptResourceName", resource);
    let number = v8::Integer::new(scope, message.get_line_number(scope).unwrap_or(0) as i32).into();
    set(scope, object, "lineNumber", number);
    let column = v8::Integer::new(scope, message.get_start_column() as i32).into();
    set(scope, object, "startColumn", column);
    rv.set(object.into());
}

/// `callSites(frameCount)`: internalBinding('util').getCallSites, the
/// caller's stack as plain objects, without the frame of the function that
/// asked (node_util.cc).
pub fn call_sites(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let count = args.get(0).int32_value(scope).unwrap_or(10).max(0) as usize;
    let Some(trace) = v8::StackTrace::current_stack_trace(scope, count + 1) else {
        return rv.set(v8::Array::new(scope, 0).into());
    };
    let names = [
        "functionName",
        "scriptId",
        "scriptName",
        "lineNumber",
        "columnNumber",
        "column",
    ]
    .map(|name| v8::Local::<v8::Name>::from(string(scope, name)));
    let mut sites = Vec::new();
    for i in 1..trace.get_frame_count() {
        let Some(frame) = trace.get_frame(scope, i) else {
            continue;
        };
        let empty = || v8::String::empty(scope);
        let function = frame.get_function_name(scope).unwrap_or_else(empty);
        let script = frame.get_script_name(scope).unwrap_or_else(empty);
        let id = string(scope, &frame.get_script_id().to_string());
        let line = v8::Integer::new(scope, frame.get_line_number() as i32);
        let column = v8::Integer::new(scope, frame.get_column() as i32);
        let values: [v8::Local<v8::Value>; 6] = [
            function.into(),
            id.into(),
            script.into(),
            line.into(),
            column.into(),
            column.into(),
        ];
        let null = v8::null(scope).into();
        sites.push(v8::Object::with_prototype_and_properties(scope, null, &names, &values).into());
    }
    rv.set(v8::Array::new_with_elements(scope, &sites).into());
}

/// `constructorName(object)`: internalBinding('util').getConstructorName,
/// V8's class name for the object.
pub fn constructor_name(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) else {
        return type_error(scope, "The \"object\" argument must be of type object.");
    };
    let name = object.get_constructor_name();
    rv.set(v8::Local::new(scope, name).into());
}

macro_rules! type_checks {
    ($($name:ident: $check:expr),* $(,)?) => {
        $(
            pub fn $name(_: &mut v8::PinScope, args: Args, mut rv: Ret) {
                let check: fn(&v8::Value) -> bool = $check;
                rv.set_bool(check(&args.get(0)));
            }
        )*
    };
}
for_each_type_check!(type_checks);
