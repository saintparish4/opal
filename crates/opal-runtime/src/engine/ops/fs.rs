//! internalBinding('fs'): the synchronous calls lib/fs.js (vendored
//! unmodified in js/node/) makes for its *Sync functions, with the semantics
//! of Node's src/node_file.cc. Each makes the system call libuv's uv_fs_*
//! makes for it on unix, and a failure throws what Node's UVException
//! throws: `ENOENT: no such file or directory, open 'x'`, with `errno`,
//! `code`, `syscall`, and `path` and `dest` when the call has them, in that
//! order. The callback and promise halves of fs are not here.

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;

use super::buffer::{Encoding, bytes, encode};
use super::requests::{self as io, Blocking, Dest, Done, Failure, Outcome};
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
    syscall: &'static str,
    path: Option<&[u8]>,
    dest: Option<&[u8]>,
) {
    let failure = Failure {
        errno: error,
        syscall,
        path: path.map(<[u8]>::to_vec),
        dest: dest.map(<[u8]>::to_vec),
    };
    let exception = io::exception(scope, &failure);
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

/// What the stat and statfs calls fill: set up once, when
/// internalBinding('fs') is first made.
pub struct FsState {
    stats: v8::Global<v8::Float64Array>,
    bigint_stats: v8::Global<v8::BigInt64Array>,
    statfs: v8::Global<v8::Float64Array>,
    bigint_statfs: v8::Global<v8::BigInt64Array>,
}

/// `fsSetup(statValues, bigintStatValues, statFsValues, bigintStatFsValues)`.
pub fn fs_setup(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let (Ok(stats), Ok(bigint_stats), Ok(statfs), Ok(bigint_statfs)) = (
        v8::Local::<v8::Float64Array>::try_from(args.get(0)),
        v8::Local::<v8::BigInt64Array>::try_from(args.get(1)),
        v8::Local::<v8::Float64Array>::try_from(args.get(2)),
        v8::Local::<v8::BigInt64Array>::try_from(args.get(3)),
    ) else {
        return type_error(scope, "fsSetup takes the stat arrays");
    };
    let state = FsState {
        stats: v8::Global::new(scope, stats),
        bigint_stats: v8::Global::new(scope, bigint_stats),
        statfs: v8::Global::new(scope, statfs),
        bigint_statfs: v8::Global::new(scope, bigint_statfs),
    };
    scope.set_slot(state);
}

/// Field indices of Node's stat arrays (node_file.h's FsStatsOffset).
const STAT_FIELDS: usize = 18;
pub type StatFields = [i64; STAT_FIELDS];

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
enum StatOf<'a> {
    Path(&'a CStr, bool),
    Fd(i32),
}

fn stat(target: StatOf) -> Result<StatFields, i32> {
    #[cfg(target_os = "linux")]
    {
        let result = match target {
            StatOf::Path(path, follow) => statx(
                libc::AT_FDCWD,
                path,
                if follow { 0 } else { libc::AT_SYMLINK_NOFOLLOW },
            ),
            StatOf::Fd(fd) => statx(fd, c"", libc::AT_EMPTY_PATH),
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
            StatOf::Path(path, true) => libc::stat(path.as_ptr(), &mut buf),
            StatOf::Path(path, false) => libc::lstat(path.as_ptr(), &mut buf),
            StatOf::Fd(fd) => libc::fstat(fd, &mut buf),
        }
    };
    check(result)?;
    Ok(stat_fields(&buf))
}

/// Stat fields as Node's binding hands them over: in its shared arrays
/// (FillGlobalStatsArray), or, for a promise, in arrays of their own, as
/// FSReqPromise keeps.
pub(super) fn stats_array<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    fields: &[i64],
    bigint: bool,
    statfs: bool,
    shared: bool,
) -> v8::Local<'s, v8::Value> {
    let arrays = scope.get_slot::<FsState>().map(|state| {
        if statfs {
            (state.statfs.clone(), state.bigint_statfs.clone())
        } else {
            (state.stats.clone(), state.bigint_stats.clone())
        }
    });
    if bigint {
        let array = match (&arrays, shared) {
            (Some((_, big)), true) => v8::Local::new(scope, big),
            _ => {
                let buffer = v8::ArrayBuffer::new(scope, fields.len() * 8);
                v8::BigInt64Array::new(scope, buffer, 0, fields.len()).unwrap()
            }
        };
        for (i, &value) in fields.iter().enumerate() {
            let value = v8::BigInt::new_from_i64(scope, value).into();
            array.set_index(scope, i as u32, value);
        }
        array.into()
    } else {
        let array = match (&arrays, shared) {
            (Some((float, _)), true) => v8::Local::new(scope, float),
            _ => {
                let buffer = v8::ArrayBuffer::new(scope, fields.len() * 8);
                v8::Float64Array::new(scope, buffer, 0, fields.len()).unwrap()
            }
        };
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

/// Makes `path` and any missing parents; the first one made, if any. Each
/// directory is made with `mode`, as MKDirpSync makes them.
fn mkdirp(path: &[u8], mode: libc::mode_t) -> Result<Option<Vec<u8>>, i32> {
    // SAFETY: the path is a C string.
    match check(unsafe { libc::mkdir(c_path(path).as_ptr(), mode) }) {
        Ok(_) => Ok(Some(path.to_vec())),
        Err(e) if e == -libc::EEXIST || e == -libc::EISDIR => {
            match stat(StatOf::Path(&c_path(path), true)) {
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

const COPYFILE_EXCL: i32 = 1;
const COPYFILE_FICLONE_FORCE: i32 = 4;

/// Returns or throws a sync call's outcome, as the binding does when no
/// request is passed: the error then has the caller's stack.
fn sync<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    rv: &mut Ret<'s>,
    outcome: Outcome,
    dest: Option<&Dest>,
) {
    match outcome {
        Ok(done) => {
            let value = io::value(scope, done, dest, true);
            if !value.is_undefined() {
                rv.set(value);
            }
        }
        Err(failure) => {
            let error = io::exception(scope, &failure);
            scope.throw_exception(error);
        }
    }
}

/// Runs a call's job, sync or as the request at `index` asks, and returns
/// or throws a sync call's outcome.
fn call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &Args<'s>,
    rv: &mut Ret<'s>,
    index: i32,
    blocking: impl FnOnce() -> Blocking,
    job: impl FnOnce() -> Outcome + Send + 'static,
) {
    let request = args.get(index);
    let blocking = if request.is_undefined() {
        Blocking::No
    } else {
        blocking()
    };
    if let Some(outcome) = io::dispatch(scope, request, rv, blocking, None, job) {
        sync(scope, rv, outcome, None);
    }
}

/// Above this, a read or write runs on the pool even on a regular file, so
/// that one large transfer doesn't stall the loop.
const LARGE: usize = 1 << 20;

/// Whether calls on `fd` can block: anything but a regular file or a
/// directory (a pipe, a socket, a terminal) can.
#[allow(clippy::unnecessary_cast)] // mode_t's width differs by platform
fn fd_blocks(fd: i32) -> Blocking {
    match stat(StatOf::Fd(fd)) {
        Ok(fields) => {
            let kind = fields[1] as u32 & libc::S_IFMT as u32;
            if kind == libc::S_IFREG as u32 || kind == libc::S_IFDIR as u32 {
                Blocking::No
            } else {
                Blocking::Yes
            }
        }
        Err(_) => Blocking::No,
    }
}

fn transfer_blocks(fd: i32, length: usize) -> Blocking {
    if length > LARGE {
        Blocking::Yes
    } else {
        fd_blocks(fd)
    }
}

/// Opening a FIFO waits for the other end, so it runs on the pool.
#[allow(clippy::unnecessary_cast)]
fn open_blocks(path: &[u8]) -> Blocking {
    match stat(StatOf::Path(&c_path(path), true)) {
        Ok(fields) if fields[1] as u32 & libc::S_IFMT as u32 == libc::S_IFIFO as u32 => {
            Blocking::Yes
        }
        _ => Blocking::No,
    }
}

fn unit(result: Result<impl Sized, i32>, failure: impl FnOnce(i32) -> Failure) -> Outcome {
    result.map(|_| Done::Undefined).map_err(failure)
}

/// `stat(path, useBigint, req | undefined, throwIfNoEntry)` and `lstat`:
/// the filled stat array, or undefined for a path that doesn't exist when
/// throwIfNoEntry is false, in the callback and promise forms too.
fn stat_path<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>, follow: bool) {
    let path = path_arg(scope, args.get(0));
    let bigint = args.get(1).is_true();
    let throw_if_no_entry = !args.get(3).is_false();
    let syscall = if follow { "stat" } else { "lstat" };
    let job = move || match stat(StatOf::Path(&c_path(&path), follow)) {
        Ok(fields) => Ok(Done::Stats(fields, bigint)),
        Err(e) if !throw_if_no_entry && (e == -libc::ENOENT || e == -libc::ENOTDIR) => {
            Ok(Done::Undefined)
        }
        Err(e) => Err(Failure::path(e, syscall, &path)),
    };
    let request = args.get(2);
    if let Some(outcome) = io::dispatch(scope, request, &mut rv, Blocking::No, None, job) {
        sync(scope, &mut rv, outcome, None);
    }
}

pub fn fs_stat<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    stat_path(scope, args, rv, true);
}

pub fn fs_lstat<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    stat_path(scope, args, rv, false);
}

