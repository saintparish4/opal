//! internalBinding('os') and the environment half of internalBinding
//! ('credentials'): Node's src/node_os.cc and src/node_credentials.cc over
//! what libuv does for them on Linux and macOS (deps/uv/src/unix), reading
//! the same files and making the same system calls, so the numbers match
//! Node's on one machine, quirks included.
//!
//! A function that can fail takes Node's `ctx` object last and, when it
//! fails, fills it in as CollectUVExceptionInfo does (errno, code, message,
//! syscall) and returns undefined, which lib/os.js (vendored unmodified)
//! turns into an ERR_SYSTEM_ERROR.

use std::ffi::CStr;

use super::buffer::Encoding;
use super::{Args, Ret, set, string};

/// CollectUVExceptionInfo: a failed call's errno, code, message and
/// syscall, set on `ctx`.
fn fail<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ctx: v8::Local<'s, v8::Value>,
    errno: i32,
    syscall: &str,
) {
    let Ok(ctx) = v8::Local::<v8::Object>::try_from(ctx) else {
        return;
    };
    if errno == 0 {
        return;
    }
    let (code, message) = super::fs::uv_error(errno);
    let value = v8::Integer::new(scope, errno).into();
    set(scope, ctx, "errno", value);
    let value = string(scope, code).into();
    set(scope, ctx, "code", value);
    let value = string(scope, message).into();
    set(scope, ctx, "message", value);
    let value = string(scope, syscall).into();
    set(scope, ctx, "syscall", value);
}

/// libuv's error number for the last system call's failure.
fn last_error() -> i32 {
    -std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

/// The last argument: the `ctx` a failure is reported on.
fn ctx<'s>(args: &Args<'s>) -> v8::Local<'s, v8::Value> {
    args.get((args.length() - 1).max(0))
}

/// String::NewFromUtf8, which replaces what isn't UTF-8.
fn utf8<'s>(scope: &mut v8::PinScope<'s, '_>, bytes: &[u8]) -> v8::Local<'s, v8::Value> {
    match v8::String::new_from_utf8(scope, bytes, v8::NewStringType::Normal) {
        Some(text) => text.into(),
        None => v8::undefined(scope).into(),
    }
}

/// OneByteString: each byte one Latin-1 character.
fn one_byte<'s>(scope: &mut v8::PinScope<'s, '_>, bytes: &[u8]) -> v8::Local<'s, v8::Value> {
    match v8::String::new_from_one_byte(scope, bytes, v8::NewStringType::Normal) {
        Some(text) => text.into(),
        None => v8::undefined(scope).into(),
    }
}

/// The bytes of a C string field, up to its NUL.
fn c_bytes(field: &[libc::c_char]) -> Vec<u8> {
    field
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect()
}

/// uv__slurp: what one read of up to `len - 1` bytes gets from a file.
#[cfg(target_os = "linux")]
fn slurp(path: &str, len: usize) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buffer = vec![0; len - 1];
    let n = loop {
        match file.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => break result.ok()?,
        }
    };
    buffer.truncate(n);
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// The numbers scanf's `%lf %lf ...` would read from the start of `text`.
#[cfg(target_os = "linux")]
fn leading_numbers<T: std::str::FromStr>(text: &str, count: usize) -> Option<Vec<T>> {
    let numbers: Vec<T> = text
        .split_ascii_whitespace()
        .take(count)
        .map_while(|word| word.parse().ok())
        .collect();
    (numbers.len() == count).then_some(numbers)
}

/// `getHostname(ctx)`: uv_os_gethostname.
pub fn os_hostname<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    // UV_MAXHOSTNAMESIZE on macOS, which is more than Linux's.
    let mut buffer = [0 as libc::c_char; 257];
    // SAFETY: the buffer has room for the length given.
    if unsafe { libc::gethostname(buffer.as_mut_ptr(), buffer.len()) } != 0 {
        return fail(scope, ctx(&args), last_error(), "uv_os_gethostname");
    }
    let name = c_bytes(&buffer[..buffer.len() - 1]);
    rv.set(utf8(scope, &name));
}

/// `getOSInformation(ctx)`: uv_os_uname's [sysname, version, release,
/// machine].
pub fn os_information<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    // SAFETY: utsname is plain data, and uname fills it in.
    let mut info: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut info) } == -1 {
        return fail(scope, ctx(&args), last_error(), "uv_os_uname");
    }
    let fields = [
        utf8(scope, &c_bytes(&info.sysname)),
        utf8(scope, &c_bytes(&info.version)),
        utf8(scope, &c_bytes(&info.release)),
        utf8(scope, &c_bytes(&info.machine)),
    ];
    rv.set(v8::Array::new_with_elements(scope, &fields).into());
}

/// One processor, as uv_cpu_info gives it: times in milliseconds.
struct Cpu {
    model: Vec<u8>,
    speed: u64,
    /// user, nice, sys, idle, irq.
    times: [u64; 5],
}

