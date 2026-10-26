//! Running a child while opal waits for it, and leaving the way it left.
//!
//! A signal from the terminal (Ctrl-C, Ctrl-\) already reaches the child,
//! which is in the same foreground process group, so opal only has to not die
//! of it first. A signal sent to opal's pid by another process (`kill`, a
//! supervisor stopping a service) reaches opal alone, so opal passes it on.
//! On Linux the two are told apart by `si_code`: the kernel marks one that
//! came from `kill(2)` or `sigqueue(3)`. macOS leaves `si_code` 0 for both,
//! so there a terminal's signals (SIGINT, SIGQUIT and SIGHUP) are taken to
//! be the terminal's while opal is in its foreground process group, and
//! everything else, SIGTERM included, which no terminal sends, to be a
//! process's. npm forwards both kinds, so a Ctrl-C arrives at the child
//! twice, and a program that reads a second Ctrl-C as "force quit" quits at
//! the first; opal forwards only the second kind.

use std::io;
use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::scripts::{Outcome, outcome};

const FORWARDED: [libc::c_int; 4] = [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP];

static CHILD: AtomicI32 = AtomicI32::new(0);

/// Whether opal is in its terminal's foreground process group, so that the
/// terminal's signals reach the child without opal (macOS).
#[cfg(target_os = "macos")]
static FOREGROUND: AtomicBool = AtomicBool::new(false);

/// Whether a signal opal received was sent by a process rather than by the
/// terminal (see the module's comment).
#[cfg(not(target_os = "macos"))]
fn sent_by_a_process(_: libc::c_int, info: &libc::siginfo_t) -> bool {
    info.si_code == libc::SI_USER || info.si_code == libc::SI_QUEUE
}

#[cfg(target_os = "macos")]
fn sent_by_a_process(signal: libc::c_int, _: &libc::siginfo_t) -> bool {
    let from_terminal = matches!(signal, libc::SIGINT | libc::SIGQUIT | libc::SIGHUP);
    !(from_terminal && FOREGROUND.load(Ordering::Relaxed))
}

extern "C" fn forward(signal: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
    // SAFETY: the kernel passes a valid siginfo_t to an SA_SIGINFO handler.
    let info = unsafe { &*info };
    let child = CHILD.load(Ordering::Relaxed);
    if child > 0 && sent_by_a_process(signal, info) {
        // SAFETY: kill is async-signal-safe.
        unsafe { libc::kill(child, signal) };
    }
}

/// Starts `command`, forwards signals sent to opal to it while it runs, and
/// waits for it.
pub fn run_forwarding(command: &mut Command) -> io::Result<Outcome> {
    install();
    let mut child = command.spawn()?;
    CHILD.store(child.id() as i32, Ordering::Relaxed);
    let status = child.wait();
    CHILD.store(0, Ordering::Relaxed);
    Ok(outcome(status?))
}

fn install() {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: open, tcgetpgrp, getpgrp and close only read state; a
        // process with no terminal fails the open.
        let foreground = unsafe {
            let tty = libc::open(c"/dev/tty".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
            let foreground = tty >= 0 && libc::tcgetpgrp(tty) == libc::getpgrp();
            if tty >= 0 {
                libc::close(tty);
            }
            foreground
        };
        FOREGROUND.store(foreground, Ordering::Relaxed);
    }
    for signal in FORWARDED {
        // SAFETY: a zeroed sigaction is valid; the handler has the
        // SA_SIGINFO signature and does only async-signal-safe work.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = forward as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
}

/// Ends opal the way a child ended: with its exit code, or by its signal, so
/// the shell that started opal reports what it would have reported for the
/// child (`130` after Ctrl-C, and so on).
pub fn leave(outcome: Outcome) -> ! {
    crate::trace::checkpoint("exit");
    crate::trace::finish();
    match outcome {
        Outcome::Exited(code) => std::process::exit(code),
        Outcome::Signaled(signal) => {
            // SAFETY: restoring the default action and raising the signal on
            // ourselves; nothing here depends on memory safety.
            unsafe {
                libc::signal(signal, libc::SIG_DFL);
                let mut set: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, signal);
                libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
                libc::raise(signal);
            }
            // A signal whose default action is not to terminate.
            std::process::exit(128 + signal)
        }
    }
}