/// `fstat(fd, useBigint, req | undefined, shouldNotThrow)`.
pub fn fs_fstat<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let bigint = args.get(1).is_true();
    let job = move || {
        stat(StatOf::Fd(fd))
            .map(|fields| Done::Stats(fields, bigint))
            .map_err(|e| Failure::new(e, "fstat"))
    };
    let should_not_throw = args.get(3).is_true();
    if let Some(outcome) = io::dispatch(scope, args.get(2), &mut rv, Blocking::No, None, job) {
        match outcome {
            Err(_) if should_not_throw => {}
            outcome => sync(scope, &mut rv, outcome, None),
        }
    }
}

/// `statfs(path, useBigint, req)`: type, bsize, frsize, blocks, bfree,
/// bavail, files, ffree.
#[allow(clippy::unnecessary_cast)] // as in stat_fields
pub fn fs_statfs<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let bigint = args.get(1).is_true();
    call(
        scope,
        &args,
        &mut rv,
        2,
        || Blocking::No,
        move || {
            // SAFETY: statfs writes a statfs struct, which may be zeroed.
            let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
            // SAFETY: the path is a C string and buf a statfs struct.
            let result = unsafe { libc::statfs(c_path(&path).as_ptr(), &mut buf) };
            check(result).map_err(|e| Failure::path(e, "statfs", &path))?;
            // libuv's uv_statfs_t. macOS's statfs has no fragment size: its
            // blocks are counted in f_bsize.
            #[cfg(target_os = "macos")]
            let frsize = buf.f_bsize as i64;
            #[cfg(not(target_os = "macos"))]
            let frsize = buf.f_frsize as i64;
            let fields = [
                buf.f_type as i64,
                buf.f_bsize as i64,
                frsize,
                buf.f_blocks as i64,
                buf.f_bfree as i64,
                buf.f_bavail as i64,
                buf.f_files as i64,
                buf.f_ffree as i64,
            ];
            Ok(Done::StatFs(fields, bigint))
        },
    );
}

