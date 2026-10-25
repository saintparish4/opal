//! internalBinding('fs'): the synchronous calls lib/fs.js (vendored
//! unmodified in js/node/) makes for its *Sync functions, with the semantics
//! of Node's src/node_file.cc. Each makes the system call libuv's uv_fs_*
//! makes for it on unix, and a failure throws what Node's UVException
//! throws: `ENOENT: no such file or directory, open 'x'`, with `errno`,
//! `code`, `syscall`, and `path` and `dest` when the call has them, in that
//! order. The callback and promise halves of fs are not here.

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;

use super::buffer::{Encoding, bytes, decode, encode};
use super::{Args, Ret, set, string, type_error};

/// libuv's error names and messages (uv-common.c's UV_ERRNO_MAP), by
/// libuv's error number: the negated errno on unix, and libuv's own numbers
/// for errors that aren't the system's.
const UV_ERRORS: &[(i32, &str, &str)] = &[
    (-libc::E2BIG, "E2BIG", "argument list too long"),
    (-libc::EACCES, "EACCES", "permission denied"),
    (-libc::EADDRINUSE, "EADDRINUSE", "address already in use"),
    (
        -libc::EADDRNOTAVAIL,
        "EADDRNOTAVAIL",
        "address not available",
    ),
    (
        -libc::EAFNOSUPPORT,
        "EAFNOSUPPORT",
        "address family not supported",
    ),
    (-libc::EAGAIN, "EAGAIN", "resource temporarily unavailable"),
    (
        -libc::EALREADY,
        "EALREADY",
        "connection already in progress",
    ),
    (-libc::EBADF, "EBADF", "bad file descriptor"),
    (-libc::EBUSY, "EBUSY", "resource busy or locked"),
    (-libc::ECANCELED, "ECANCELED", "operation canceled"),
    (
        -libc::ECONNABORTED,
        "ECONNABORTED",
        "software caused connection abort",
    ),
    (-libc::ECONNREFUSED, "ECONNREFUSED", "connection refused"),
    (-libc::ECONNRESET, "ECONNRESET", "connection reset by peer"),
    (
        -libc::EDESTADDRREQ,
        "EDESTADDRREQ",
        "destination address required",
    ),
    (-libc::EEXIST, "EEXIST", "file already exists"),
    (
        -libc::EFAULT,
        "EFAULT",
        "bad address in system call argument",
    ),
    (-libc::EFBIG, "EFBIG", "file too large"),
    (-libc::EHOSTDOWN, "EHOSTDOWN", "host is down"),
    (-libc::EHOSTUNREACH, "EHOSTUNREACH", "host is unreachable"),
    (-libc::EILSEQ, "EILSEQ", "illegal byte sequence"),
    (-libc::EINTR, "EINTR", "interrupted system call"),
    (-libc::EINVAL, "EINVAL", "invalid argument"),
    (-libc::EIO, "EIO", "i/o error"),
    (-libc::EISCONN, "EISCONN", "socket is already connected"),
    (-libc::EISDIR, "EISDIR", "illegal operation on a directory"),
    (-libc::ELOOP, "ELOOP", "too many symbolic links encountered"),
    (-libc::EMFILE, "EMFILE", "too many open files"),
    (-libc::EMLINK, "EMLINK", "too many links"),
    (-libc::EMSGSIZE, "EMSGSIZE", "message too long"),
    (-libc::ENAMETOOLONG, "ENAMETOOLONG", "name too long"),
    (-libc::ENETDOWN, "ENETDOWN", "network is down"),
    (-libc::ENETUNREACH, "ENETUNREACH", "network is unreachable"),
    (-libc::ENFILE, "ENFILE", "file table overflow"),
    (-libc::ENOBUFS, "ENOBUFS", "no buffer space available"),
    (-libc::ENODATA, "ENODATA", "no data available"),
    (-libc::ENODEV, "ENODEV", "no such device"),
    (-libc::ENOENT, "ENOENT", "no such file or directory"),
    (-libc::ENOEXEC, "ENOEXEC", "exec format error"),
    (-libc::ENOMEM, "ENOMEM", "not enough memory"),
    #[cfg(target_os = "linux")]
    (-libc::ENONET, "ENONET", "machine is not on the network"),
    (-libc::ENOPROTOOPT, "ENOPROTOOPT", "protocol not available"),
    (-libc::ENOSPC, "ENOSPC", "no space left on device"),
    (-libc::ENOSYS, "ENOSYS", "function not implemented"),
    (-libc::ENOTCONN, "ENOTCONN", "socket is not connected"),
    (-libc::ENOTDIR, "ENOTDIR", "not a directory"),
    (-libc::ENOTEMPTY, "ENOTEMPTY", "directory not empty"),
    (
        -libc::ENOTSOCK,
        "ENOTSOCK",
        "socket operation on non-socket",
    ),
    (
        -libc::ENOTSUP,
        "ENOTSUP",
        "operation not supported on socket",
    ),
    (-libc::ENOTTY, "ENOTTY", "inappropriate ioctl for device"),
    (-libc::ENXIO, "ENXIO", "no such device or address"),
    (
        -libc::EOVERFLOW,
        "EOVERFLOW",
        "value too large for defined data type",
    ),
    (-libc::EPERM, "EPERM", "operation not permitted"),
    (-libc::EPIPE, "EPIPE", "broken pipe"),
    (-libc::EPROTO, "EPROTO", "protocol error"),
    (
        -libc::EPROTONOSUPPORT,
        "EPROTONOSUPPORT",
        "protocol not supported",
    ),
    (
        -libc::EPROTOTYPE,
        "EPROTOTYPE",
        "protocol wrong type for socket",
    ),
    (-libc::ERANGE, "ERANGE", "result too large"),
    #[cfg(target_os = "linux")]
    (-libc::EREMOTEIO, "EREMOTEIO", "remote I/O error"),
    (-libc::EROFS, "EROFS", "read-only file system"),
    (
        -libc::ESHUTDOWN,
        "ESHUTDOWN",
        "cannot send after transport endpoint shutdown",
    ),
    (
        -libc::ESOCKTNOSUPPORT,
        "ESOCKTNOSUPPORT",
        "socket type not supported",
    ),
    (-libc::ESPIPE, "ESPIPE", "invalid seek"),
    (-libc::ESRCH, "ESRCH", "no such process"),
    (-libc::ETIMEDOUT, "ETIMEDOUT", "connection timed out"),
    (-libc::ETXTBSY, "ETXTBSY", "text file is busy"),
    #[cfg(target_os = "linux")]
    (-libc::EUNATCH, "EUNATCH", "protocol driver not attached"),
    (-libc::EXDEV, "EXDEV", "cross-device link not permitted"),
    (-3000, "EAI_ADDRFAMILY", "address family not supported"),
    (-3001, "EAI_AGAIN", "temporary failure"),
    (-3002, "EAI_BADFLAGS", "bad ai_flags value"),
    (-3003, "EAI_CANCELED", "request canceled"),
    (-3004, "EAI_FAIL", "permanent failure"),
    (-3005, "EAI_FAMILY", "ai_family not supported"),
    (-3006, "EAI_MEMORY", "out of memory"),
    (-3007, "EAI_NODATA", "no address"),
    (-3008, "EAI_NONAME", "unknown node or service"),
    (-3009, "EAI_OVERFLOW", "argument buffer overflow"),
    (
        -3010,
        "EAI_SERVICE",
        "service not available for socket type",
    ),
    (-3011, "EAI_SOCKTYPE", "socket type not supported"),
    (-3013, "EAI_BADHINTS", "invalid value for hints"),
    (-3014, "EAI_PROTOCOL", "resolved protocol is unknown"),
    #[cfg(target_os = "macos")]
    (-libc::EFTYPE, "EFTYPE", "inappropriate file type or format"),
    #[cfg(not(target_os = "macos"))]
    (-4028, "EFTYPE", "inappropriate file type or format"),
    (-4080, "ECHARSET", "invalid Unicode character"),
    (-4094, "UNKNOWN", "unknown error"),
    (-4095, "EOF", "end of file"),
];