/// uv_cpu_info on Linux: times from /proc/stat, models from /proc/cpuinfo,
/// speeds from cpufreq.
#[cfg(target_os = "linux")]
fn cpu_info() -> Result<Vec<Cpu>, i32> {
    #[cfg(target_arch = "aarch64")]
    const MARKER: &str = "CPU part\t: ";
    #[cfg(not(target_arch = "aarch64"))]
    const MARKER: &str = "model name\t: ";
    // arm64 names its cores by part number.
    #[cfg(target_arch = "aarch64")]
    const PARTS: &[(&str, &str)] = &[
        ("0x811", "ARM810"),
        ("0x920", "ARM920"),
        ("0x922", "ARM922"),
        ("0x926", "ARM926"),
        ("0x940", "ARM940"),
        ("0x946", "ARM946"),
        ("0x966", "ARM966"),
        ("0xa20", "ARM1020"),
        ("0xa22", "ARM1022"),
        ("0xa26", "ARM1026"),
        ("0xb02", "ARM11 MPCore"),
        ("0xb36", "ARM1136"),
        ("0xb56", "ARM1156"),
        ("0xb76", "ARM1176"),
        ("0xc05", "Cortex-A5"),
        ("0xc07", "Cortex-A7"),
        ("0xc08", "Cortex-A8"),
        ("0xc09", "Cortex-A9"),
        ("0xc0d", "Cortex-A17"),
        ("0xc0f", "Cortex-A15"),
        ("0xc0e", "Cortex-A17"),
        ("0xc14", "Cortex-R4"),
        ("0xc15", "Cortex-R5"),
        ("0xc17", "Cortex-R7"),
        ("0xc18", "Cortex-R8"),
        ("0xc20", "Cortex-M0"),
        ("0xc21", "Cortex-M1"),
        ("0xc23", "Cortex-M3"),
        ("0xc24", "Cortex-M4"),
        ("0xc27", "Cortex-M7"),
        ("0xc60", "Cortex-M0+"),
        ("0xd01", "Cortex-A32"),
        ("0xd03", "Cortex-A53"),
        ("0xd04", "Cortex-A35"),
        ("0xd05", "Cortex-A55"),
        ("0xd06", "Cortex-A65"),
        ("0xd07", "Cortex-A57"),
        ("0xd08", "Cortex-A72"),
        ("0xd09", "Cortex-A73"),
        ("0xd0a", "Cortex-A75"),
        ("0xd0b", "Cortex-A76"),
        ("0xd0c", "Neoverse-N1"),
        ("0xd0d", "Cortex-A77"),
        ("0xd0e", "Cortex-A76AE"),
        ("0xd13", "Cortex-R52"),
        ("0xd20", "Cortex-M23"),
        ("0xd21", "Cortex-M33"),
        ("0xd41", "Cortex-A78"),
        ("0xd42", "Cortex-A78AE"),
        ("0xd4a", "Neoverse-E1"),
        ("0xd4b", "Cortex-A78C"),
        ("0xd4f", "Neoverse-V2"),
    ];
    const KERNEL_MAX: usize = 8192;

    let stat = std::fs::read("/proc/stat").map_err(|e| -e.raw_os_error().unwrap_or(libc::EIO))?;
    let stat = String::from_utf8_lossy(&stat);
    // Each processor's line, "cpuN user nice system idle iowait irq ...",
    // after the line that totals them, up to the first line that isn't one.
    let mut times: std::collections::BTreeMap<usize, ([u64; 5], usize, u64)> = Default::default();
    for line in stat.lines().skip(1) {
        let Some(rest) = line.strip_prefix("cpu") else {
            break;
        };
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(cpu) = rest[..digits].parse::<usize>() else {
            break;
        };
        let Some(numbers) = leading_numbers::<u64>(&rest[digits..], 6) else {
            break;
        };
        if cpu >= KERNEL_MAX {
            continue;
        }
        let entry = [numbers[0], numbers[1], numbers[2], numbers[3], numbers[5]];
        times.insert(cpu, (entry, 0, 0));
    }
    let maxcpu = times.keys().next_back().map_or(0, |&cpu| cpu + 1);

    // Up to eight models, the first "unknown". libuv's search for a
    // processor's model stops one past the slot that matched, and an empty
    // slot matches any model, so every processor /proc/cpuinfo names gets
    // slot 2, which holds the first model read.
    let mut models: [Vec<u8>; 8] = Default::default();
    models[0] = b"unknown".to_vec();
    if let Ok(cpuinfo) = std::fs::read("/proc/cpuinfo") {
        let mut lines = cpuinfo.split_inclusive(|&b| b == b'\n').peekable();
        while let Some(line) = lines.next() {
            // "processor\t: N", at the start of each processor's section.
            let Some(rest) = line.strip_prefix(b"processor\t: ") else {
                break;
            };
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            let Ok(cpu) = std::str::from_utf8(&rest[..digits])
                .unwrap_or("")
                .parse::<usize>()
            else {
                break;
            };
            // scanf's "\n" also eats the blank lines after it.
            while lines
                .peek()
                .is_some_and(|l| l.iter().all(u8::is_ascii_whitespace))
            {
                lines.next();
            }
            let mut found = None;
            for line in lines.by_ref() {
                if let Some(text) = line.strip_prefix(MARKER.as_bytes()) {
                    found = Some(text);
                    break;
                }
            }
            let Some(text) = found else { break };
            let text = &text[..text.iter().position(|&b| b == b'\n').unwrap_or(text.len())];
            #[cfg(target_arch = "aarch64")]
            let text = PARTS
                .iter()
                .find(|(part, _)| part.as_bytes() == text)
                .map_or(&b"unknown"[..], |(_, name)| name.as_bytes());
            let mut slot = 0;
            let mut matched = false;
            while !matched && slot < models.len() {
                matched = text.starts_with(&models[slot]);
                slot += 1;
            }
            if matched && slot < models.len() {
                if models[slot].is_empty() {
                    // snprintf into a 64-byte slot.
                    models[slot] = text[..text.len().min(63)].to_vec();
                }
                if cpu < maxcpu
                    && let Some(entry) = times.get_mut(&cpu)
                {
                    entry.1 = slot;
                }
            }
            // The rest of the section.
            for line in lines.by_ref() {
                if line == b"\n" {
                    break;
                }
            }
        }
    }

    for (cpu, entry) in times.iter_mut() {
        let path = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq");
        if let Some(freq) = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| leading_numbers::<u64>(&text, 1))
        {
            entry.2 = freq[0];
        }
    }
    // sysconf(_SC_CLK_TCK) is 100 on Linux: 10 milliseconds a tick.
    Ok(times
        .into_values()
        .map(|(times, model, freq)| Cpu {
            model: models[model].clone(),
            speed: freq / 1000,
            times: times.map(|t| t * 10),
        })
        .collect())
}