/// `internalModuleStat(path)`: 0 for a file, 1 for a directory, or the
/// negative error number.
pub fn fs_internal_module_stat<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: Args<'s>,
    mut rv: Ret<'s>,
) {
    let path = path_arg(scope, args.get(args.length() - 1));
    let result = match stat(StatOf::Path(&c_path(&path), true)) {
        Ok(fields) if is_dir(fields[1]) => 1,
        Ok(_) => 0,
        Err(e) => e,
    };
    rv.set_int32(result);
}

/// `access(path, mode, req)`.
pub fn fs_access<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let Some(mode) = mode_arg(scope, args.get(1), libc::F_OK) else {
        return;
    };
    call(
        scope,
        &args,
        &mut rv,
        2,
        || Blocking::No,
        move || {
            // SAFETY: the path is a C string.
            let result = check(unsafe { libc::access(c_path(&path).as_ptr(), mode) });
            unit(result, |e| Failure::path(e, "access", &path))
        },
    );
}

/// `existsSync(path)`.
pub fn fs_exists_sync<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
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

/// `open(path, flags, mode, req)`: the new file descriptor.
pub fn fs_open<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let flags = int(scope, args.get(1));
    let mode = uint(scope, args.get(2));
    let probe = path.clone();
    call(
        scope,
        &args,
        &mut rv,
        3,
        move || open_blocks(&probe),
        move || {
            open(&path, flags, mode)
                .map(|fd| Done::Number(fd as f64))
                .map_err(|e| Failure::path(e, "open", &path))
        },
    );
}

/// `close(fd, req)`.
pub fn fs_close<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    call(
        scope,
        &args,
        &mut rv,
        1,
        || Blocking::No,
        move || {
            // SAFETY: as in close().
            unit(check(unsafe { libc::close(fd) }), |e| {
                Failure::new(e, "close")
            })
        },
    );
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

/// The window `[offset, offset + length)` of a buffer argument at `index`,
/// which lib/fs.js has already checked lies within it, as a view of its
/// own.
fn window<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &Args<'s>,
    index: i32,
) -> Option<v8::Local<'s, v8::ArrayBufferView>> {
    let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(args.get(index)) else {
        type_error(scope, "argument must be a buffer");
        return None;
    };
    let length = view.byte_length();
    let offset = args.get(index + 1).integer_value(scope).unwrap_or(0).max(0) as usize;
    let count = args.get(index + 2).integer_value(scope).unwrap_or(0).max(0) as usize;
    let start = offset.min(length);
    let end = start.saturating_add(count).min(length);
    let buffer = view.buffer(scope)?;
    v8::Uint8Array::new(scope, buffer, view.byte_offset() + start, end - start).map(Into::into)
}

/// `read(fd, buffer, offset, length, position, req)`: bytes read.
pub fn fs_read<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let Some(view) = window(scope, &args, 1) else {
        return;
    };
    let position = position(scope, args.get(4));
    let length = view.byte_length();
    let request = args.get(5);
    let blocking = if request.is_undefined() {
        Blocking::No
    } else {
        transfer_blocks(fd, length)
    };
    let dest = Dest(vec![v8::Global::new(scope, view)]);
    let job = move || {
        let mut bytes = vec![0; length];
        let n = read(fd, &mut bytes, position).map_err(|e| Failure::new(e, "read"))?;
        bytes.truncate(n);
        Ok(Done::Read(bytes))
    };
    let sync_dest = Dest(dest.0.clone());
    if let Some(outcome) = io::dispatch(scope, request, &mut rv, blocking, Some(dest), job) {
        sync(scope, &mut rv, outcome, Some(&sync_dest));
    }
}

/// The ArrayBufferViews of an array, in order.
fn views<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<Vec<v8::Local<'s, v8::ArrayBufferView>>> {
    let array = v8::Local::<v8::Array>::try_from(value).ok()?;
    let mut out = Vec::with_capacity(array.length() as usize);
    for i in 0..array.length() {
        let item = array.get_index(scope, i)?;
        out.push(v8::Local::<v8::ArrayBufferView>::try_from(item).ok()?);
    }
    Some(out)
}