/// libuv's name and message for one of its error numbers.
pub(super) fn uv_error(error: i32) -> (&'static str, &'static str) {
    UV_ERRORS
        .iter()
        .find(|(n, _, _)| *n == error)
        .map_or(("UNKNOWN", "unknown error"), |&(_, name, message)| {
            (name, message)
        })
}

/// `uvErrors()`: [[number, name, message], ...], for internalBinding('uv').
pub fn uv_errors(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    let mut rows = Vec::with_capacity(UV_ERRORS.len());
    for &(number, name, message) in UV_ERRORS {
        let row = [
            v8::Integer::new(scope, number).into(),
            string(scope, name).into(),
            string(scope, message).into(),
        ];
        rows.push(v8::Array::new_with_elements(scope, &row).into());
    }
    rv.set(v8::Array::new_with_elements(scope, &rows).into());
}

/// libuv's error number for the last system call's failure.
fn last_error() -> i32 {
    -std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

/// A system call's result, or libuv's error number when it failed (-1).
fn check<T: Into<i64> + Copy>(result: T) -> Result<T, i32> {
    if result.into() < 0 {
        Err(last_error())
    } else {
        Ok(result)
    }
}

/// Retries a call that EINTR interrupted, as libuv does for reads and
/// writes.
fn retry<T: Into<i64> + Copy>(mut call: impl FnMut() -> T) -> Result<T, i32> {
    loop {
        match check(call()) {
            Err(e) if e == -libc::EINTR => continue,
            other => return other,
        }
    }
}

/// Throws Node's UVException: `CODE: message, syscall 'path' -> 'dest'`.
pub(super) fn throw_uv(
    scope: &mut v8::PinScope,
    error: i32,
    syscall: &str,
    path: Option<&[u8]>,
    dest: Option<&[u8]>,
) {
    let (code, description) = uv_error(error);
    let path = path.map(String::from_utf8_lossy);
    let dest = dest.map(String::from_utf8_lossy);
    let mut text = format!("{code}: {description}, {syscall}");
    if let Some(path) = &path {
        text.push_str(&format!(" '{path}'"));
    }
    if let Some(dest) = &dest {
        text.push_str(&format!(" -> '{dest}'"));
    }
    let message = string(scope, &text);
    let exception = v8::Exception::error(scope, message);
    let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
    let value = v8::Integer::new(scope, error).into();
    set(scope, object, "errno", value);
    let value = string(scope, code).into();
    set(scope, object, "code", value);
    let value = string(scope, syscall).into();
    set(scope, object, "syscall", value);
    if let Some(path) = &path {
        let value = string(scope, path).into();
        set(scope, object, "path", value);
    }
    if let Some(dest) = &dest {
        let value = string(scope, dest).into();
        set(scope, object, "dest", value);
    }
    scope.throw_exception(exception);
}

/// Throws Node's ErrnoException, which rmSync uses: `CODE, message 'path'`
/// with the positive errno.
fn throw_errno(scope: &mut v8::PinScope, errno: i32, syscall: &str, message: &str, path: &[u8]) {
    let (code, _) = uv_error(-errno);
    let path = String::from_utf8_lossy(path);
    let text = format!("{code}, {message} '{path}'");
    let text = string(scope, &text);
    let exception = v8::Exception::error(scope, text);
    let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
    let value = v8::Integer::new(scope, errno).into();
    set(scope, object, "errno", value);
    let value = string(scope, code).into();
    set(scope, object, "code", value);
    let value = string(scope, syscall).into();
    set(scope, object, "syscall", value);
    let value = string(scope, &path).into();
    set(scope, object, "path", value);
    scope.throw_exception(exception);
}

/// A path argument's bytes: a string as UTF-8 (lone surrogates replaced,
/// as Node's BufferValue does), or a Buffer's bytes.
fn path_arg(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Vec<u8> {
    if value.is_array_buffer_view() {
        return bytes(scope, value).map(|b| b.to_vec()).unwrap_or_default();
    }
    match value.to_string(scope) {
        Some(text) => encode(scope, text, Encoding::Utf8),
        None => Vec::new(),
    }
}

/// A path as the C string a system call takes. lib/fs.js refuses paths with
/// NUL bytes before they get here.
fn c_path(path: &[u8]) -> CString {
    CString::new(path).unwrap_or_default()
}

/// Throws one of Node's C++ errors (node_errors.h's THROW_ERR_*): a
/// TypeError or RangeError with a `code`.
fn throw_node_error(scope: &mut v8::PinScope, range: bool, code: &str, message: &str) {
    let message = string(scope, message);
    let exception = if range {
        v8::Exception::range_error(scope, message)
    } else {
        v8::Exception::type_error(scope, message)
    };
    let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
    let value = string(scope, code).into();
    set(scope, object, "code", value);
    scope.throw_exception(exception);
}

/// How node_errors.cc's DetermineSpecificErrorType describes a value it
/// didn't expect.
fn describe(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> String {
    if value.is_function() {
        return "function".to_string();
    }
    if value.is_string() {
        let mut text = value.to_rust_string_lossy(scope);
        if text.chars().count() > 28 {
            text = text.chars().take(25).collect::<String>() + "...";
        }
        if !text.contains('\'') {
            return format!("type string ('{text}')");
        }
        let quoted = v8::String::new(scope, &text)
            .and_then(|t| v8::json::stringify(scope, t.into()))
            .map_or(text, |t| t.to_rust_string_lossy(scope));
        return format!("type string ({quoted})");
    }
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
        let name = object.get_constructor_name().to_rust_string_lossy(scope);
        return format!("an instance of {name}");
    }
    if let Ok(symbol) = v8::Local::<v8::Symbol>::try_from(value) {
        let description = symbol.description(scope);
        if description.is_undefined() {
            return "Symbol()".to_string();
        }
        return format!("Symbol({})", description.to_rust_string_lossy(scope));
    }
    if value.is_null_or_undefined() {
        return value.to_rust_string_lossy(scope);
    }
    let text = value.to_rust_string_lossy(scope);
    let kind = value.type_of(scope).to_rust_string_lossy(scope);
    format!("type {kind} ({text})")
}

/// A file descriptor argument, or a thrown ERR_INVALID_ARG_TYPE or
/// ERR_OUT_OF_RANGE: node_file.cc's GetValidatedFd, which the fd calls
/// check for themselves.
fn fd_arg(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<i32> {
    if !value.is_number() {
        let received = describe(scope, value);
        let message = format!("The \"fd\" argument must be of type number. Received {received}");
        throw_node_error(scope, false, "ERR_INVALID_ARG_TYPE", &message);
        return None;
    }
    let fd = value.number_value(scope).unwrap_or(f64::NAN);
    let out_of_range = fd < 0.0 || fd > i32::MAX as f64;
    if out_of_range || fd.fract() != 0.0 || fd.is_nan() {
        let must = if out_of_range && !fd.is_infinite() {
            ">= 0 && <= 2147483647"
        } else {
            "an integer"
        };
        let received = value.to_rust_string_lossy(scope);
        let message =
            format!("The value of \"fd\" is out of range. It must be {must}. Received {received}");
        throw_node_error(scope, true, "ERR_OUT_OF_RANGE", &message);
        return None;
    }
    Some(fd as i32)
}

/// The mode argument of access and copyFile: node_file.cc's
/// GetValidFileMode, with `default` for null or undefined.
fn mode_arg(scope: &mut v8::PinScope, value: v8::Local<v8::Value>, default: i32) -> Option<i32> {
    if value.is_null_or_undefined() {
        return Some(default);
    }
    if !value.is_number() {
        throw_node_error(
            scope,
            false,
            "ERR_INVALID_ARG_TYPE",
            "mode must be int32 or null/undefined",
        );
        return None;
    }
    let mode = int(scope, value);
    if !(0..=7).contains(&mode) {
        throw_node_error(
            scope,
            true,
            "ERR_OUT_OF_RANGE",
            "mode is out of range: >= 0 && <= 7",
        );
        return None;
    }
    Some(mode)
}

/// Whether the call is the callback or promise form: a request object (or
/// kUsePromises) at `index`, where node_file.cc looks for one. Those need
/// file I/O on the event loop, which Opal's doesn't have yet, so this throws
/// an error saying so, once the arguments have been checked as Node checks
/// them, so that a bad argument is still Node's error.
fn asynchronous(scope: &mut v8::PinScope, args: &Args, index: i32) -> bool {
    if args.get(index).is_undefined() {
        return false;
    }
    let message = string(
        scope,
        "Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise",
    );
    let exception = v8::Exception::error(scope, message);
    let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
    let code = string(scope, "ERR_OPAL_UNIMPLEMENTED").into();
    set(scope, object, "code", code);
    scope.throw_exception(exception);
    true
}

fn int(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> i32 {
    value.int32_value(scope).unwrap_or(0)
}

fn uint(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> u32 {
    value.uint32_value(scope).unwrap_or(0)
}

/// A file position: a number or bigint, and -1 (the current position) for
/// anything else, as node_file.cc's GetOffset reads it.
fn position(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> i64 {
    if value.is_number() {
        value.integer_value(scope).unwrap_or(-1)
    } else if let Ok(big) = v8::Local::<v8::BigInt>::try_from(value) {
        big.i64_value().0
    } else {
        -1
    }
}

/// What the stat and statfs calls fill, and the prototype of the Buffers
/// fs returns: set up once, when internalBinding('fs') is first made.
pub struct FsState {
    stats: v8::Global<v8::Float64Array>,
    bigint_stats: v8::Global<v8::BigInt64Array>,
    statfs: v8::Global<v8::Float64Array>,
    bigint_statfs: v8::Global<v8::BigInt64Array>,
    buffer_prototype: v8::Global<v8::Object>,
}

/// `fsSetup(statValues, bigintStatValues, statFsValues, bigintStatFsValues,
/// Buffer.prototype)`.
pub fn fs_setup(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let (Ok(stats), Ok(bigint_stats), Ok(statfs), Ok(bigint_statfs), Ok(buffer_prototype)) = (
        v8::Local::<v8::Float64Array>::try_from(args.get(0)),
        v8::Local::<v8::BigInt64Array>::try_from(args.get(1)),
        v8::Local::<v8::Float64Array>::try_from(args.get(2)),
        v8::Local::<v8::BigInt64Array>::try_from(args.get(3)),
        v8::Local::<v8::Object>::try_from(args.get(4)),
    ) else {
        return type_error(scope, "fsSetup takes the stat arrays and Buffer.prototype");
    };
    let state = FsState {
        stats: v8::Global::new(scope, stats),
        bigint_stats: v8::Global::new(scope, bigint_stats),
        statfs: v8::Global::new(scope, statfs),
        bigint_statfs: v8::Global::new(scope, bigint_statfs),
        buffer_prototype: v8::Global::new(scope, buffer_prototype),
    };
    scope.set_slot(state);
}

/// A Buffer holding a copy of `data`, as Node's Buffer::Copy makes one.
fn new_buffer<'s>(scope: &mut v8::PinScope<'s, '_>, data: &[u8]) -> v8::Local<'s, v8::Value> {
    let length = data.len();
    let store = v8::ArrayBuffer::new_backing_store_from_vec(data.to_vec()).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
    let array = v8::Uint8Array::new(scope, buffer, 0, length).unwrap();
    let prototype = scope
        .get_slot::<FsState>()
        .map(|s| s.buffer_prototype.clone());
    if let Some(prototype) = prototype {
        let prototype = v8::Local::new(scope, prototype);
        array.set_prototype(scope, prototype.into());
    }
    array.into()
}

/// `bytes` in `encoding` (a Buffer for "buffer"): StringBytes::Encode.
fn encoded<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: &[u8],
    encoding: v8::Local<v8::Value>,
) -> Option<v8::Local<'s, v8::Value>> {
    match Encoding::from_value(scope, encoding) {
        Encoding::Buffer => Some(new_buffer(scope, data)),
        encoding => decode(scope, data, encoding).map(Into::into),
    }
}

/// Field indices of Node's stat arrays (node_file.h's FsStatsOffset).
const STAT_FIELDS: usize = 18;
type StatFields = [i64; STAT_FIELDS];

#[cfg(target_os = "linux")]
fn statx(dirfd: i32, path: &CStr, flags: i32) -> Result<StatFields, i32> {
    // SAFETY: statx writes a statx struct, which may be zeroed.
    let mut buf: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: path is a C string and buf a statx struct.
    let result = unsafe {
        libc::statx(
            dirfd,
            path.as_ptr(),
            flags | libc::AT_STATX_SYNC_AS_STAT,
            libc::STATX_BASIC_STATS | libc::STATX_BTIME,
            &mut buf,
        )
    };
    check(result)?;
    let dev = libc::makedev(buf.stx_dev_major, buf.stx_dev_minor);
    let rdev = libc::makedev(buf.stx_rdev_major, buf.stx_rdev_minor);
    Ok([
        dev as i64,
        buf.stx_mode as i64,
        buf.stx_nlink as i64,
        buf.stx_uid as i64,
        buf.stx_gid as i64,
        rdev as i64,
        buf.stx_blksize as i64,
        buf.stx_ino as i64,
        buf.stx_size as i64,
        buf.stx_blocks as i64,
        buf.stx_atime.tv_sec,
        buf.stx_atime.tv_nsec as i64,
        buf.stx_mtime.tv_sec,
        buf.stx_mtime.tv_nsec as i64,
        buf.stx_ctime.tv_sec,
        buf.stx_ctime.tv_nsec as i64,
        buf.stx_btime.tv_sec,
        buf.stx_btime.tv_nsec as i64,
    ])
}

/// A stat struct's fields in Node's order. Where the platform keeps no
/// birth time, libuv reports the change time.
// The fields' types differ between platforms, so a cast that is a no-op on
// one isn't on another.
#[allow(clippy::unnecessary_cast)]
fn stat_fields(s: &libc::stat) -> StatFields {
    #[cfg(target_os = "macos")]
    let birth = (s.st_birthtime, s.st_birthtime_nsec);
    #[cfg(not(target_os = "macos"))]
    let birth = (s.st_ctime, s.st_ctime_nsec);
    [
        s.st_dev as i64,
        s.st_mode as i64,
        s.st_nlink as i64,
        s.st_uid as i64,
        s.st_gid as i64,
        s.st_rdev as i64,
        s.st_blksize as i64,
        s.st_ino as i64,
        s.st_size as i64,
        s.st_blocks as i64,
        s.st_atime as i64,
        s.st_atime_nsec as i64,
        s.st_mtime as i64,
        s.st_mtime_nsec as i64,
        s.st_ctime as i64,
        s.st_ctime_nsec as i64,
        birth.0 as i64,
        birth.1 as i64,
    ]
}

/// What libuv's uv_fs_stat, uv_fs_lstat and uv_fs_fstat report. On Linux
/// that is statx, for the birth time, falling back to fstatat where statx
/// is missing or a seccomp filter refuses it, as libuv does.
enum Target<'a> {
    Path(&'a CStr, bool),
    Fd(i32),
}

fn stat(target: Target) -> Result<StatFields, i32> {
    #[cfg(target_os = "linux")]
    {
        let result = match target {
            Target::Path(path, follow) => statx(
                libc::AT_FDCWD,
                path,
                if follow { 0 } else { libc::AT_SYMLINK_NOFOLLOW },
            ),
            Target::Fd(fd) => statx(fd, c"", libc::AT_EMPTY_PATH),
        };
        match result {
            Err(e) if e == -libc::ENOSYS || e == -libc::EPERM || e == -libc::EOPNOTSUPP => {}
            other => return other,
        }
    }
    // SAFETY: the stat calls write a stat struct, which may be zeroed.
    let mut buf: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: path is a C string and buf a stat struct.
    let result = unsafe {
        match target {
            Target::Path(path, true) => libc::stat(path.as_ptr(), &mut buf),
            Target::Path(path, false) => libc::lstat(path.as_ptr(), &mut buf),
            Target::Fd(fd) => libc::fstat(fd, &mut buf),
        }
    };
    check(result)?;
    Ok(stat_fields(&buf))
}

/// Fills the shared stat array (Float64 or BigInt64) with `fields` and
/// returns it: node_file.cc's FillGlobalStatsArray.
fn fill_stats<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    fields: &[i64],
    bigint: bool,
    statfs: bool,
) -> v8::Local<'s, v8::Value> {
    let Some(state) = scope.get_slot::<FsState>() else {
        return v8::undefined(scope).into();
    };
    let (float, big) = if statfs {
        (state.statfs.clone(), state.bigint_statfs.clone())
    } else {
        (state.stats.clone(), state.bigint_stats.clone())
    };
    if bigint {
        let array = v8::Local::new(scope, big);
        for (i, &value) in fields.iter().enumerate() {
            let value = v8::BigInt::new_from_i64(scope, value).into();
            array.set_index(scope, i as u32, value);
        }
        array.into()
    } else {
        let array = v8::Local::new(scope, float);
        for (i, &value) in fields.iter().enumerate() {
            // dev and ino are unsigned in libuv's uv_stat_t.
            let value = if (i == 0 || i == 7) && value < 0 {
                value as u64 as f64
            } else {
                value as f64
            };
            let value = v8::Number::new(scope, value).into();
            array.set_index(scope, i as u32, value);
        }
        array.into()
    }
}

/// Whether a stat mode is a directory's. mode_t is 16 bits on macOS.
#[allow(clippy::unnecessary_cast)]
fn is_dir(mode: i64) -> bool {
    mode as u32 & libc::S_IFMT as u32 == libc::S_IFDIR as u32
}

/// `stat(path, useBigint, undefined, throwIfNoEntry)` and `lstat`: the
/// filled stat array, or undefined for a path that doesn't exist when
/// throwIfNoEntry is false.
fn stat_path(scope: &mut v8::PinScope, args: Args, mut rv: Ret, follow: bool) {
    let path = path_arg(scope, args.get(0));
    let bigint = args.get(1).is_true();
    let throw_if_no_entry = !args.get(3).is_false();
    match stat(Target::Path(&c_path(&path), follow)) {
        Ok(fields) => rv.set(fill_stats(scope, &fields, bigint, false)),
        Err(e) if !throw_if_no_entry && (e == -libc::ENOENT || e == -libc::ENOTDIR) => {}
        Err(e) => throw_uv(
            scope,
            e,
            if follow { "stat" } else { "lstat" },
            Some(&path),
            None,
        ),
    }
}

pub fn fs_stat(scope: &mut v8::PinScope, args: Args, rv: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    stat_path(scope, args, rv, true);
}

pub fn fs_lstat(scope: &mut v8::PinScope, args: Args, rv: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    stat_path(scope, args, rv, false);
}

/// `fstat(fd, useBigint, undefined, shouldNotThrow)`.
pub fn fs_fstat(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 2) {
        return;
    }
    let bigint = args.get(1).is_true();
    match stat(Target::Fd(fd)) {
        Ok(fields) => rv.set(fill_stats(scope, &fields, bigint, false)),
        Err(_) if args.get(3).is_true() => {}
        Err(e) => throw_uv(scope, e, "fstat", None, None),
    }
}

/// `statfs(path, useBigint)`: type, bsize, blocks, bfree, bavail, files,
/// ffree.
#[allow(clippy::unnecessary_cast)] // as in stat_fields
pub fn fs_statfs(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    let path = path_arg(scope, args.get(0));
    let bigint = args.get(1).is_true();
    // SAFETY: statfs writes a statfs struct, which may be zeroed.
    let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: the path is a C string and buf a statfs struct.
    let result = unsafe { libc::statfs(c_path(&path).as_ptr(), &mut buf) };
    if let Err(e) = check(result) {
        return throw_uv(scope, e, "statfs", Some(&path), None);
    }
    let fields = [
        buf.f_type as i64,
        buf.f_bsize as i64,
        buf.f_blocks as i64,
        buf.f_bfree as i64,
        buf.f_bavail as i64,
        buf.f_files as i64,
        buf.f_ffree as i64,
    ];
    rv.set(fill_stats(scope, &fields, bigint, true));
}

/// `internalModuleStat(path)`: 0 for a file, 1 for a directory, or the
/// negative error number.
pub fn fs_internal_module_stat(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let path = path_arg(scope, args.get(args.length() - 1));
    let result = match stat(Target::Path(&c_path(&path), true)) {
        Ok(fields) if is_dir(fields[1]) => 1,
        Ok(_) => 0,
        Err(e) => e,
    };
    rv.set_int32(result);
}

/// `access(path, mode)`.
pub fn fs_access(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let path = path_arg(scope, args.get(0));
    let Some(mode) = mode_arg(scope, args.get(1), libc::F_OK) else {
        return;
    };
    if asynchronous(scope, &args, 2) {
        return;
    }
    // SAFETY: the path is a C string.
    if let Err(e) = check(unsafe { libc::access(c_path(&path).as_ptr(), mode) }) {
        throw_uv(scope, e, "access", Some(&path), None);
    }
}

/// `existsSync(path)`.
pub fn fs_exists_sync(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let path = path_arg(scope, args.get(0));
    // SAFETY: the path is a C string.
    let result = unsafe { libc::access(c_path(&path).as_ptr(), libc::F_OK) };
    rv.set_bool(result == 0);
}

fn open(path: &[u8], flags: i32, mode: u32) -> Result<i32, i32> {
    // libuv opens every file close-on-exec.
    // SAFETY: the path is a C string.
    retry(|| unsafe { libc::open(c_path(path).as_ptr(), flags | libc::O_CLOEXEC, mode) })
}

fn close(fd: i32) {
    // SAFETY: closing a descriptor is safe; a bad one is EBADF.
    unsafe { libc::close(fd) };
}

/// `open(path, flags, mode)`: the new file descriptor.
pub fn fs_open(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if asynchronous(scope, &args, 3) {
        return;
    }
    let path = path_arg(scope, args.get(0));
    let flags = int(scope, args.get(1));
    let mode = uint(scope, args.get(2));
    match open(&path, flags, mode) {
        Ok(fd) => rv.set_int32(fd),
        Err(e) => throw_uv(scope, e, "open", Some(&path), None),
    }
}

/// `close(fd)`.
pub fn fs_close(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 1) {
        return;
    }
    // SAFETY: as in close().
    if let Err(e) = check(unsafe { libc::close(fd) }) {
        throw_uv(scope, e, "close", None, None);
    }
}

