//! Running independent work on several threads, and nothing more.
//!
//! Both parallel paths in this crate have the same shape: a slice of items
//! that do not depend on each other, a closure that may fail, and a per-thread
//! accumulator merged at the end. This is that, in `std`, rather than a
//! scheduler dependency for a loop.
//!
//! Ordering is *not* preserved and must not be relied on. The linker's
//! ordering requirement is handled by its caller, which runs one depth level
//! at a time — a rule that lives there because it is about `node_modules`, not
//! about threads.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How many threads to use for filesystem work.
///
/// Filesystem work is syscall-bound, so this is not a CPU count question — the
/// useful number is "how many requests the kernel will overlap". Capped
/// because past a point the queue depth stops helping and context switching
/// starts costing.
pub fn workers() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().clamp(1, 16))
        .unwrap_or(4)
}

/// Applies `work` to every item, on `threads` threads, folding into `T`.
///
/// The first error wins and the rest of the work is abandoned; a partially
/// applied batch is safe here because every operation underneath is
/// idempotent, which is the same property that lets a killed install converge
/// by re-running.
pub fn each<Item, T, E, W, F>(items: &[Item], threads: usize, seed: F, work: W) -> Result<Vec<T>, E>
where
    Item: Sync,
    T: Send,
    E: Send,
    W: Fn(&Item, &mut T) -> Result<(), E> + Sync,
    F: Fn() -> T + Sync,
{
    let next = AtomicUsize::new(0);
    let failure: Mutex<Option<E>> = Mutex::new(None);
    let mut folded = Vec::new();

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads.max(1))
            .map(|_| {
                scope.spawn(|| {
                    let mut accumulator = seed();
                    loop {
                        if failure.lock().is_ok_and(|held| held.is_some()) {
                            break;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else { break };
                        if let Err(error) = work(item, &mut accumulator) {
                            if let Ok(mut held) = failure.lock() {
                                held.get_or_insert(error);
                            }
                            break;
                        }
                    }
                    accumulator
                })
            })
            .collect();

        for handle in handles {
            // A worker panicking is a bug in the closure, not a condition to
            // model: let it propagate rather than inventing an error for it.
            folded.push(handle.join().expect("a worker panicked"));
        }
    });

    match failure.into_inner().ok().flatten() {
        Some(error) => Err(error),
        None => Ok(folded),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_every_item_is_visited_exactly_once() {
        let items: Vec<usize> = (0..1000).collect();
        let counts = each(&items, 8, Vec::new, |item, seen: &mut Vec<usize>| {
            seen.push(*item);
            Ok::<(), ()>(())
        })
        .unwrap();

        let mut all: Vec<usize> = counts.into_iter().flatten().collect();
        all.sort_unstable();
        assert_eq!(all, items);
    }

    #[test]
    fn test_the_first_error_is_returned_and_the_rest_abandoned() {
        let items: Vec<usize> = (0..1000).collect();
        let outcome = each(
            &items,
            8,
            || (),
            |item, ()| {
                if *item == 500 { Err("boom") } else { Ok(()) }
            },
        );
        assert_eq!(outcome.err(), Some("boom"));
    }

    #[test]
    fn test_one_thread_is_a_plain_loop() {
        let items: Vec<usize> = (0..10).collect();
        let folded = each(
            &items,
            1,
            || 0usize,
            |item, sum: &mut usize| {
                *sum += item;
                Ok::<(), ()>(())
            },
        )
        .unwrap();
        assert_eq!(folded, vec![45]);
    }

    #[test]
    fn test_an_empty_batch_does_no_work() {
        let items: Vec<usize> = Vec::new();
        let folded = each(&items, 8, || 0usize, |_, _| Ok::<(), ()>(())).unwrap();
        assert_eq!(folded.len(), 8);
        assert!(folded.iter().all(|count| *count == 0));
    }
}
