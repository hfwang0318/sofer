//! Minimal parallel map used to process several images at once.
//!
//! Deliberately small: no dependency, order preserved, one bad item cannot take
//! down the rest. `jobs = 1` runs inline (no thread at all), so the default path
//! stays exactly as it was before concurrency existed.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Apply `f` to every item, running at most `jobs` of them at a time.
///
/// Results come back in **input order**, regardless of completion order.
pub fn map<T, R, F>(items: &[T], jobs: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
{
    let n = items.len();
    if n == 0 {
        return Vec::new();
    }
    let jobs = jobs.clamp(1, n);
    if jobs == 1 {
        return (0..n).map(|i| f(i, &items[i])).collect();
    }

    // Each worker pulls the next index and writes its result into its own slot,
    // so the output vector is order-preserving without sorting or channels.
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<R>>> = Mutex::new((0..n).map(|_| None).collect());

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let value = f(i, &items[i]);
                let mut guard = slots.lock().unwrap();
                guard[i] = Some(value);
            });
        }
    });

    let mut out = slots.into_inner().unwrap();
    out.drain(..)
        .map(|v| v.expect("every index was visited exactly once"))
        .collect()
}

/// Counting semaphore with an RAII permit.
///
/// Used to bound the number of *engine calls* in flight. It has to be a single
/// shared budget rather than "jobs per level", otherwise `--jobs 3` on three
/// images that each tile into three rows would put nine requests on a server that
/// only has four slots.
pub struct Semaphore {
    free: Mutex<usize>,
    cv: std::sync::Condvar,
}

pub struct Permit<'a>(&'a Semaphore);

impl Semaphore {
    pub fn new(permits: usize) -> Self {
        Self {
            free: Mutex::new(permits.max(1)),
            cv: std::sync::Condvar::new(),
        }
    }

    pub fn acquire(&self) -> Permit<'_> {
        let mut free = self.free.lock().unwrap();
        while *free == 0 {
            free = self.cv.wait(free).unwrap();
        }
        *free -= 1;
        Permit(self)
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap() += 1;
        self.0.cv.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    #[test]
    fn preserves_order_even_when_finishing_out_of_order() {
        // earlier items sleep longer, so completion order is reversed
        let items: Vec<usize> = (0..8).collect();
        let out = map(&items, 4, |i, &v| {
            std::thread::sleep(Duration::from_millis((8 - i) as u64 * 2));
            v * 10
        });
        assert_eq!(out, vec![0, 10, 20, 30, 40, 50, 60, 70]);
    }

    #[test]
    fn runs_every_item_exactly_once() {
        let items: Vec<usize> = (0..50).collect();
        let seen = AtomicUsize::new(0);
        let out = map(&items, 8, |_, &v| {
            seen.fetch_add(1, Ordering::Relaxed);
            v
        });
        assert_eq!(seen.load(Ordering::Relaxed), 50);
        assert_eq!(out.len(), 50);
        assert!(out.iter().enumerate().all(|(i, &v)| i == v));
    }

    #[test]
    fn semaphore_bounds_concurrency() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::Arc;

        let sem = Arc::new(Semaphore::new(2));
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let (sem, live, peak) = (sem.clone(), live.clone(), peak.clone());
                scope.spawn(move || {
                    let _p = sem.acquire();
                    let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                    live.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert!(
            peak.load(Ordering::SeqCst) <= 2,
            "peak {}",
            peak.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn one_job_runs_inline() {
        let items = vec![1, 2, 3];
        let out = map(&items, 1, |_, &v| v + 1);
        assert_eq!(out, vec![2, 3, 4]);
    }

    #[test]
    fn more_jobs_than_items_is_fine() {
        let items = vec![1, 2];
        assert_eq!(map(&items, 16, |_, &v| v * 2), vec![2, 4]);
        assert!(map(&[] as &[i32], 4, |_, &v| v).is_empty());
    }
}