fn read(fd: i32, buf: &mut [u8], position: i64) -> Result<usize, i32> {
    // SAFETY: buf is a valid buffer of buf.len() bytes.
    let n = retry(|| unsafe {
        if position < 0 {
            libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) as i64
        } else {
            libc::pread(fd, buf.as_mut_ptr().cast(), buf.len(), position) as i64
        }
    })?;
    Ok(n as usize)
}

fn write(fd: i32, buf: &[u8], position: i64) -> Result<usize, i32> {
    // SAFETY: buf is a valid buffer of buf.len() bytes.
    let n = retry(|| unsafe {
        if position < 0 {
            libc::write(fd, buf.as_ptr().cast(), buf.len()) as i64
        } else {
            libc::pwrite(fd, buf.as_ptr().cast(), buf.len(), position) as i64
        }
    })?;
    Ok(n as usize)
}

/// The window `[offset, offset + length)` of a buffer argument, which
/// lib/fs.js has already checked lies within it.
fn window<'a>(scope: &mut v8::PinScope, args: &Args, index: i32) -> Option<&'a mut [u8]> {
    let buffer = bytes(scope, args.get(index));
    let Some(buffer) = buffer else {
        type_error(scope, "argument must be a buffer");
        return None;
    };
    let offset = args.get(index + 1).integer_value(scope).unwrap_or(0).max(0) as usize;
    let length = args.get(index + 2).integer_value(scope).unwrap_or(0).max(0) as usize;
    let start = offset.min(buffer.len());
    let end = start.saturating_add(length).min(buffer.len());
    Some(&mut buffer[start..end])
}