// libc's own declarations of these are deprecated in favour of the mach2
// crate, for two names.
#[cfg(target_os = "macos")]
unsafe extern "C" {
    static mach_task_self_: libc::mach_port_t;
    fn mach_host_self() -> libc::mach_port_t;
}

/// uv_cpu_info on macOS: the brand string, hw.cpufrequency, and each
/// processor's ticks from the Mach host.
#[cfg(target_os = "macos")]
fn cpu_info() -> Result<Vec<Cpu>, i32> {
    // SAFETY: sysconf can't fail for _SC_CLK_TCK.
    let multiplier = 1000 / unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;
    let mut model = [0 as libc::c_char; 512];
    let mut size = model.len();
    // SAFETY: each sysctl writes at most `size` bytes into what it's given.
    let failed = |name: &CStr, model: &mut [libc::c_char], size: &mut usize| unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            model.as_mut_ptr().cast(),
            size,
            std::ptr::null_mut(),
            0,
        ) != 0
    };
    if failed(c"machdep.cpu.brand_string", &mut model, &mut size) {
        size = model.len();
        if failed(c"hw.model", &mut model, &mut size) {
            return Err(last_error());
        }
    }
    let model = c_bytes(&model);
    let mut speed: u64 = 0;
    let mut size = size_of::<u64>();
    // SAFETY: speed has room for the u64 the sysctl gives.
    unsafe {
        libc::sysctlbyname(
            c"hw.cpufrequency".as_ptr(),
            (&raw mut speed).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if speed == 0 {
        // Apple's arm64 machines don't say; libuv guesses this.
        speed = 2_400_000_000;
    }
    let mut count: libc::natural_t = 0;
    let mut info: libc::processor_info_array_t = std::ptr::null_mut();
    let mut info_count: libc::mach_msg_type_number_t = 0;
    // SAFETY: the host fills in the count and an array it allocates, which
    // is deallocated below.
    let status = unsafe {
        libc::host_processor_info(
            mach_host_self(),
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut count,
            &mut info,
            &mut info_count,
        )
    };
    if status != libc::KERN_SUCCESS {
        return Err(-libc::EINVAL);
    }
    // SAFETY: the array holds `count` processor_cpu_load_info records.
    let loads = unsafe {
        std::slice::from_raw_parts(info.cast::<libc::processor_cpu_load_info>(), count as usize)
    };
    let cpus = loads
        .iter()
        .map(|load| {
            let ticks = load.cpu_ticks.map(|t| t as u64 * multiplier);
            Cpu {
                model: model.clone(),
                speed: speed / 1_000_000,
                // CPU_STATE_USER, NICE, SYSTEM, IDLE; macOS has no irq.
                times: [ticks[0], ticks[3], ticks[1], ticks[2], 0],
            }
        })
        .collect();
    // SAFETY: deallocates the array host_processor_info allocated.
    unsafe {
        libc::vm_deallocate(
            mach_task_self_,
            info as libc::vm_address_t,
            info_count as libc::vm_size_t * size_of::<libc::integer_t>(),
        )
    };
    Ok(cpus)
}

/// `getCPUs()`: [model, speed, user, nice, sys, idle, irq, model, ...], or
/// undefined when uv_cpu_info fails.
pub fn os_cpus(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    let Ok(cpus) = cpu_info() else { return };
    let mut items = Vec::with_capacity(cpus.len() * 7);
    for cpu in cpus {
        items.push(one_byte(scope, &cpu.model));
        items.push(v8::Number::new(scope, cpu.speed as f64).into());
        for time in cpu.times {
            items.push(v8::Number::new(scope, time as f64).into());
        }
    }
    rv.set(v8::Array::new_with_elements(scope, &items).into());
}

/// uv__read_proc_meminfo: a /proc/meminfo field, in bytes, or 0.
#[cfg(target_os = "linux")]
fn meminfo(field: &str) -> u64 {
    let Some(text) = slurp("/proc/meminfo", 4096) else {
        return 0;
    };
    let Some(at) = text.find(field) else { return 0 };
    leading_numbers::<u64>(&text[at + field.len()..], 1).map_or(0, |n| n[0] * 1024)
}

#[cfg(target_os = "linux")]
fn sysinfo() -> Option<libc::sysinfo> {
    // SAFETY: sysinfo is plain data, and sysinfo() fills it in.
    let mut info: libc::sysinfo = unsafe { std::mem::zeroed() };
    (unsafe { libc::sysinfo(&mut info) } == 0).then_some(info)
}

/// uv_get_free_memory.
fn free_memory() -> u64 {
    #[cfg(target_os = "linux")]
    {
        match meminfo("MemAvailable:") {
            0 => sysinfo().map_or(0, |info| info.freeram * u64::from(info.mem_unit)),
            available => available,
        }
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: vm_statistics is plain data the host fills in, `count`
        // words of it.
        let mut info: libc::vm_statistics = unsafe { std::mem::zeroed() };
        let mut count = (size_of::<libc::vm_statistics>() / size_of::<libc::integer_t>()) as u32;
        let status = unsafe {
            libc::host_statistics(
                mach_host_self(),
                libc::HOST_VM_INFO,
                (&raw mut info).cast(),
                &mut count,
            )
        };
        if status != libc::KERN_SUCCESS {
            return 0;
        }
        // SAFETY: sysconf can't fail for _SC_PAGESIZE.
        info.free_count as u64 * unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64
    }
}

/// uv_get_total_memory.
fn total_memory() -> u64 {
    #[cfg(target_os = "linux")]
    {
        match meminfo("MemTotal:") {
            0 => sysinfo().map_or(0, |info| info.totalram * u64::from(info.mem_unit)),
            total => total,
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut total: u64 = 0;
        let mut size = size_of::<u64>();
        let mut which = [libc::CTL_HW, libc::HW_MEMSIZE];
        // SAFETY: total has room for the u64 the sysctl gives.
        let failed = unsafe {
            libc::sysctl(
                which.as_mut_ptr(),
                2,
                (&raw mut total).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0;
        if failed { 0 } else { total }
    }
}

/// `getFreeMem()`.
pub fn os_free_memory(_: &mut v8::PinScope, _: Args, mut rv: Ret) {
    rv.set_double(free_memory() as f64);
}

/// `getTotalMem()`.
pub fn os_total_memory(_: &mut v8::PinScope, _: Args, mut rv: Ret) {
    rv.set_double(total_memory() as f64);
}

/// uv_uptime, in seconds.
fn uptime() -> Result<f64, i32> {
    #[cfg(target_os = "linux")]
    {
        // /proc/uptime first: CLOCK_BOOTTIME can be wrong in containers.
        if let Some(seconds) =
            slurp("/proc/uptime", 128).and_then(|t| leading_numbers::<f64>(&t, 1))
        {
            return Ok(seconds[0]);
        }
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime fills in the timespec.
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut now) } != 0 {
            return Err(last_error());
        }
        Ok(now.tv_sec as f64)
    }
    #[cfg(target_os = "macos")]
    {
        let mut boot = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        let mut size = size_of::<libc::timeval>();
        let mut which = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        // SAFETY: boot has room for the timeval the sysctl gives.
        let failed = unsafe {
            libc::sysctl(
                which.as_mut_ptr(),
                2,
                (&raw mut boot).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0;
        if failed {
            return Err(last_error());
        }
        // SAFETY: time accepts a null pointer.
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        Ok((now - boot.tv_sec) as f64)
    }
}

/// `getUptime(ctx)`.
pub fn os_uptime<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    match uptime() {
        Ok(seconds) => rv.set_double(seconds),
        Err(errno) => fail(scope, ctx(&args), errno, "uv_uptime"),
    }
}

/// uv_loadavg: the 1, 5 and 15 minute load averages, or zeros.
fn load_average() -> [f64; 3] {
    #[cfg(target_os = "linux")]
    {
        if let Some(avg) = slurp("/proc/loadavg", 128).and_then(|t| leading_numbers::<f64>(&t, 3)) {
            return [avg[0], avg[1], avg[2]];
        }
        sysinfo().map_or([0.0; 3], |info| {
            info.loads.map(|load| load as f64 / 65536.0)
        })
    }
    #[cfg(target_os = "macos")]
    {
        // struct loadavg (sys/resource.h).
        #[repr(C)]
        struct LoadAvg {
            ldavg: [u32; 3],
            fscale: libc::c_long,
        }
        let mut info = LoadAvg {
            ldavg: [0; 3],
            fscale: 0,
        };
        let mut size = size_of::<LoadAvg>();
        let mut which = [libc::CTL_VM, libc::VM_LOADAVG];
        // SAFETY: info has the layout and size the sysctl fills in.
        let failed = unsafe {
            libc::sysctl(
                which.as_mut_ptr(),
                2,
                (&raw mut info).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } < 0;
        if failed {
            return [0.0; 3];
        }
        info.ldavg.map(|load| load as f64 / info.fscale as f64)
    }
}

/// `getLoadAvg(array)`: fills in a Float64Array of three.
pub fn os_load_avg(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let Ok(array) = v8::Local::<v8::Float64Array>::try_from(args.get(0)) else {
        return super::type_error(scope, "getLoadAvg needs a Float64Array of three");
    };
    let bytes: Vec<u8> = load_average()
        .iter()
        .flat_map(|n| n.to_ne_bytes())
        .collect();
    let Some(view) = super::buffer::bytes(scope, array.into()) else {
        return;
    };
    let n = view.len().min(bytes.len());
    view[..n].copy_from_slice(&bytes[..n]);
}

/// One address of an interface, as uv_interface_addresses gives it.
struct Address {
    name: Vec<u8>,
    /// Some(scope id) for IPv6.
    address: std::net::IpAddr,
    netmask: std::net::IpAddr,
    v6_scope: Option<u32>,
    internal: bool,
    mac: [u8; 6],
}

/// The address a sockaddr holds, read as `v6` says (the family of the
/// interface's address, which libuv reads its netmask as too).
///
/// # Safety
/// `addr` is null or points at a sockaddr of at least that family's size.
unsafe fn ip_of(addr: *const libc::sockaddr, v6: bool) -> std::net::IpAddr {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    if addr.is_null() {
        return if v6 {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        };
    }
    if v6 {
        // SAFETY: the caller's.
        let addr = unsafe { &*addr.cast::<libc::sockaddr_in6>() };
        IpAddr::V6(Ipv6Addr::from(addr.sin6_addr.s6_addr))
    } else {
        // SAFETY: the caller's.
        let addr = unsafe { &*addr.cast::<libc::sockaddr_in>() };
        IpAddr::V4(Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr)))
    }
}

/// uv_interface_addresses: each address of each interface that is up and
/// running, with the hardware address of the interface it's on.
fn interface_addresses() -> Result<Vec<Address>, i32> {
    #[cfg(target_os = "linux")]
    const LINK: i32 = libc::AF_PACKET;
    #[cfg(target_os = "macos")]
    const LINK: i32 = libc::AF_LINK;

    let mut addrs: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills in a list freed below.
    if unsafe { libc::getifaddrs(&mut addrs) } != 0 {
        return Err(last_error());
    }
    // SAFETY: the list getifaddrs made, until freeifaddrs.
    let entries = std::iter::successors(unsafe { addrs.as_ref() }, |ent| unsafe {
        ent.ifa_next.as_ref()
    });
    let up = |ent: &libc::ifaddrs| {
        let flags = ent.ifa_flags as i32;
        flags & libc::IFF_UP != 0 && flags & libc::IFF_RUNNING != 0 && !ent.ifa_addr.is_null()
    };
    // SAFETY: an entry that is up has an address.
    let family = |ent: &libc::ifaddrs| unsafe { (*ent.ifa_addr).sa_family } as i32;
    let mut found = Vec::new();
    for ent in entries.clone().filter(|ent| up(ent) && family(ent) != LINK) {
        let v6 = family(ent) == libc::AF_INET6;
        // SAFETY: getifaddrs gives each entry a name, and an address (and
        // netmask, if any) of the size its family needs.
        let name = unsafe { CStr::from_ptr(ent.ifa_name) }.to_bytes().to_vec();
        let address = unsafe { ip_of(ent.ifa_addr, v6) };
        let netmask = unsafe { ip_of(ent.ifa_netmask, v6) };
        let v6_scope =
            v6.then(|| unsafe { (*ent.ifa_addr.cast::<libc::sockaddr_in6>()).sin6_scope_id });
        let internal = ent.ifa_flags as i32 & libc::IFF_LOOPBACK != 0;
        found.push(Address {
            name,
            address,
            netmask,
            v6_scope,
            internal,
            mac: [0; 6],
        });
    }
    for ent in entries.filter(|ent| up(ent) && family(ent) == LINK) {
        // SAFETY: as above.
        let name = unsafe { CStr::from_ptr(ent.ifa_name) }.to_bytes();
        #[cfg(target_os = "linux")]
        // SAFETY: an AF_PACKET entry's address is a sockaddr_ll.
        let mac = unsafe { (*ent.ifa_addr.cast::<libc::sockaddr_ll>()).sll_addr };
        #[cfg(target_os = "macos")]
        // SAFETY: an AF_LINK entry's address is a sockaddr_dl, its hardware
        // address after the name in sdl_data (LLADDR).
        let mac = unsafe {
            let dl = ent.ifa_addr.cast::<libc::sockaddr_dl>();
            let data = (&raw const (*dl).sdl_data)
                .cast::<u8>()
                .add((*dl).sdl_nlen as usize);
            std::ptr::read_unaligned(data.cast::<[u8; 6]>())
        };
        for address in &mut found {
            // On Linux an alias ("eth0:1") shares its interface's address.
            let same = if cfg!(target_os = "linux") {
                address.name.starts_with(name)
                    && matches!(address.name.get(name.len()), None | Some(b':'))
            } else {
                address.name == name
            };
            if same {
                address.mac.copy_from_slice(&mac[..6]);
            }
        }
    }
    // SAFETY: frees the list getifaddrs made; nothing refers to it now.
    unsafe { libc::freeifaddrs(addrs) };
    Ok(found)
}

/// uv_inet_ntop for IPv6, BIND's: the longest run of two or more zero
/// groups is `::`, and an IPv4-compatible or -mapped address ends in dotted
/// quad.
fn ipv6_text(address: std::net::Ipv6Addr) -> String {
    let words = address.segments();
    let (mut best, mut cur): ((i32, i32), (i32, i32)) = ((-1, 0), (-1, 0));
    for (i, &word) in words.iter().enumerate() {
        if word == 0 {
            if cur.0 == -1 {
                cur = (i as i32, 1);
            } else {
                cur.1 += 1;
            }
        } else if cur.0 != -1 {
            if best.0 == -1 || cur.1 > best.1 {
                best = cur;
            }
            cur.0 = -1;
        }
    }
    if cur.0 != -1 && (best.0 == -1 || cur.1 > best.1) {
        best = cur;
    }
    if best.0 != -1 && best.1 < 2 {
        best.0 = -1;
    }
    let mut text = String::new();
    for (i, &word) in words.iter().enumerate() {
        let i = i as i32;
        if best.0 != -1 && i >= best.0 && i < best.0 + best.1 {
            if i == best.0 {
                text.push(':');
            }
            continue;
        }
        if i != 0 {
            text.push(':');
        }
        if i == 6
            && best.0 == 0
            && (best.1 == 6
                || (best.1 == 7 && words[7] != 1)
                || (best.1 == 5 && words[5] == 0xffff))
        {
            let [.., a, b, c, d] = address.octets();
            text.push_str(&format!("{a}.{b}.{c}.{d}"));
            return text;
        }
        text.push_str(&format!("{word:x}"));
    }
    if best.0 != -1 && best.0 + best.1 == 8 {
        text.push(':');
    }
    text
}

fn ip_text(address: std::net::IpAddr) -> String {
    match address {
        std::net::IpAddr::V4(address) => address.to_string(),
        std::net::IpAddr::V6(address) => ipv6_text(address),
    }
}

/// `getInterfaceAddresses(ctx)`: [name, address, netmask, family, mac,
/// internal, scopeid, name, ...].
pub fn os_interface_addresses<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: Args<'s>,
    mut rv: Ret<'s>,
) {
    let addresses = match interface_addresses() {
        Ok(addresses) => addresses,
        Err(errno) => return fail(scope, ctx(&args), errno, "uv_interface_addresses"),
    };
    let mut items = Vec::with_capacity(addresses.len() * 7);
    for address in addresses {
        items.push(utf8(scope, &address.name));
        let text = ip_text(address.address);
        items.push(one_byte(scope, text.as_bytes()));
        let text = ip_text(address.netmask);
        items.push(one_byte(scope, text.as_bytes()));
        let family = if address.v6_scope.is_some() {
            "IPv6"
        } else {
            "IPv4"
        };
        items.push(string(scope, family).into());
        let mac = address.mac.map(|b| format!("{b:02x}")).join(":");
        items.push(one_byte(scope, mac.as_bytes()));
        items.push(v8::Boolean::new(scope, address.internal).into());
        items.push(match address.v6_scope {
            Some(id) => v8::Integer::new_from_unsigned(scope, id).into(),
            None => v8::Integer::new(scope, -1).into(),
        });
    }
    rv.set(v8::Array::new_with_elements(scope, &items).into());
}

/// The effective user's entry in the password database (uv_os_get_passwd):
/// username, home directory, shell, uid, gid.
struct Passwd {
    username: Vec<u8>,
    homedir: Vec<u8>,
    shell: Vec<u8>,
    uid: u32,
    gid: u32,
}

fn passwd() -> Result<Passwd, i32> {
    // SAFETY: geteuid can't fail.
    let uid = unsafe { libc::geteuid() };
    let mut size = 2000;
    loop {
        let mut buffer = vec![0 as libc::c_char; size];
        // SAFETY: passwd is plain data; getpwuid_r fills it in with
        // pointers into `buffer`, which outlives their use.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let r = loop {
            let r = unsafe {
                libc::getpwuid_r(uid, &mut entry, buffer.as_mut_ptr(), size, &mut result)
            };
            if r != libc::EINTR {
                break r;
            }
        };
        if r == libc::ERANGE {
            size *= 2;
            continue;
        }
        if r != 0 {
            return Err(-r);
        }
        if result.is_null() {
            return Err(-libc::ENOENT);
        }
        // SAFETY: the entry's strings point into `buffer`.
        let text = |p: *const libc::c_char| unsafe { CStr::from_ptr(p) }.to_bytes().to_vec();
        return Ok(Passwd {
            username: text(entry.pw_name),
            homedir: text(entry.pw_dir),
            shell: text(entry.pw_shell),
            uid: entry.pw_uid,
            gid: entry.pw_gid,
        });
    }
}