/// `readBuffers(fd, buffers, position, req)`: readv/preadv, bytes read.
pub fn fs_read_buffers<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let Some(views) = views(scope, args.get(1)) else {
        return type_error(scope, "buffers must be an array of ArrayBufferViews");
    };
    let position = position(scope, args.get(2));
    let lengths: Vec<usize> = views.iter().map(|view| view.byte_length()).collect();
    let total: usize = lengths.iter().sum();
    let request = args.get(3);
    let blocking = if request.is_undefined() {
        Blocking::No
    } else {
        transfer_blocks(fd, total)
    };
    let dest = Dest(
        views
            .iter()
            .map(|view| v8::Global::new(scope, *view))
            .collect(),
    );
    let sync_dest = Dest(dest.0.clone());
    let job = move || {
        let mut chunks: Vec<Vec<u8>> = lengths.iter().map(|&n| vec![0; n]).collect();
        let vecs: Vec<libc::iovec> = chunks
            .iter_mut()
            .map(|chunk| libc::iovec {
                iov_base: chunk.as_mut_ptr().cast(),
                iov_len: chunk.len(),
            })
            .collect();
        let count = vecs.len() as i32;
        // SAFETY: each iovec describes one of `chunks`, alive until here.
        let n = retry(|| unsafe {
            if position < 0 {
                libc::readv(fd, vecs.as_ptr(), count) as i64
            } else {
                libc::preadv(fd, vecs.as_ptr(), count, position) as i64
            }
        })
        .map_err(|e| Failure::new(e, "read"))? as usize;
        let mut bytes: Vec<u8> = chunks.concat();
        bytes.truncate(n);
        Ok(Done::Read(bytes))
    };
    if let Some(outcome) = io::dispatch(scope, request, &mut rv, blocking, Some(dest), job) {
        sync(scope, &mut rv, outcome, Some(&sync_dest));
    }
}

/// `writeBuffers(fd, buffers, position, req)`: writev/pwritev, bytes
/// written.
pub fn fs_write_buffers<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let Some(views) = views(scope, args.get(1)) else {
        return type_error(scope, "buffers must be an array of ArrayBufferViews");
    };
    let position = position(scope, args.get(2));
    let chunks: Vec<Vec<u8>> = views
        .iter()
        .map(|view| {
            bytes(scope, (*view).into())
                .map(|b| b.to_vec())
                .unwrap_or_default()
        })
        .collect();
    let total: usize = chunks.iter().map(Vec::len).sum();
    call(
        scope,
        &args,
        &mut rv,
        3,
        || transfer_blocks(fd, total),
        move || {
            let vecs: Vec<libc::iovec> = chunks
                .iter()
                .map(|chunk| libc::iovec {
                    iov_base: chunk.as_ptr() as *mut libc::c_void,
                    iov_len: chunk.len(),
                })
                .collect();
            let count = vecs.len() as i32;
            // SAFETY: each iovec describes one of `chunks`, alive until here.
            let n = retry(|| unsafe {
                if position < 0 {
                    libc::writev(fd, vecs.as_ptr(), count) as i64
                } else {
                    libc::pwritev(fd, vecs.as_ptr(), count, position) as i64
                }
            })
            .map_err(|e| Failure::new(e, "write"))?;
            Ok(Done::Number(n as f64))
        },
    );
}

/// A write's sync form reports a failure the old way, which writeSync still
/// uses: by setting `errno` and `syscall` on the context object it passes,
/// for its handleErrorFromBinding to throw. Without one, it throws.
fn write_call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &Args<'s>,
    rv: &mut Ret<'s>,
    index: i32,
    fd: i32,
    data: Vec<u8>,
    position: i64,
) {
    let request = args.get(index);
    let blocking = if request.is_undefined() {
        Blocking::No
    } else {
        transfer_blocks(fd, data.len())
    };
    let job = move || {
        write(fd, &data, position)
            .map(|n| Done::Number(n as f64))
            .map_err(|e| Failure::new(e, "write"))
    };
    let Some(outcome) = io::dispatch(scope, request, rv, blocking, None, job) else {
        return;
    };
    match (
        outcome,
        v8::Local::<v8::Object>::try_from(args.get(index + 1)),
    ) {
        (Err(failure), Ok(ctx)) => {
            let value = v8::Integer::new(scope, failure.errno).into();
            set(scope, ctx, "errno", value);
            let value = string(scope, failure.syscall).into();
            set(scope, ctx, "syscall", value);
        }
        (outcome, _) => sync(scope, rv, outcome, None),
    }
}

/// `writeBuffer(fd, buffer, offset, length, position, req, ctx)`: bytes
/// written.
pub fn fs_write_buffer<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let Some(view) = window(scope, &args, 1) else {
        return;
    };
    let data = bytes(scope, view.into())
        .map(|b| b.to_vec())
        .unwrap_or_default();
    let position = position(scope, args.get(4));
    write_call(scope, &args, &mut rv, 5, fd, data, position);
}