/// `read(fd, buffer, offset, length, position)`: bytes read.
pub fn fs_read(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 5) {
        return;
    }
    let Some(buf) = window(scope, &args, 1) else {
        return;
    };
    let position = position(scope, args.get(4));
    match read(fd, buf, position) {
        Ok(n) => rv.set_double(n as f64),
        Err(e) => throw_uv(scope, e, "read", None, None),
    }
}

/// The buffers of an array of ArrayBufferViews, as iovecs.
fn iovecs(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<Vec<libc::iovec>> {
    let array = v8::Local::<v8::Array>::try_from(value).ok()?;
    let mut out = Vec::with_capacity(array.length() as usize);
    for i in 0..array.length() {
        let item = array.get_index(scope, i)?;
        let buf = bytes(scope, item)?;
        out.push(libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        });
    }
    Some(out)
}

/// `readBuffers(fd, buffers, position)` and `writeBuffers`: readv/preadv
/// and writev/pwritev.
fn vectored(scope: &mut v8::PinScope, args: Args, mut rv: Ret, writing: bool) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 3) {
        return;
    }
    let Some(vecs) = iovecs(scope, args.get(1)) else {
        return type_error(scope, "buffers must be an array of ArrayBufferViews");
    };
    let position = position(scope, args.get(2));
    let count = vecs.len() as i32;
    // SAFETY: each iovec describes a live ArrayBufferView's bytes.
    let result = retry(|| unsafe {
        match (writing, position < 0) {
            (false, true) => libc::readv(fd, vecs.as_ptr(), count) as i64,
            (false, false) => libc::preadv(fd, vecs.as_ptr(), count, position) as i64,
            (true, true) => libc::writev(fd, vecs.as_ptr(), count) as i64,
            (true, false) => libc::pwritev(fd, vecs.as_ptr(), count, position) as i64,
        }
    });
    match result {
        Ok(n) => rv.set_double(n as f64),
        Err(e) => throw_uv(scope, e, if writing { "write" } else { "read" }, None, None),
    }
}

