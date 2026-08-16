//! Shared process-env serialization for tests (#757).
//!
//! `std::env::set_var` mutates process-global state, so any two tests that
//! touch env must not overlap. `core_impl` has had a lock for this since
//! forever — but it is a private `fn` inside the private `core_impl` module,
//! so `serve_core`, a sibling, PHYSICALLY CANNOT REACH IT. Five files there
//! mutate env with no serialization at all, which is the substrate under the
//! flaky-test family (#809, #824, #839): passes alone, fails in the full
//! suite, never fails under isolation.
//!
//! This module exists at crate root so both trees can reach it.
//!
//! Re-entrancy: the guard is safe to nest. A plain `MutexGuard`-owning guard
//! would deadlock the moment someone wrote two guards in one scope, and
//! `std::sync::Mutex` is not reentrant — so depth is tracked per thread and
//! only the outermost acquisition locks. That is a trap avoided rather than a
//! feature: nothing nests today, and the point is that adding one later must
//! not hang the suite.

#![cfg(test)]

use std::cell::Cell;
use std::sync::{Mutex, MutexGuard, PoisonError};

static ENV_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    /// How many live guards this thread holds. Only the outermost locks.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Serializes process-env mutation across tests. Hold for as long as the env
/// is modified, and drop only after restoring it.
///
/// A poisoned mutex is recovered rather than propagated: one panicking test
/// holding the guard would otherwise cascade a `PoisonError` through every
/// later acquisition and fail dozens of unrelated tests.
pub(crate) fn env_test_lock() -> EnvLockGuard {
    let outermost = DEPTH.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);
        current == 0
    });
    let held = outermost.then(|| ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner));
    EnvLockGuard { _held: held }
}

/// RAII handle for [`env_test_lock`]. Releases the mutex when the outermost
/// guard on this thread drops.
pub(crate) struct EnvLockGuard {
    _held: Option<MutexGuard<'static, ()>>,
}

impl Drop for EnvLockGuard {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// One env guard for the whole crate (#757).
///
/// Four private `EnvGuard` copies existed in `serve_core` alone —
/// `process_engine.rs`, `websocket_routes.rs`, `god_mode_ui.rs` and `mod.rs` —
/// none of which serialized against each other or against `core_impl`. Same
/// shape as every other defect found on 2026-08-16: one rule implemented N
/// times, each locally reasonable, drifting where nobody looks.
///
/// Holds [`env_test_lock`] for its whole lifetime, so protection is inherited
/// rather than remembered. Restores the previous value on drop.
pub(crate) struct EnvVarGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
    _lock: EnvLockGuard,
}

impl EnvVarGuard {
    /// Set `key`, restoring the previous value on drop.
    pub(crate) fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let lock = env_test_lock();
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        Self {
            key,
            old,
            _lock: lock,
        }
    }

    /// Unset `key`, restoring the previous value on drop.
    pub(crate) fn remove(key: &'static str) -> Self {
        let lock = env_test_lock();
        let old = std::env::var_os(key);
        std::env::remove_var(key);
        Self {
            key,
            old,
            _lock: lock,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.old.take() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

#[cfg(test)]
mod test_env_tests {
    use super::*;

    #[test]
    fn nested_guards_do_not_deadlock_and_release_at_the_outermost() {
        let outer = env_test_lock();
        assert_eq!(DEPTH.with(Cell::get), 1);
        {
            let _inner = env_test_lock();
            assert_eq!(DEPTH.with(Cell::get), 2, "nesting must not re-lock");
        }
        assert_eq!(
            DEPTH.with(Cell::get),
            1,
            "inner drop must not release the lock"
        );
        drop(outer);
        assert_eq!(DEPTH.with(Cell::get), 0);
    }

    #[test]
    fn the_lock_is_actually_exclusive_across_threads() {
        // Two threads incrementing a shared counter under the guard: if the
        // mutex were not held, the sleep window would interleave them.
        static SEEN: Mutex<Vec<u8>> = Mutex::new(Vec::new());
        let worker = |tag: u8| {
            move || {
                let _guard = env_test_lock();
                SEEN.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(tag);
                std::thread::sleep(std::time::Duration::from_millis(20));
                SEEN.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(tag);
            }
        };
        let a = std::thread::spawn(worker(1));
        let b = std::thread::spawn(worker(2));
        a.join().expect("a");
        b.join().expect("b");
        let seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert!(
            seen == vec![1, 1, 2, 2] || seen == vec![2, 2, 1, 1],
            "threads interleaved, so the lock did not serialize them: {seen:?}"
        );
    }
}