/// `writeString(fd, string, position, encoding, req, ctx)`: bytes written.
pub fn fs_write_string<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let Some(text) = args.get(1).to_string(scope) else {
        return;
    };
    let position = position(scope, args.get(2));
    let encoding = Encoding::from_value(scope, args.get(3));
    let data = encode(scope, text, encoding);
    write_call(scope, &args, &mut rv, 4, fd, data, position);
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
            let error = io::exception(scope, &Failure::path(e, "open", &path));
            scope.throw_exception(error);
            None
        }
    }
}

/// `readFileUtf8(path | fd, flags)`: the whole file as a string. Sync only.
pub fn fs_read_file_utf8<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
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
        let error = io::exception(scope, &Failure::new(e, "read"));
        scope.throw_exception(error);
        return;
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

/// `writeFileUtf8(path | fd, data, flags, mode)`. Sync only.
pub fn fs_write_file_utf8<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, _: Ret<'s>) {
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
        let error = io::exception(scope, &Failure::new(e, "write"));
        scope.throw_exception(error);
    }
}

/// Calls taking one path: rmdir, unlink.
fn path_call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: Args<'s>,
    mut rv: Ret<'s>,
    syscall: &'static str,
    function: unsafe extern "C" fn(*const libc::c_char) -> libc::c_int,
) {
    let path = path_arg(scope, args.get(0));
    call(
        scope,
        &args,
        &mut rv,
        1,
        || Blocking::No,
        move || {
            // SAFETY: the path is a C string.
            let result = check(unsafe { function(c_path(&path).as_ptr()) });
            unit(result, |e| Failure::path(e, syscall, &path))
        },
    );
}

pub fn fs_rmdir<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    path_call(scope, args, rv, "rmdir", libc::rmdir);
}

pub fn fs_unlink<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    path_call(scope, args, rv, "unlink", libc::unlink);
}

/// Calls taking two paths, whose errors name both: rename, link, symlink.
fn two_path_call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: Args<'s>,
    mut rv: Ret<'s>,
    index: i32,
    syscall: &'static str,
    function: unsafe extern "C" fn(*const libc::c_char, *const libc::c_char) -> libc::c_int,
) {
    let from = path_arg(scope, args.get(0));
    let to = path_arg(scope, args.get(1));
    call(
        scope,
        &args,
        &mut rv,
        index,
        || Blocking::No,
        move || {
            // SAFETY: both paths are C strings.
            let result = check(unsafe { function(c_path(&from).as_ptr(), c_path(&to).as_ptr()) });
            unit(result, |e| Failure::paths(e, syscall, &from, &to))
        },
    );
}

/// `rename(oldPath, newPath, req)`.
pub fn fs_rename<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    two_path_call(scope, args, rv, 2, "rename", libc::rename);
}

/// `link(existingPath, newPath, req)`.
pub fn fs_link<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    two_path_call(scope, args, rv, 2, "link", libc::link);
}

/// `symlink(target, path, flags, req)`; the flags only matter on Windows.
pub fn fs_symlink<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    two_path_call(scope, args, rv, 3, "symlink", libc::symlink);
}

/// Calls on a descriptor that return nothing: fsync, fdatasync.
fn fd_call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: Args<'s>,
    mut rv: Ret<'s>,
    syscall: &'static str,
    function: unsafe extern "C" fn(libc::c_int) -> libc::c_int,
) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    call(
        scope,
        &args,
        &mut rv,
        1,
        || Blocking::No,
        move || {
            // SAFETY: a bad descriptor is EBADF.
            unit(check(unsafe { function(fd) }), |e| Failure::new(e, syscall))
        },
    );
}

pub fn fs_fsync<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    fd_call(scope, args, rv, "fsync", libc::fsync);
}

pub fn fs_fdatasync<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    #[cfg(target_os = "macos")]
    fd_call(scope, args, rv, "fdatasync", libc::fsync);
    #[cfg(not(target_os = "macos"))]
    fd_call(scope, args, rv, "fdatasync", libc::fdatasync);
}

/// `ftruncate(fd, length, req)`.
pub fn fs_ftruncate<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let length = args.get(1).integer_value(scope).unwrap_or(0);
    call(
        scope,
        &args,
        &mut rv,
        2,
        || Blocking::No,
        move || {
            // SAFETY: a bad descriptor is EBADF.
            unit(check(unsafe { libc::ftruncate(fd, length) }), |e| {
                Failure::new(e, "ftruncate")
            })
        },
    );
}

/// `chmod(path, mode, req)`.
pub fn fs_chmod<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let mode = uint(scope, args.get(1)) as libc::mode_t;
    call(
        scope,
        &args,
        &mut rv,
        2,
        || Blocking::No,
        move || {
            // SAFETY: the path is a C string.
            let result = check(unsafe { libc::chmod(c_path(&path).as_ptr(), mode) });
            unit(result, |e| Failure::path(e, "chmod", &path))
        },
    );
}

/// `fchmod(fd, mode, req)`.
pub fn fs_fchmod<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let mode = uint(scope, args.get(1)) as libc::mode_t;
    call(
        scope,
        &args,
        &mut rv,
        2,
        || Blocking::No,
        move || {
            // SAFETY: a bad descriptor is EBADF.
            unit(check(unsafe { libc::fchmod(fd, mode) }), |e| {
                Failure::new(e, "fchmod")
            })
        },
    );
}