pub fn fs_read_buffers(scope: &mut v8::PinScope, args: Args, rv: Ret) {
    vectored(scope, args, rv, false);
}

pub fn fs_write_buffers(scope: &mut v8::PinScope, args: Args, rv: Ret) {
    vectored(scope, args, rv, true);
}

/// Reports a failed write the old way, which writeSync still uses: by
/// setting `errno` and `syscall` on the context object it passes, for its
/// handleErrorFromBinding to throw. Without one, throws.
fn report<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ctx: v8::Local<'s, v8::Value>,
    error: i32,
    syscall: &str,
) {
    let Ok(ctx) = v8::Local::<v8::Object>::try_from(ctx) else {
        return throw_uv(scope, error, syscall, None, None);
    };
    let value = v8::Integer::new(scope, error).into();
    set(scope, ctx, "errno", value);
    let value = string(scope, syscall).into();
    set(scope, ctx, "syscall", value);
}

/// `writeBuffer(fd, buffer, offset, length, position, undefined, ctx)`:
/// bytes written.
pub fn fs_write_buffer<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 5) {
        return;
    }
    let Some(buf) = window(scope, &args, 1) else {
        return;
    };
    let position = position(scope, args.get(4));
    match write(fd, buf, position) {
        Ok(n) => rv.set_double(n as f64),
        Err(e) => report(scope, args.get(6), e, "write"),
    }
}

/// `writeString(fd, string, position, encoding, undefined, ctx)`: bytes
/// written.
pub fn fs_write_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 4) {
        return;
    }
    let Some(text) = args.get(1).to_string(scope) else {
        return;
    };
    let position = position(scope, args.get(2));
    let encoding = Encoding::from_value(scope, args.get(3));
    let data = encode(scope, text, encoding);
    match write(fd, &data, position) {
        Ok(n) => rv.set_double(n as f64),
        Err(e) => report(scope, args.get(5), e, "write"),
    }
}

/// A file argument that is either a descriptor the caller owns or a path
/// to open (and close after).
fn open_arg(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    flags: i32,
    mode: u32,
) -> Option<(i32, bool)> {
    if value.is_int32() {
        return Some((int(scope, value), false));
    }
    let path = path_arg(scope, value);
    match open(&path, flags, mode) {
        Ok(fd) => Some((fd, true)),
        Err(e) => {
            throw_uv(scope, e, "open", Some(&path), None);
            None
        }
    }
}

/// `readFileUtf8(path | fd, flags)`: the whole file as a string.
pub fn fs_read_file_utf8(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    let flags = int(scope, args.get(1));
    let Some((fd, owned)) = open_arg(scope, args.get(0), flags, 0o666) else {
        return;
    };
    let mut data = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let result = loop {
        match read(fd, &mut chunk, -1) {
            Ok(0) => break Ok(()),
            Ok(n) => data.extend_from_slice(&chunk[..n]),
            Err(e) => break Err(e),
        }
    };
    if owned {
        close(fd);
    }
    if let Err(e) = result {
        return throw_uv(scope, e, "read", None, None);
    }
    match v8::String::new_from_utf8(scope, &data, v8::NewStringType::Normal) {
        Some(text) => rv.set(text.into()),
        None => {
            let message = string(
                scope,
                "Cannot create a string longer than 0x1fffffe8 characters",
            );
            let exception = v8::Exception::error(scope, message);
            let object = v8::Local::<v8::Object>::try_from(exception).unwrap();
            let code = string(scope, "ERR_STRING_TOO_LONG").into();
            set(scope, object, "code", code);
            scope.throw_exception(exception);
        }
    }
}