/// `getHomeDirectory(ctx)`: uv_os_homedir, $HOME if it's set (even to
/// nothing), else the password database's.
pub fn os_home_directory<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    use std::os::unix::ffi::OsStrExt;
    if let Some(home) = std::env::var_os("HOME") {
        return rv.set(utf8(scope, home.as_bytes()));
    }
    match passwd() {
        Ok(entry) => rv.set(utf8(scope, &entry.homedir)),
        Err(errno) => fail(scope, ctx(&args), errno, "uv_os_homedir"),
    }
}

/// `getUserInfo(options, ctx)`: { uid, gid, username, homedir, shell },
/// with no prototype, the strings in `options.encoding`.
pub fn os_user_info<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let encoding = match v8::Local::<v8::Object>::try_from(args.get(0)) {
        Ok(options) => {
            let key = string(scope, "encoding");
            let Some(value) = options.get(scope, key.into()) else {
                return;
            };
            Encoding::from_value(scope, value)
        }
        Err(_) => Encoding::Utf8,
    };
    let entry = match passwd() {
        Ok(entry) => entry,
        Err(errno) => return fail(scope, ctx(&args), errno, "uv_os_get_passwd"),
    };
    let names: Vec<v8::Local<v8::Name>> = ["uid", "gid", "username", "homedir", "shell"]
        .iter()
        .map(|name| string(scope, name).into())
        .collect();
    let values = [
        v8::Number::new(scope, entry.uid as f64).into(),
        v8::Number::new(scope, entry.gid as f64).into(),
        super::requests::encoded(scope, &entry.username, encoding),
        super::requests::encoded(scope, &entry.homedir, encoding),
        super::requests::encoded(scope, &entry.shell, encoding),
    ];
    let null = v8::null(scope).into();
    rv.set(v8::Object::with_prototype_and_properties(scope, null, &names, &values).into());
}