fn chown_path<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: Args<'s>,
    mut rv: Ret<'s>,
    syscall: &'static str,
    function: unsafe extern "C" fn(*const libc::c_char, libc::uid_t, libc::gid_t) -> libc::c_int,
) {
    let path = path_arg(scope, args.get(0));
    let uid = uint(scope, args.get(1));
    let gid = uint(scope, args.get(2));
    call(
        scope,
        &args,
        &mut rv,
        3,
        || Blocking::No,
        move || {
            // SAFETY: the path is a C string.
            let result = check(unsafe { function(c_path(&path).as_ptr(), uid, gid) });
            unit(result, |e| Failure::path(e, syscall, &path))
        },
    );
}

/// `chown(path, uid, gid, req)`.
pub fn fs_chown<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    chown_path(scope, args, rv, "chown", libc::chown);
}

/// `lchown(path, uid, gid, req)`.
pub fn fs_lchown<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    chown_path(scope, args, rv, "lchown", libc::lchown);
}

/// `fchown(fd, uid, gid, req)`.
pub fn fs_fchown<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let uid = uint(scope, args.get(1));
    let gid = uint(scope, args.get(2));
    call(
        scope,
        &args,
        &mut rv,
        3,
        || Blocking::No,
        move || {
            // SAFETY: a bad descriptor is EBADF.
            unit(check(unsafe { libc::fchown(fd, uid, gid) }), |e| {
                Failure::new(e, "fchown")
            })
        },
    );
}

fn times(scope: &mut v8::PinScope, args: &Args) -> [libc::timespec; 2] {
    let atime = args.get(1).number_value(scope).unwrap_or(f64::NAN);
    let mtime = args.get(2).number_value(scope).unwrap_or(f64::NAN);
    [timespec(atime), timespec(mtime)]
}

fn utimes_path<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: Args<'s>,
    mut rv: Ret<'s>,
    syscall: &'static str,
    flags: i32,
) {
    let path = path_arg(scope, args.get(0));
    let times = times(scope, &args);
    call(
        scope,
        &args,
        &mut rv,
        3,
        || Blocking::No,
        move || {
            // SAFETY: the path is a C string and times two timespecs.
            let result = check(unsafe {
                libc::utimensat(
                    libc::AT_FDCWD,
                    c_path(&path).as_ptr(),
                    times.as_ptr(),
                    flags,
                )
            });
            unit(result, |e| Failure::path(e, syscall, &path))
        },
    );
}

/// `utimes(path, atime, mtime, req)`, in seconds.
pub fn fs_utimes<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    utimes_path(scope, args, rv, "utime", 0);
}

/// `lutimes(path, atime, mtime, req)`.
pub fn fs_lutimes<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, rv: Ret<'s>) {
    utimes_path(scope, args, rv, "lutime", libc::AT_SYMLINK_NOFOLLOW);
}

/// `futimes(fd, atime, mtime, req)`.
pub fn fs_futimes<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let Some(fd) = fd_arg(scope, args.get(0)) else {
        return;
    };
    let times = times(scope, &args);
    call(
        scope,
        &args,
        &mut rv,
        3,
        || Blocking::No,
        move || {
            // SAFETY: times is two timespecs.
            unit(check(unsafe { libc::futimens(fd, times.as_ptr()) }), |e| {
                Failure::new(e, "futime")
            })
        },
    );
}

/// `mkdir(path, mode, recursive, req)`: with `recursive`, the first
/// directory made (undefined when there was none to make), as Node's
/// MKDirpSync reports it.
pub fn fs_mkdir<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let mode = uint(scope, args.get(1)) as libc::mode_t;
    let recursive = args.get(2).is_true();
    call(
        scope,
        &args,
        &mut rv,
        3,
        || Blocking::No,
        move || {
            if !recursive {
                // SAFETY: the path is a C string.
                let result = check(unsafe { libc::mkdir(c_path(&path).as_ptr(), mode) });
                return unit(result, |e| Failure::path(e, "mkdir", &path));
            }
            match mkdirp(&path, mode) {
                Ok(Some(first)) => Ok(Done::Text(first)),
                Ok(None) => Ok(Done::Undefined),
                Err(e) => Err(Failure::path(e, "mkdir", &path)),
            }
        },
    );
}

/// `readdir(path, encoding, withFileTypes, req)`: the names, sorted
/// bytewise as libuv's scandir sorts them, and with withFileTypes `[names,
/// types]`.
pub fn fs_readdir<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let encoding = Encoding::from_value(scope, args.get(1));
    let with_types = args.get(2).is_true();
    call(
        scope,
        &args,
        &mut rv,
        3,
        || Blocking::No,
        move || {
            let fail = |error: std::io::Error| {
                Failure::path(-error.raw_os_error().unwrap_or(libc::EIO), "scandir", &path)
            };
            let entries = std::fs::read_dir(std::ffi::OsStr::from_bytes(&path)).map_err(fail)?;
            let mut found = Vec::new();
            for entry in entries {
                let entry = entry.map_err(fail)?;
                let kind = entry.file_type().map_or(0, dirent_type);
                found.push((entry.file_name().as_bytes().to_vec(), kind));
            }
            found.sort();
            let types = with_types.then(|| found.iter().map(|&(_, kind)| kind).collect());
            let names = found.into_iter().map(|(name, _)| name).collect();
            Ok(Done::Names(names, types, encoding))
        },
    );
}