/// `writeFileUtf8(path | fd, data, flags, mode)`.
pub fn fs_write_file_utf8(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let Some(text) = args.get(1).to_string(scope) else {
        return;
    };
    let data = encode(scope, text, Encoding::Utf8);
    let flags = int(scope, args.get(2));
    let mode = uint(scope, args.get(3));
    let Some((fd, owned)) = open_arg(scope, args.get(0), flags, mode) else {
        return;
    };
    let mut rest = &data[..];
    let mut result = Ok(());
    while !rest.is_empty() {
        match write(fd, rest, -1) {
            Ok(n) => rest = &rest[n..],
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    if owned {
        close(fd);
    }
    if let Err(e) = result {
        throw_uv(scope, e, "write", None, None);
    }
}

/// Calls taking one path: rmdir, unlink.
fn path_call(
    scope: &mut v8::PinScope,
    args: &Args,
    syscall: &str,
    call: unsafe extern "C" fn(*const libc::c_char) -> libc::c_int,
) {
    let path = path_arg(scope, args.get(0));
    // SAFETY: the path is a C string.
    if let Err(e) = check(unsafe { call(c_path(&path).as_ptr()) }) {
        throw_uv(scope, e, syscall, Some(&path), None);
    }
}

pub fn fs_rmdir(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 1) {
        return;
    }
    path_call(scope, &args, "rmdir", libc::rmdir);
}

pub fn fs_unlink(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 1) {
        return;
    }
    path_call(scope, &args, "unlink", libc::unlink);
}

/// Calls taking two paths, whose errors name both: rename, link, symlink.
fn two_path_call(
    scope: &mut v8::PinScope,
    args: &Args,
    syscall: &str,
    call: unsafe extern "C" fn(*const libc::c_char, *const libc::c_char) -> libc::c_int,
) {
    let from = path_arg(scope, args.get(0));
    let to = path_arg(scope, args.get(1));
    // SAFETY: both paths are C strings.
    if let Err(e) = check(unsafe { call(c_path(&from).as_ptr(), c_path(&to).as_ptr()) }) {
        throw_uv(scope, e, syscall, Some(&from), Some(&to));
    }
}

/// `rename(oldPath, newPath)`.
pub fn fs_rename(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    two_path_call(scope, &args, "rename", libc::rename);
}

/// `link(existingPath, newPath)`.
pub fn fs_link(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    two_path_call(scope, &args, "link", libc::link);
}

/// `symlink(target, path, flags)`; the flags only matter on Windows.
pub fn fs_symlink(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 3) {
        return;
    }
    two_path_call(scope, &args, "symlink", libc::symlink);
}

/// Calls on a descriptor that return nothing: fsync, fdatasync.
fn fd_call(
    scope: &mut v8::PinScope,
    args: &Args,
    syscall: &str,
    call: unsafe extern "C" fn(libc::c_int) -> libc::c_int,
) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, args, 1) {
        return;
    }
    // SAFETY: a bad descriptor is EBADF.
    if let Err(e) = check(unsafe { call(fd) }) {
        throw_uv(scope, e, syscall, None, None);
    }
}

pub fn fs_fsync(scope: &mut v8::PinScope, args: Args, _: Ret) {
    fd_call(scope, &args, "fsync", libc::fsync);
}

pub fn fs_fdatasync(scope: &mut v8::PinScope, args: Args, _: Ret) {
    #[cfg(target_os = "macos")]
    fd_call(scope, &args, "fdatasync", libc::fsync);
    #[cfg(not(target_os = "macos"))]
    fd_call(scope, &args, "fdatasync", libc::fdatasync);
}

/// `ftruncate(fd, length)`.
pub fn fs_ftruncate(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 2) {
        return;
    }
    let length = args.get(1).integer_value(scope).unwrap_or(0);
    // SAFETY: a bad descriptor is EBADF.
    if let Err(e) = check(unsafe { libc::ftruncate(fd, length) }) {
        throw_uv(scope, e, "ftruncate", None, None);
    }
}

/// `chmod(path, mode)`.
pub fn fs_chmod(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    let path = path_arg(scope, args.get(0));
    let mode = uint(scope, args.get(1)) as libc::mode_t;
    // SAFETY: the path is a C string.
    if let Err(e) = check(unsafe { libc::chmod(c_path(&path).as_ptr(), mode) }) {
        throw_uv(scope, e, "chmod", Some(&path), None);
    }
}

/// `fchmod(fd, mode)`.
pub fn fs_fchmod(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 2) {
        return;
    }
    let mode = uint(scope, args.get(1)) as libc::mode_t;
    // SAFETY: a bad descriptor is EBADF.
    if let Err(e) = check(unsafe { libc::fchmod(fd, mode) }) {
        throw_uv(scope, e, "fchmod", None, None);
    }
}

fn chown_path(
    scope: &mut v8::PinScope,
    args: &Args,
    syscall: &str,
    call: unsafe extern "C" fn(*const libc::c_char, libc::uid_t, libc::gid_t) -> libc::c_int,
) {
    let path = path_arg(scope, args.get(0));
    let uid = uint(scope, args.get(1));
    let gid = uint(scope, args.get(2));
    // SAFETY: the path is a C string.
    if let Err(e) = check(unsafe { call(c_path(&path).as_ptr(), uid, gid) }) {
        throw_uv(scope, e, syscall, Some(&path), None);
    }
}

/// `chown(path, uid, gid)`.
pub fn fs_chown(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 3) {
        return;
    }
    chown_path(scope, &args, "chown", libc::chown);
}

/// `lchown(path, uid, gid)`.
pub fn fs_lchown(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 3) {
        return;
    }
    chown_path(scope, &args, "lchown", libc::lchown);
}

/// `fchown(fd, uid, gid)`.
pub fn fs_fchown(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 3) {
        return;
    }
    let uid = uint(scope, args.get(1));
    let gid = uint(scope, args.get(2));
    // SAFETY: a bad descriptor is EBADF.
    if let Err(e) = check(unsafe { libc::fchown(fd, uid, gid) }) {
        throw_uv(scope, e, "fchown", None, None);
    }
}

/// A time in seconds as a timespec, as libuv's uv__fs_to_timespec makes
/// one: infinity is "now", NaN is "leave as is".
fn timespec(time: f64) -> libc::timespec {
    if time.is_nan() {
        return libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        };
    }
    if time.is_infinite() {
        return libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_NOW,
        };
    }
    let mut sec = time as i64;
    let mut nsec = ((time - sec as f64) * 1e9) as i64;
    if nsec < 0 {
        sec -= 1;
        nsec += 1_000_000_000;
    }
    libc::timespec {
        tv_sec: sec as libc::time_t,
        tv_nsec: nsec as _,
    }
}

fn times(scope: &mut v8::PinScope, args: &Args) -> [libc::timespec; 2] {
    let atime = args.get(1).number_value(scope).unwrap_or(f64::NAN);
    let mtime = args.get(2).number_value(scope).unwrap_or(f64::NAN);
    [timespec(atime), timespec(mtime)]
}

fn utimes_path(scope: &mut v8::PinScope, args: &Args, syscall: &str, flags: i32) {
    let path = path_arg(scope, args.get(0));
    let times = times(scope, args);
    // SAFETY: the path is a C string and times two timespecs.
    let result = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            c_path(&path).as_ptr(),
            times.as_ptr(),
            flags,
        )
    };
    if let Err(e) = check(result) {
        throw_uv(scope, e, syscall, Some(&path), None);
    }
}

/// `utimes(path, atime, mtime)`, in seconds.
pub fn fs_utimes(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 3) {
        return;
    }
    utimes_path(scope, &args, "utime", 0);
}

/// `lutimes(path, atime, mtime)`.
pub fn fs_lutimes(scope: &mut v8::PinScope, args: Args, _: Ret) {
    if asynchronous(scope, &args, 3) {
        return;
    }
    utimes_path(scope, &args, "lutime", libc::AT_SYMLINK_NOFOLLOW);
}