/// `setPriority(pid, priority, ctx)`: 0, or the error it filled `ctx` in
/// with.
pub fn os_set_priority<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let pid = args.get(0).int32_value(scope).unwrap_or(0);
    let priority = args.get(1).int32_value(scope).unwrap_or(0);
    let error = if !(-20..=19).contains(&priority) {
        -libc::EINVAL
        // SAFETY: setpriority accepts any process and priority.
    } else if unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, priority) } != 0 {
        last_error()
    } else {
        0
    };
    if error != 0 {
        fail(scope, args.get(2), error, "uv_os_setpriority");
    }
    rv.set_int32(error);
}

/// `getPriority(pid, ctx)`.
pub fn os_get_priority<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    let pid = args.get(0).int32_value(scope).unwrap_or(0);
    // getpriority returns -1 for a priority of -1 too; errno tells them
    // apart.
    // SAFETY: errno is this thread's, and getpriority accepts any process.
    let priority = unsafe {
        *errno_location() = 0;
        libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t)
    };
    // SAFETY: as above.
    let errno = unsafe { *errno_location() };
    if priority == -1 && errno != 0 {
        return fail(scope, args.get(1), -errno, "uv_os_getpriority");
    }
    rv.set_int32(priority);
}

#[cfg(target_os = "linux")]
unsafe fn errno_location() -> *mut i32 {
    unsafe { libc::__errno_location() }
}