/// `readlink(path, encoding, req)`.
pub fn fs_readlink<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let encoding = Encoding::from_value(scope, args.get(1));
    call(
        scope,
        &args,
        &mut rv,
        2,
        || Blocking::No,
        move || {
            std::fs::read_link(std::ffi::OsStr::from_bytes(&path))
                .map(|target| Done::Encoded(target.as_os_str().as_bytes().to_vec(), encoding))
                .map_err(|error| {
                    Failure::path(
                        -error.raw_os_error().unwrap_or(libc::EIO),
                        "readlink",
                        &path,
                    )
                })
        },
    );
}

/// `realpath(path, encoding, req)`: realpath(3), as uv_fs_realpath calls
/// it.
pub fn fs_realpath<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let encoding = Encoding::from_value(scope, args.get(1));
    call(
        scope,
        &args,
        &mut rv,
        2,
        || Blocking::No,
        move || {
            // SAFETY: the path is a C string; realpath mallocs the result.
            let resolved = unsafe { libc::realpath(c_path(&path).as_ptr(), std::ptr::null_mut()) };
            if resolved.is_null() {
                return Err(Failure::path(last_error(), "realpath", &path));
            }
            // SAFETY: realpath returned a malloced C string, freed here once
            // copied.
            let real = unsafe { CStr::from_ptr(resolved) }.to_bytes().to_vec();
            unsafe { libc::free(resolved.cast()) };
            Ok(Done::Encoded(real, encoding))
        },
    );
}

/// `mkdtemp(prefix, encoding, req)`: the directory made.
pub fn fs_mkdtemp<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let mut template = path_arg(scope, args.get(0));
    template.extend_from_slice(b"XXXXXX");
    let encoding = Encoding::from_value(scope, args.get(1));
    let original = template.clone();
    let job = move || {
        let mut buf = template.clone();
        buf.push(0);
        // SAFETY: buf is a NUL-terminated template mkdtemp rewrites in place.
        let made = unsafe { libc::mkdtemp(buf.as_mut_ptr().cast()) };
        buf.pop();
        if made.is_null() {
            // libuv reports the request's path, which mkdtemp has
            // rewritten, failing or not.
            return Err(Failure::path(last_error(), "mkdtemp", &buf));
        }
        Ok(Done::Encoded(buf, encoding))
    };
    if let Some(outcome) = io::dispatch(scope, args.get(2), &mut rv, Blocking::No, None, job) {
        // The sync form's error names the template as it was given.
        let outcome = outcome.map_err(|failure| Failure {
            path: Some(original),
            ..failure
        });
        sync(scope, &mut rv, outcome, None);
    }
}

/// The JavaScript classes of the handles fs returns, which ops/requests.rs
/// makes when an open delivers one.
pub struct Handles {
    file: v8::Global<v8::Function>,
    dir: v8::Global<v8::Function>,
}

pub enum Handle {
    File,
    Dir,
}

/// `fsHandles(FileHandle, DirHandle)`: the classes, from realm.js.
pub fn fs_handles(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let (Ok(file), Ok(dir)) = (
        v8::Local::<v8::Function>::try_from(args.get(0)),
        v8::Local::<v8::Function>::try_from(args.get(1)),
    ) else {
        return type_error(
            scope,
            "fsHandles takes the FileHandle and DirHandle classes",
        );
    };
    let handles = Handles {
        file: v8::Global::new(scope, file),
        dir: v8::Global::new(scope, dir),
    };
    scope.set_slot(handles);
}

/// A new FileHandle or DirHandle around `value` (a descriptor, a directory
/// id).
pub(super) fn new_handle<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    kind: Handle,
    value: v8::Local<'s, v8::Value>,
) -> v8::Local<'s, v8::Value> {
    let class = scope.get_slot::<Handles>().map(|handles| match kind {
        Handle::File => handles.file.clone(),
        Handle::Dir => handles.dir.clone(),
    });
    let Some(class) = class else {
        return v8::undefined(scope).into();
    };
    let class = v8::Local::new(scope, class);
    class
        .new_instance(scope, &[value])
        .map_or_else(|| v8::undefined(scope).into(), Into::into)
}

/// `openFileHandle(path, flags, mode, req)`: fs.promises' open, a
/// FileHandle for the new descriptor.
pub fn fs_open_file_handle<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let flags = int(scope, args.get(1));
    let mode = uint(scope, args.get(2));
    let probe = path.clone();
    call(
        scope,
        &args,
        &mut rv,
        3,
        move || open_blocks(&probe),
        move || {
            open(&path, flags, mode)
                .map(Done::FileHandle)
                .map_err(|e| Failure::path(e, "open", &path))
        },
    );
}