/// `futimes(fd, atime, mtime)`.
pub fn fs_futimes(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    if asynchronous(scope, &args, 3) {
        return;
    }
    let times = times(scope, &args);
    // SAFETY: times is two timespecs.
    if let Err(e) = check(unsafe { libc::futimens(fd, times.as_ptr()) }) {
        throw_uv(scope, e, "futime", None, None);
    }
}

/// `mkdir(path, mode, recursive)`: with `recursive`, the first directory
/// made (undefined when there was none to make), as Node's MKDirpSync
/// reports it.
pub fn fs_mkdir(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if asynchronous(scope, &args, 3) {
        return;
    }
    let path = path_arg(scope, args.get(0));
    let mode = uint(scope, args.get(1)) as libc::mode_t;
    if !args.get(2).is_true() {
        // SAFETY: the path is a C string.
        if let Err(e) = check(unsafe { libc::mkdir(c_path(&path).as_ptr(), mode) }) {
            throw_uv(scope, e, "mkdir", Some(&path), None);
        }
        return;
    }
    match mkdirp(&path, mode) {
        Ok(Some(first)) => {
            let first = String::from_utf8_lossy(&first);
            rv.set(string(scope, &first).into());
        }
        Ok(None) => {}
        Err(e) => throw_uv(scope, e, "mkdir", Some(&path), None),
    }
}