#[cfg(target_os = "macos")]
unsafe fn errno_location() -> *mut i32 {
    unsafe { libc::__error() }
}

/// uv__get_constrained_cpu: the CPUs a cgroup's quota allows, when it
/// sets one.
#[cfg(target_os = "linux")]
fn cpu_quota() -> Option<i64> {
    let cgroup = slurp("/proc/self/cgroup", 1024)?;
    if let Some(rest) = cgroup.strip_prefix("0::/") {
        // cgroup v2: cpu.max at each level from this cgroup up to the root.
        // libuv keeps the last quota it reads, the one nearest the root.
        let path = &rest[..rest.find('\n').unwrap_or(rest.len())];
        let mut dir = format!("/sys/fs/cgroup/{path}");
        slurp(&format!("{dir}/cgroup.controllers"), 1024)?;
        let mut quota = None;
        loop {
            if let Some(max) = slurp(&format!("{dir}/cpu.max"), 16)
                && !max.starts_with("max")
                && let Some([limit, period]) = leading_numbers::<i64>(&max, 2).map(|n| [n[0], n[1]])
                && period != 0
            {
                quota = Some((limit / period).max(1));
            }
            if dir == "/sys/fs/cgroup" {
                break;
            }
            match dir.rfind('/') {
                Some(slash) => dir.truncate(slash),
                None => break,
            }
            if !dir.starts_with("/sys/fs/cgroup") {
                break;
            }
        }
        quota
    } else {
        // cgroup v1: the cpu controller's cfs quota over its period.
        let at = cgroup.find(":cpu,")? + ":cpu,".len();
        let rest = &cgroup[at..];
        let path = &rest[..rest.find('\n').unwrap_or(rest.len())];
        let read = |file: &str| {
            slurp(&format!("/sys/fs/cgroup/{path}/{file}"), 1024)
                .and_then(|text| leading_numbers::<i64>(&text, 1))
                .map(|n| n[0])
        };
        let quota = read("cpu.cfs_quota_us")?;
        let period = read("cpu.cfs_period_us").filter(|&p| p != 0)?;
        Some(quota / period)
    }
}

