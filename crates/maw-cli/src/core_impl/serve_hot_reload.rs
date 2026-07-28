// A value re-read from disk only once its cached copy has gone stale.
//
// serve holds peer pubkeys in memory so it is not stat-ing peers.json on every
// request, but a peer added seconds ago has to work without a restart. HotReload
// is the small compromise: serve the cached value, reload past a short TTL.

/// Panicking loader for a [`HotReload::frozen`] value — it is never called
/// because a frozen cache reports itself fresh forever.
#[cfg(test)]
fn hot_reload_frozen_loader<T>() -> T {
    unreachable!("frozen HotReload never reloads")
}

/// A value loaded from disk once, then refreshed at most every `ttl_secs` so
/// config / peer-store edits take effect **without restarting serve** — this is
/// the fix for the freeze where `peer_pubkeys` was baked at startup, so a peer
/// added after boot got `refuse-missing-peer-key` until a restart (#16).
/// `frozen` builds a fixed value that never reloads (tests inject keys through
/// it). Reusable by the federation status snapshot (#7), same freeze pattern.
struct HotReload<T> {
    cache: std::sync::RwLock<Option<(T, u64)>>,
    loader: fn() -> T,
    ttl_secs: u64,
    hot: bool,
}

impl<T: Clone> HotReload<T> {
    /// Live cache: (re)loads from `loader` when the value is missing or older
    /// than `ttl_secs`.
    fn live(loader: fn() -> T, ttl_secs: u64) -> Self {
        Self {
            cache: std::sync::RwLock::new(None),
            loader,
            ttl_secs,
            hot: true,
        }
    }

    /// Fixed value that never reloads — for tests that inject state directly.
    #[cfg(test)]
    fn frozen(value: T) -> Self {
        Self {
            cache: std::sync::RwLock::new(Some((value, 0))),
            loader: hot_reload_frozen_loader::<T>,
            ttl_secs: 0,
            hot: false,
        }
    }

    /// Current value, reloading first if hot and stale. Returns a clone so the
    /// caller never holds the lock across its own work. A rare double reload
    /// under concurrent staleness is harmless — the loads are idempotent.
    fn get(&self, now_secs: u64) -> T {
        if self.hot {
            let stale = self
                .cache
                .read()
                .ok()
                .and_then(|guard| {
                    guard
                        .as_ref()
                        .map(|(_, at)| now_secs.saturating_sub(*at) >= self.ttl_secs)
                })
                .unwrap_or(true);
            if stale {
                let value = (self.loader)();
                if let Ok(mut guard) = self.cache.write() {
                    *guard = Some((value.clone(), now_secs));
                }
                return value;
            }
        }
        self.cache
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().map(|(value, _)| value.clone()))
            .unwrap_or_else(|| (self.loader)())
    }
}

#[cfg(test)]
mod hot_reload_tests {
    use super::HotReload;
    use std::sync::atomic::{AtomicU64, Ordering};

    static HOT_RELOAD_TEST_CALLS: AtomicU64 = AtomicU64::new(0);

    fn counting_loader() -> u64 {
        HOT_RELOAD_TEST_CALLS.fetch_add(1, Ordering::SeqCst) + 1
    }

    #[test]
    fn live_reloads_after_ttl_but_caches_within_the_window() {
        HOT_RELOAD_TEST_CALLS.store(0, Ordering::SeqCst);
        let cache = HotReload::live(counting_loader, 3);
        assert_eq!(cache.get(100), 1); // first access loads
        assert_eq!(cache.get(101), 1); // 1s later, within TTL → cached
        assert_eq!(cache.get(102), 1); // 2s < 3s → still cached
        assert_eq!(cache.get(103), 2); // 3s >= TTL → reloads (a peer added now is seen)
        assert_eq!(cache.get(103), 2); // fresh again
        assert_eq!(HOT_RELOAD_TEST_CALLS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn frozen_never_reloads_even_far_in_the_future() {
        let cache = HotReload::frozen(vec!["a".to_owned()]);
        assert_eq!(cache.get(0), vec!["a".to_owned()]);
        assert_eq!(cache.get(u64::MAX), vec!["a".to_owned()]);
    }
}