/// Makes `path` and any missing parents; the first one made, if any. Each
/// directory is made with `mode`, as MKDirpSync makes them.
fn mkdirp(path: &[u8], mode: libc::mode_t) -> Result<Option<Vec<u8>>, i32> {
    // SAFETY: the path is a C string.
    match check(unsafe { libc::mkdir(c_path(path).as_ptr(), mode) }) {
        Ok(_) => Ok(Some(path.to_vec())),
        Err(e) if e == -libc::EEXIST || e == -libc::EISDIR => {
            match stat(Target::Path(&c_path(path), true)) {
                Ok(fields) if is_dir(fields[1]) => Ok(None),
                Ok(_) => Err(-libc::EEXIST),
                Err(e) => Err(e),
            }
        }
        Err(e) if e == -libc::ENOENT => {
            let trimmed = trim_trailing_slashes(path);
            let Some(slash) = trimmed.iter().rposition(|&b| b == b'/') else {
                return Err(e);
            };
            let parent = if slash == 0 {
                &trimmed[..1]
            } else {
                &trimmed[..slash]
            };
            if parent == trimmed {
                return Err(e);
            }
            let first = match mkdirp(parent, mode) {
                Ok(first) => first,
                // A file where a parent directory should be.
                Err(e) if e == -libc::EEXIST => return Err(-libc::ENOTDIR),
                Err(e) => return Err(e),
            };
            // SAFETY: the path is a C string.
            match check(unsafe { libc::mkdir(c_path(path).as_ptr(), mode) }) {
                Ok(_) => Ok(first.or_else(|| Some(path.to_vec()))),
                Err(e) if e == -libc::EEXIST => Ok(first),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

fn trim_trailing_slashes(path: &[u8]) -> &[u8] {
    let mut end = path.len();
    while end > 1 && path[end - 1] == b'/' {
        end -= 1;
    }
    &path[..end]
}

/// libuv's dirent types (UV_DIRENT_*), from a file type.
fn dirent_type(kind: std::fs::FileType) -> i32 {
    use std::os::unix::fs::FileTypeExt;
    if kind.is_file() {
        1
    } else if kind.is_dir() {
        2
    } else if kind.is_symlink() {
        3
    } else if kind.is_fifo() {
        4
    } else if kind.is_socket() {
        5
    } else if kind.is_char_device() {
        6
    } else if kind.is_block_device() {
        7
    } else {
        0
    }
}

/// `readdir(path, encoding, withFileTypes)`: the names, sorted bytewise as
/// libuv's scandir sorts them, and with withFileTypes `[names, types]`.
pub fn fs_readdir(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if asynchronous(scope, &args, 3) {
        return;
    }
    let path = path_arg(scope, args.get(0));
    let with_types = args.get(2).is_true();
    let fail = |scope: &mut v8::PinScope, error: std::io::Error| {
        let e = -error.raw_os_error().unwrap_or(libc::EIO);
        throw_uv(scope, e, "scandir", Some(&path), None);
    };
    let entries = match std::fs::read_dir(std::ffi::OsStr::from_bytes(&path)) {
        Ok(entries) => entries,
        Err(error) => return fail(scope, error),
    };
    let mut found = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => {
                let kind = entry.file_type().map_or(0, dirent_type);
                found.push((entry.file_name().as_bytes().to_vec(), kind));
            }
            Err(error) => return fail(scope, error),
        }
    }
    found.sort();
    let encoding = args.get(1);
    let mut names = Vec::with_capacity(found.len());
    for (name, _) in &found {
        let Some(name) = encoded(scope, name, encoding) else {
            return;
        };
        names.push(name);
    }
    let names = v8::Array::new_with_elements(scope, &names);
    if !with_types {
        return rv.set(names.into());
    }
    let types: Vec<_> = found
        .iter()
        .map(|&(_, kind)| v8::Integer::new(scope, kind).into())
        .collect();
    let types = v8::Array::new_with_elements(scope, &types);
    rv.set(v8::Array::new_with_elements(scope, &[names.into(), types.into()]).into());
}

/// `readlink(path, encoding)`.
pub fn fs_readlink(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    let path = path_arg(scope, args.get(0));
    match std::fs::read_link(std::ffi::OsStr::from_bytes(&path)) {
        Ok(target) => {
            if let Some(value) = encoded(scope, target.as_os_str().as_bytes(), args.get(1)) {
                rv.set(value);
            }
        }
        Err(error) => {
            let e = -error.raw_os_error().unwrap_or(libc::EIO);
            throw_uv(scope, e, "readlink", Some(&path), None);
        }
    }
}

/// `realpath(path, encoding)`: realpath(3), as uv_fs_realpath calls it.
pub fn fs_realpath(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    let path = path_arg(scope, args.get(0));
    // SAFETY: the path is a C string; realpath mallocs the result.
    let resolved = unsafe { libc::realpath(c_path(&path).as_ptr(), std::ptr::null_mut()) };
    if resolved.is_null() {
        return throw_uv(scope, last_error(), "realpath", Some(&path), None);
    }
    // SAFETY: realpath returned a malloced C string, freed here once copied.
    let real = unsafe { CStr::from_ptr(resolved) }.to_bytes().to_vec();
    unsafe { libc::free(resolved.cast()) };
    if let Some(value) = encoded(scope, &real, args.get(1)) {
        rv.set(value);
    }
}

/// `mkdtemp(prefix, encoding)`: the directory made.
pub fn fs_mkdtemp(scope: &mut v8::PinScope, args: Args, mut rv: Ret) {
    if asynchronous(scope, &args, 2) {
        return;
    }
    let mut template = path_arg(scope, args.get(0));
    template.extend_from_slice(b"XXXXXX");
    let mut buf = template.clone();
    buf.push(0);
    // SAFETY: buf is a NUL-terminated template mkdtemp rewrites in place.
    let made = unsafe { libc::mkdtemp(buf.as_mut_ptr().cast()) };
    if made.is_null() {
        return throw_uv(scope, last_error(), "mkdtemp", Some(&template), None);
    }
    buf.pop();
    if let Some(value) = encoded(scope, &buf, args.get(1)) {
        rv.set(value);
    }
}

const COPYFILE_EXCL: i32 = 1;
const COPYFILE_FICLONE_FORCE: i32 = 4;

/// `copyFile(src, dest, mode)`: libuv's uv_fs_copyfile.
pub fn fs_copy_file(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let src = path_arg(scope, args.get(0));
    let dest = path_arg(scope, args.get(1));
    let Some(mode) = mode_arg(scope, args.get(2), 0) else {
        return;
    };
    if asynchronous(scope, &args, 3) {
        return;
    }
    if let Err(e) = copy_file(&src, &dest, mode) {
        throw_uv(scope, e, "copyfile", Some(&src), Some(&dest));
    }
}

fn copy_file(src: &[u8], dest: &[u8], mode: i32) -> Result<(), i32> {
    use std::os::fd::FromRawFd;
    let from = open(src, libc::O_RDONLY, 0)?;
    // SAFETY: from is a descriptor this function owns.
    let mut from = unsafe { std::fs::File::from_raw_fd(from) };
    let fields = stat(Target::Fd(std::os::fd::AsRawFd::as_raw_fd(&from)))?;
    let permissions = fields[1] as u32 & 0o7777;
    let mut flags = libc::O_WRONLY | libc::O_CREAT;
    if mode & COPYFILE_EXCL != 0 {
        flags |= libc::O_EXCL;
    }
    let to = open(dest, flags, permissions)?;
    // SAFETY: to is a descriptor this function owns.
    let mut to = unsafe { std::fs::File::from_raw_fd(to) };
    let to_fields = stat(Target::Fd(std::os::fd::AsRawFd::as_raw_fd(&to)))?;
    // Copying a file onto itself leaves it as it is.
    if fields[0] == to_fields[0] && fields[7] == to_fields[7] {
        return Ok(());
    }
    let os = |error: std::io::Error| -error.raw_os_error().unwrap_or(libc::EIO);
    to.set_len(0).map_err(os)?;
    to.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(permissions))
        .map_err(os)?;
    if mode & COPYFILE_FICLONE_FORCE != 0 {
        #[cfg(target_os = "linux")]
        {
            const FICLONE: libc::c_ulong = 0x4004_9409;
            let (to_fd, from_fd) = (
                std::os::fd::AsRawFd::as_raw_fd(&to),
                std::os::fd::AsRawFd::as_raw_fd(&from),
            );
            // SAFETY: both are open descriptors.
            return check(unsafe { libc::ioctl(to_fd, FICLONE as _, from_fd) }).map(|_| ());
        }
        #[cfg(not(target_os = "linux"))]
        return Err(-libc::ENOTSUP);
    }
    std::io::copy(&mut from, &mut to).map_err(os)?;
    Ok(())
}

/// `rmSync(path, maxRetries, recursive, retryDelay)`: Node's C++ rm, which
/// lib/fs.js calls once it has checked the options and that the path is
/// there. Busy and similar errors are retried, waiting a little longer each
/// time; what is left is thrown as Node's ErrnoException.
pub fn fs_rm_sync(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let path = path_arg(scope, args.get(0));
    let max_retries = int(scope, args.get(1)).max(0);
    let recursive = args.get(2).is_true();
    let delay = int(scope, args.get(3)).max(0) as u64;
    let os_path = std::ffi::OsStr::from_bytes(&path);
    let remove = || -> std::io::Result<()> {
        match std::fs::symlink_metadata(os_path) {
            Ok(meta) if meta.is_dir() && recursive => std::fs::remove_dir_all(os_path),
            Ok(meta) if meta.is_dir() => std::fs::remove_dir(os_path),
            Ok(_) => std::fs::remove_file(os_path),
            Err(error) => Err(error),
        }
    };
    let mut attempt = 0;
    let error = loop {
        let errno = match remove() {
            Ok(()) => return,
            Err(error) => error.raw_os_error().unwrap_or(libc::EIO),
        };
        if errno == libc::ENOENT {
            return;
        }
        let retryable = [
            libc::EBUSY,
            libc::EMFILE,
            libc::ENFILE,
            libc::ENOTEMPTY,
            libc::EPERM,
        ]
        .contains(&errno);
        if !retryable || attempt >= max_retries {
            break errno;
        }
        attempt += 1;
        std::thread::sleep(std::time::Duration::from_millis(attempt as u64 * delay));
    };
    let path_text = String::from_utf8_lossy(&path).into_owned();
    let (errno, message) = match error {
        libc::EACCES => (libc::EACCES, format!("Permission denied: {path_text}")),
        libc::ENOTEMPTY => (libc::ENOTEMPTY, format!("Directory not empty: {path_text}")),
        libc::ENOTDIR => (libc::ENOTDIR, format!("Not a directory: {path_text}")),
        libc::EPERM => (libc::EPERM, format!("Operation not permitted: {path_text}")),
        other => {
            let message = std::io::Error::from_raw_os_error(other).to_string();
            (other, format!("Unknown error: {message}"))
        }
    };
    throw_errno(scope, errno, "rm", &message, &path);
}

/// Sets `name: value` for each listed libc constant on an object.
macro_rules! libc_constants {
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

fn fixed<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    names: &[(&str, i32)],
) {
    for &(name, value) in names {
        let value = v8::Integer::new(scope, value).into();
        set(scope, object, name, value);
    }
}

/// `fsConstants()`: internalBinding('constants').fs, in Node's order
/// (node_constants.cc's DefineSystemConstants).
pub fn fs_constants<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'s, v8::Value>,
) {
    let object = v8::Object::new(scope);
    fixed(
        scope,
        object,
        &[("UV_FS_SYMLINK_DIR", 1), ("UV_FS_SYMLINK_JUNCTION", 2)],
    );
    libc_constants!(scope, object, O_RDONLY, O_WRONLY, O_RDWR);
    fixed(
        scope,
        object,
        &[
            ("UV_DIRENT_UNKNOWN", 0),
            ("UV_DIRENT_FILE", 1),
            ("UV_DIRENT_DIR", 2),
            ("UV_DIRENT_LINK", 3),
            ("UV_DIRENT_FIFO", 4),
            ("UV_DIRENT_SOCKET", 5),
            ("UV_DIRENT_CHAR", 6),
            ("UV_DIRENT_BLOCK", 7),
        ],
    );
    libc_constants!(
        scope, object, S_IFMT, S_IFREG, S_IFDIR, S_IFCHR, S_IFBLK, S_IFIFO, S_IFLNK, S_IFSOCK,
        O_CREAT, O_EXCL,
    );
    fixed(scope, object, &[("UV_FS_O_FILEMAP", 0)]);
    libc_constants!(
        scope,
        object,
        O_NOCTTY,
        O_TRUNC,
        O_APPEND,
        O_DIRECTORY,
        #[cfg(target_os = "linux")]
        O_NOATIME,
        O_NOFOLLOW,
        O_SYNC,
        O_DSYNC,
        #[cfg(target_os = "macos")]
        O_SYMLINK,
        #[cfg(target_os = "linux")]
        O_DIRECT,
        O_NONBLOCK,
        S_IRWXU,
        S_IRUSR,
        S_IWUSR,
        S_IXUSR,
        S_IRWXG,
        S_IRGRP,
        S_IWGRP,
        S_IXGRP,
        S_IRWXO,
        S_IROTH,
        S_IWOTH,
        S_IXOTH,
        F_OK,
        R_OK,
        W_OK,
        X_OK,
    );
    fixed(
        scope,
        object,
        &[
            ("UV_FS_COPYFILE_EXCL", 1),
            ("COPYFILE_EXCL", 1),
            ("UV_FS_COPYFILE_FICLONE", 2),
            ("COPYFILE_FICLONE", 2),
            ("UV_FS_COPYFILE_FICLONE_FORCE", 4),
            ("COPYFILE_FICLONE_FORCE", 4),
        ],
    );
    rv.set(object.into());
}