/// uv_available_parallelism.
fn available_parallelism() -> u32 {
    #[allow(unused_mut)]
    let mut count: i64 = -1;
    #[cfg(target_os = "linux")]
    {
        // SAFETY: cpu_set_t is plain data that sched_getaffinity fills in.
        let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::sched_getaffinity(0, size_of::<libc::cpu_set_t>(), &mut set) } == 0 {
            // SAFETY: the set sched_getaffinity filled in.
            count = unsafe { libc::CPU_COUNT(&set) } as i64;
        }
    }
    #[cfg(target_os = "macos")]
    for name in [c"hw.activecpu", c"hw.logicalcpu", c"hw.ncpu"] {
        let mut n: libc::c_int = 0;
        let mut size = size_of::<libc::c_int>();
        // SAFETY: n has room for the int the sysctl gives.
        let ok = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&raw mut n).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } == 0;
        if ok && size == size_of::<libc::c_int>() && n > 0 {
            count = n as i64;
            break;
        }
    }
    if count < 0 {
        // SAFETY: sysconf accepts any name.
        count = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) } as i64;
    }
    #[cfg(target_os = "linux")]
    if let Some(quota) = cpu_quota()
        && quota > 0
        && quota < count
    {
        count = quota;
    }
    count.max(1) as u32
}

