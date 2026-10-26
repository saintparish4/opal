//! Stacks big enough to parse a file.
//!
//! oxc's parser recurses once per level of nesting and has no stack check,
//! and so does a walk over the tree it builds: a module nested deeper than
//! its thread's stack holds aborts the process. A level of nesting takes at
//! least one byte of source, so a stack sized to the file always holds it.
//! Threads this module starts record their stack, so [`with_room_for`] can
//! run a parse in place when it fits and on a thread sized for the file
//! when it does not.
//!
//! `crates/opal-bundler/examples/parse-stack.rs` measured parsing and then
//! walking a module, over twenty-one forms of nesting and seven of
//! brackets. The worst were 299 bytes of stack per byte of source
//! (`a=>a=>…`) and 1,802 per bracket (`()=>{…}`) in a release build, and
//! 514 (`!!…`, walked) and 3,899 (`function f(){…}`) in a debug build.
//!
//! [`bracket_depth`] counts brackets in strings, comments, and regular
//! expressions too, so a file can hide a level of brackets from it with a
//! `)` in a comment. Each hidden level still takes two bytes, the bracket
//! and the `)`, so [`PER_BYTE`] is at least half the worst bracket, and
//! the count only adds to it. Both constants are twice what that needs.

use std::cell::Cell;
use std::sync::Mutex;

/// Stack per byte of source, whatever form its nesting takes, brackets
/// hidden from [`bracket_depth`] included.
const PER_BYTE: usize = if cfg!(debug_assertions) { 3_900 } else { 1_800 };

/// Stack per level of `(`, `[`, or `{` that [`bracket_depth`] counts, on top
/// of [`PER_BYTE`].
const PER_BRACKET: usize = if cfg!(debug_assertions) {
    8 << 10
} else {
    4 << 10
};

/// What else is on the stack: the caller's frames, and the parser's and the
/// walk's own at the top level.
const BASE: usize = 1 << 20;

thread_local! {
    /// Where this thread's stack started and how big it is, if [`run`] or
    /// [`enter`] recorded it.
    static STACK: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// The most stack parsing `source` and walking its tree can take.
pub fn needed(source: &str) -> usize {
    BASE.saturating_add(bracket_depth(source).saturating_mul(PER_BRACKET))
        .saturating_add(source.len().saturating_mul(PER_BYTE))
}

/// How deep `(`, `[`, and `{` nest in `source`, counting those in strings,
/// comments, and regular expressions too, so it can only overstate.
pub fn bracket_depth(source: &str) -> usize {
    let (mut depth, mut deepest) = (0usize, 0usize);
    for byte in source.bytes() {
        match byte {
            b'(' | b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// Records that the current thread has `size` bytes of stack. A thread
/// that calls it first thing lets [`with_room_for`] parse in place.
pub fn enter(size: usize) {
    STACK.set(Some((position(), size)));
}

/// How much of this thread's stack is left, if [`enter`] recorded it.
pub fn remaining() -> Option<usize> {
    STACK
        .get()
        .map(|(start, size)| size.saturating_sub(start.abs_diff(position())))
}

/// Runs `work` on a new thread with `size` bytes of stack, recorded for
/// [`with_room_for`], and returns what it returns. A panic in `work`
/// resumes here.
pub fn run<T: Send>(
    name: &str,
    size: usize,
    work: impl FnOnce() -> T + Send,
) -> std::io::Result<T> {
    std::thread::scope(|scope| {
        let thread = std::thread::Builder::new()
            .name(name.to_string())
            .stack_size(size)
            .spawn_scoped(scope, move || {
                enter(size);
                work()
            })?;
        Ok(thread
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
    })
}

/// Runs `work`, which parses `source` and may walk its tree, in place when
/// this thread's stack has room for it, and otherwise on a thread with a
/// stack sized for `source`. A thread whose stack the OS will not reserve
/// (a file of many megabytes asks for gigabytes) is not started, and
/// `work` runs in place, as it would without this module.
pub fn with_room_for<T: Send>(source: &str, work: impl FnOnce() -> T + Send) -> T {
    let needed = needed(source);
    if remaining().is_some_and(|left| left >= needed) {
        return work();
    }
    // `work` has to survive a thread that fails to start, so the thread
    // takes it from here.
    let slot = Mutex::new(Some(work));
    let take = || slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    match run("opal-parse", needed, || take().map(|work| work())) {
        Ok(Some(result)) => result,
        Ok(None) => unreachable!("the parse thread took the work and returned nothing"),
        Err(_) => take().expect("a thread that failed to start ran nothing")(),
    }
}

/// An address on the current thread's stack.
fn position() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bracket_depth_is_the_deepest_point_across_kinds() {
        assert_eq!(bracket_depth("f([{}]) + g(1)"), 3);
        assert_eq!(bracket_depth("no brackets"), 0);
        // Closing more than was opened does not go below zero.
        assert_eq!(bracket_depth(")))(("), 2);
    }

    #[test]
    fn test_needed_grows_with_size_and_brackets() {
        assert!(needed("a") < needed(&"a".repeat(1000)));
        assert!(needed(&"a".repeat(1000)) < needed(&"(".repeat(1000)));
    }

    #[test]
    fn test_remaining_is_known_only_on_a_recorded_thread() {
        std::thread::spawn(|| assert_eq!(remaining(), None))
            .join()
            .unwrap();
        let left = run("test", 4 << 20, remaining).unwrap().unwrap();
        assert!(left > 3 << 20 && left <= 4 << 20, "{left}");
    }

    #[test]
    fn test_work_runs_in_place_when_it_fits_and_elsewhere_when_it_does_not() {
        let here = |source: &'static str| {
            run("test", 64 << 20, move || {
                let caller = std::thread::current().id();
                with_room_for(source, || std::thread::current().id()) == caller
            })
            .unwrap()
        };
        assert!(here("export const small = 1;"));
        let big: &'static str = "a".repeat(200 << 10).leak();
        assert!(!here(big));
    }
}