/// Directories open through fs.opendir, by id: std's ReadDir, which reads
/// them as libuv's uv_fs_readdir does (`.` and `..` left out, in the
/// directory's own order).
static DIRS: std::sync::Mutex<Option<(u64, std::collections::HashMap<u64, std::fs::ReadDir>)>> =
    std::sync::Mutex::new(None);

fn with_dirs<T>(
    f: impl FnOnce(&mut u64, &mut std::collections::HashMap<u64, std::fs::ReadDir>) -> T,
) -> T {
    let mut dirs = DIRS.lock().unwrap_or_else(|e| e.into_inner());
    let (next, map) = dirs.get_or_insert_with(|| (1, std::collections::HashMap::new()));
    f(next, map)
}

fn open_dir(path: &[u8]) -> Result<u64, i32> {
    let read = std::fs::read_dir(std::ffi::OsStr::from_bytes(path))
        .map_err(|error| -error.raw_os_error().unwrap_or(libc::EIO))?;
    Ok(with_dirs(|next, map| {
        let id = *next;
        *next += 1;
        map.insert(id, read);
        id
    }))
}

/// `opendir(path, encoding, req)`: internalBinding('fs_dir')'s, a
/// DirHandle.
pub fn fs_opendir<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    call(
        scope,
        &args,
        &mut rv,
        2,
        || Blocking::No,
        move || {
            open_dir(&path)
                .map(Done::DirHandle)
                .map_err(|e| Failure::path(e, "opendir", &path))
        },
    );
}

/// `opendirSync(path)`: as opendir, its error naming no path, as
/// node_dir.cc's OpenDirSync throws it.
pub fn fs_opendir_sync<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let path = path_arg(scope, args.get(0));
    let outcome = open_dir(&path)
        .map(Done::DirHandle)
        .map_err(|e| Failure::new(e, "opendir"));
    sync(scope, &mut rv, outcome, None);
}

/// `DirHandle.read(id, encoding, bufferSize, req)`: up to bufferSize
/// entries, or null at the end.
pub fn fs_dir_read<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let id = args.get(0).number_value(scope).unwrap_or(0.0) as u64;
    let encoding = Encoding::from_value(scope, args.get(1));
    let size = args.get(2).number_value(scope).unwrap_or(32.0).max(1.0) as usize;
    call(
        scope,
        &args,
        &mut rv,
        3,
        || Blocking::No,
        move || {
            with_dirs(|_, map| {
                let Some(read) = map.get_mut(&id) else {
                    return Err(Failure::new(-libc::EBADF, "readdir"));
                };
                let mut entries = Vec::new();
                for entry in read.by_ref() {
                    let entry = entry.map_err(|error| {
                        Failure::new(-error.raw_os_error().unwrap_or(libc::EIO), "readdir")
                    })?;
                    let kind = entry.file_type().map_or(0, dirent_type);
                    entries.push((entry.file_name().as_bytes().to_vec(), kind));
                    if entries.len() == size {
                        break;
                    }
                }
                if entries.is_empty() {
                    Ok(Done::Null)
                } else {
                    Ok(Done::Entries(entries, encoding))
                }
            })
        },
    );
}

/// `DirHandle.close(id, req)`.
pub fn fs_dir_close<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let id = args.get(0).number_value(scope).unwrap_or(0.0) as u64;
    call(
        scope,
        &args,
        &mut rv,
        1,
        || Blocking::No,
        move || match with_dirs(|_, map| map.remove(&id)) {
            Some(_) => Ok(Done::Undefined),
            None => Err(Failure::new(-libc::EBADF, "closedir")),
        },
    );
}

/// `copyFile(src, dest, mode, req)`: libuv's uv_fs_copyfile. A copy can be
/// any size, so an asynchronous one always runs on the pool.
pub fn fs_copy_file<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let src = path_arg(scope, args.get(0));
    let dest = path_arg(scope, args.get(1));
    let Some(mode) = mode_arg(scope, args.get(2), 0) else {
        return;
    };
    call(
        scope,
        &args,
        &mut rv,
        3,
        || Blocking::Yes,
        move || {
            unit(copy_file(&src, &dest, mode), |e| {
                Failure::paths(e, "copyfile", &src, &dest)
            })
        },
    );
}

fn copy_file(src: &[u8], dest: &[u8], mode: i32) -> Result<(), i32> {
    use std::os::fd::FromRawFd;
    let from = open(src, libc::O_RDONLY, 0)?;
    // SAFETY: from is a descriptor this function owns.
    let mut from = unsafe { std::fs::File::from_raw_fd(from) };
    let fields = stat(StatOf::Fd(std::os::fd::AsRawFd::as_raw_fd(&from)))?;
    let permissions = fields[1] as u32 & 0o7777;
    let mut flags = libc::O_WRONLY | libc::O_CREAT;
    if mode & COPYFILE_EXCL != 0 {
        flags |= libc::O_EXCL;
    }
    let to = open(dest, flags, permissions)?;
    // SAFETY: to is a descriptor this function owns.
    let mut to = unsafe { std::fs::File::from_raw_fd(to) };
    let to_fields = stat(StatOf::Fd(std::os::fd::AsRawFd::as_raw_fd(&to)))?;
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