/// `getAvailableParallelism()`.
pub fn os_available_parallelism(_: &mut v8::PinScope, _: Args, mut rv: Ret) {
    rv.set_uint32(available_parallelism());
}

/// SafeGetenv: an environment variable, unless the process runs with more
/// privilege than whoever started it (set-user-ID or set-group-ID, or
/// AT_SECURE on Linux), when nothing in the environment is trusted. Node
/// lets a process whose only capability is CAP_NET_BIND_SERVICE read it
/// anyway; Opal doesn't look at capabilities.
fn safe_env(key: &str) -> Option<std::ffi::OsString> {
    // SAFETY: these calls can't fail.
    let differs = unsafe { libc::getuid() != libc::geteuid() || libc::getgid() != libc::getegid() };
    #[cfg(target_os = "linux")]
    // SAFETY: getauxval accepts any type.
    let secure = unsafe { libc::getauxval(libc::AT_SECURE) } != 0;
    #[cfg(not(target_os = "linux"))]
    let secure = false;
    if differs || secure {
        return None;
    }
    std::env::var_os(key)
}

/// `safeGetenv(key)`: the variable, or undefined.
pub fn safe_getenv<'s>(scope: &mut v8::PinScope<'s, '_>, args: Args<'s>, mut rv: Ret<'s>) {
    use std::os::unix::ffi::OsStrExt;
    let key = args.get(0).to_rust_string_lossy(scope);
    if let Some(value) = safe_env(&key) {
        rv.set(utf8(scope, value.as_bytes()));
    }
}

/// `getTempDir()`: the first of $TMPDIR, $TMP and $TEMP that isn't empty,
/// less one trailing slash, or undefined.
pub fn temp_dir(scope: &mut v8::PinScope, _: Args, mut rv: Ret) {
    use std::os::unix::ffi::OsStrExt;
    let Some(dir) = ["TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(|key| safe_env(key))
        .find(|dir| !dir.is_empty())
    else {
        return;
    };
    let mut dir = dir.as_bytes();
    if dir.len() > 1 && dir.ends_with(b"/") {
        dir = &dir[..dir.len() - 1];
    }
    rv.set(utf8(scope, dir));
}

/// A name the C environment can hold: not empty, without `=` or NUL.
fn env_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['=', '\0'])
}

/// `setenv(key, value)`: what an assignment to process.env does in Node,
/// besides storing the string: sets the process's environment (which child
/// processes, os.homedir and os.tmpdir see), ignoring what it can't hold.
pub fn setenv(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let key = args.get(0).to_rust_string_lossy(scope);
    let value = args.get(1).to_rust_string_lossy(scope);
    // A value is cut at a NUL, as the C string Node passes on is.
    let value = value.split('\0').next().unwrap_or("");
    if env_name(&key) {
        // SAFETY: Node does the same from its main thread; the I/O pool's
        // threads make file system calls, which don't read the environment.
        unsafe { std::env::set_var(key, value) };
    }
}

/// `unsetenv(key)`: what `delete process.env[key]` does in Node.
pub fn unsetenv(scope: &mut v8::PinScope, args: Args, _: Ret) {
    let key = args.get(0).to_rust_string_lossy(scope);
    if env_name(&key) {
        // SAFETY: as in setenv.
        unsafe { std::env::remove_var(key) };
    }
}
