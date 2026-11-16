//! Work spread over the machine's cores, for the core and its tests
//! alike.

use std::sync::atomic::{AtomicUsize, Ordering};

/// `work` of every item, in the items' order, spread over as many
/// threads as the machine has: each thread takes the next item not yet
/// taken, as items can differ widely in cost. Each item's answer is the
/// same as on one thread.
pub fn on_every_core<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if threads <= 1 || items.len() <= 1 {
        return items.iter().map(work).collect();
    }
    let next = AtomicUsize::new(0);
    let (work, next) = (&work, &next);
    let mut done: Vec<(usize, R)> = std::thread::scope(|sc| {
        let workers: Vec<_> = (0..threads.min(items.len()))
            .map(|_| {
                sc.spawn(move || {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        mine.push((i, work(item)));
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a worker panicked"))
            .collect()
    });
    done.sort_by_key(|d| d.0);
    done.into_iter().map(|d| d.1).collect()
}

/// `a` and `b` at once, `a` on a thread of its own: two pieces of work
/// that don't depend on each other. Each answer is the same as when they
/// run one after the other.
pub fn both<A: Send, B>(a: impl FnOnce() -> A + Send, b: impl FnOnce() -> B) -> (A, B) {
    std::thread::scope(|sc| {
        let a = sc.spawn(a);
        let b = b();
        (a.join().expect("a worker panicked"), b)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_come_back_in_the_items_order() {
        let items: Vec<u64> = (0..1000).collect();
        let out = on_every_core(&items, |&i| i * i);
        assert_eq!(out, items.iter().map(|i| i * i).collect::<Vec<_>>());
        assert!(on_every_core(&[] as &[u64], |&i| i).is_empty());
    }

    #[test]
    fn both_answers_come_back_in_their_places() {
        assert_eq!(both(|| 2 + 2, || "b"), (4, "b"));
    }
}
