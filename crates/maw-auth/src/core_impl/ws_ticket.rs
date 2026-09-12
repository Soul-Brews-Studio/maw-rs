use rand_core::{CryptoRng, RngCore};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

const PREFIX: &str = "mwt1_";
const DOMAIN: &[u8] = b"maw.ws-ticket.v1\0";
const TTL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WsTicketPath {
    Ws,
    Pty,
    Tmux,
}

impl TryFrom<&str> for WsTicketPath {
    type Error = &'static str;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "/ws" => Ok(Self::Ws),
            "/ws/pty" => Ok(Self::Pty),
            "/ws/tmux" => Ok(Self::Tmux),
            _ => Err("invalid WebSocket path"),
        }
    }
}

pub struct WsTicket(String);

impl WsTicket {
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for WsTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WsTicket([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WsTicketIssueError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WsTicketConsume {
    Accepted,
    Rejected,
    Unavailable,
}

struct Entry {
    digest: [u8; 32],
    expires: Instant,
    origin: String,
    path: WsTicketPath,
}

/// Callers inject a cryptographically secure RNG and a fresh monotonic clock.
/// The clock runs once under the mutex and must not block, panic, or re-enter.
#[derive(Default)]
pub struct WsTicketStore(Mutex<(Vec<Entry>, Option<Instant>)>);

impl WsTicketStore {
    /// # Errors
    /// Returns a fail-closed issuance error.
    pub fn issue<R: CryptoRng + RngCore, C: FnOnce() -> Instant>(
        &self,
        origin: &str,
        path: WsTicketPath,
        rng: &mut R,
        clock: C,
    ) -> Result<WsTicket, WsTicketIssueError> {
        if origin.is_empty() || origin.len() > 2048 {
            return Err(WsTicketIssueError);
        }
        let mut bytes = [0; 32];
        rng.try_fill_bytes(&mut bytes)
            .map_err(|_| WsTicketIssueError)?;
        let ticket = WsTicket(format!("{PREFIX}{}", hex_lower(&bytes)));
        let digest = ticket_digest(ticket.expose_secret());
        let mut state = self.0.lock().map_err(|_| WsTicketIssueError)?;
        let now = clock();
        if reversed(&mut state.1, now) {
            return Err(WsTicketIssueError);
        }
        let expires = now.checked_add(TTL).ok_or(WsTicketIssueError)?;
        state.0.retain(|entry| now < entry.expires);
        if state.0.len() >= 256 {
            return Err(WsTicketIssueError);
        }
        if state.0.iter().any(|e| bool::from(e.digest.ct_eq(&digest))) {
            return Err(WsTicketIssueError);
        }
        state.0.push(Entry {
            digest,
            expires,
            origin: origin.into(),
            path,
        });
        Ok(ticket)
    }

    #[must_use]
    pub fn consume<C: FnOnce() -> Instant>(
        &self,
        token: &str,
        origin: Option<&str>,
        path: WsTicketPath,
        clock: C,
    ) -> WsTicketConsume {
        if !valid_ticket(token) || origin.is_none_or(|o| o.is_empty() || o.len() > 2048) {
            return WsTicketConsume::Rejected;
        }
        let digest = ticket_digest(token);
        let Ok(mut state) = self.0.lock() else {
            return WsTicketConsume::Unavailable;
        };
        let now = clock();
        if reversed(&mut state.1, now) {
            return WsTicketConsume::Rejected;
        }
        state.0.retain(|entry| now < entry.expires);
        let found = state.0.iter().position(|entry| {
            bool::from(entry.digest.ct_eq(&digest))
                && origin == Some(&entry.origin)
                && path == entry.path
        });
        let Some(index) = found else {
            return WsTicketConsume::Rejected;
        };
        state.0.remove(index);
        WsTicketConsume::Accepted
    }
}

fn reversed(last: &mut Option<Instant>, now: Instant) -> bool {
    let result = last.is_some_and(|value| now < value);
    if !result {
        *last = Some(now);
    }
    result
}

fn ticket_digest(token: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(token);
    hash.finalize().into()
}

fn valid_ticket(token: &str) -> bool {
    token.len() == PREFIX.len() + 64
        && token.starts_with(PREFIX)
        && token[PREFIX.len()..]
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod ws_ticket_tests {
    use super::*;

    struct TestRng;

    impl RngCore for TestRng {
        fn next_u32(&mut self) -> u32 {
            0
        }
        fn next_u64(&mut self) -> u64 {
            0
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            dest.fill(0xab);
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }
    impl CryptoRng for TestRng {}
    struct SequenceRng(u64);
    impl RngCore for SequenceRng {
        fn next_u32(&mut self) -> u32 {
            0
        }
        fn next_u64(&mut self) -> u64 {
            0
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            dest.fill(0xa5);
            dest[..8].copy_from_slice(&self.0.to_le_bytes());
            self.0 += 1;
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }
    impl CryptoRng for SequenceRng {}
    struct FailRng;
    impl RngCore for FailRng {
        fn next_u32(&mut self) -> u32 {
            0
        }
        fn next_u64(&mut self) -> u64 {
            0
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            dest[..4].fill(0xff);
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Err(rand_core::Error::from(
                std::num::NonZeroU32::new(rand_core::Error::CUSTOM_START).unwrap(),
            ))
        }
    }
    impl CryptoRng for FailRng {}
    fn issue_at(
        store: &WsTicketStore,
        rng: &mut SequenceRng,
        now: Instant,
    ) -> Result<WsTicket, WsTicketIssueError> {
        store.issue("origin", WsTicketPath::Ws, rng, || now)
    }
    fn consume_at(store: &WsTicketStore, token: &str, now: Instant) -> WsTicketConsume {
        store.consume(token, Some("origin"), WsTicketPath::Ws, || now)
    }
    fn no_clock() -> Instant {
        panic!("clock called")
    }
    fn instant_without_ttl_room(base: Instant) -> Instant {
        (0..64).rev().fold(base, |edge, bit| {
            edge.checked_add(Duration::from_secs(1_u64 << bit))
                .unwrap_or(edge)
        })
    }

    #[test]
    fn exact_format_paths_scope_and_replay() {
        assert_eq!(WsTicketPath::try_from("/ws"), Ok(WsTicketPath::Ws));
        assert_eq!(WsTicketPath::try_from("/ws/pty"), Ok(WsTicketPath::Pty));
        assert_eq!(WsTicketPath::try_from("/ws/tmux"), Ok(WsTicketPath::Tmux));
        for bad in ["/WS", "/ws?x", "/ws/", "x/ws", "/ws/pty/x"] {
            assert!(WsTicketPath::try_from(bad).is_err());
        }
        let now = Instant::now();
        let store = WsTicketStore::default();
        let ticket = store
            .issue("https://a", WsTicketPath::Ws, &mut TestRng, || now)
            .unwrap();
        let token = ticket.expose_secret();
        assert_eq!(token, format!("mwt1_{}", "ab".repeat(32)));
        assert_eq!(format!("{ticket:?}"), "WsTicket([REDACTED])");
        let reject = |candidate, origin, path| {
            assert_eq!(
                store.consume(candidate, origin, path, || now),
                WsTicketConsume::Rejected
            );
        };
        reject("bad", Some("https://a"), WsTicketPath::Ws);
        for (origin, path) in [
            (None, WsTicketPath::Ws),
            (Some("https://b"), WsTicketPath::Ws),
            (Some("https://a"), WsTicketPath::Pty),
        ] {
            reject(token, origin, path);
        }
        assert_eq!(
            store.consume(token, Some("https://a"), WsTicketPath::Ws, || now),
            WsTicketConsume::Accepted
        );
        assert_eq!(
            store.consume(token, Some("https://a"), WsTicketPath::Ws, || now),
            WsTicketConsume::Rejected
        );
        let ttl_store = WsTicketStore::default();
        let ttl_ticket = ttl_store
            .issue("x", WsTicketPath::Ws, &mut TestRng, || {
                assert!(ttl_store.0.try_lock().is_err());
                now
            })
            .unwrap();
        assert_eq!(
            ttl_store.consume(
                ttl_ticket.expose_secret(),
                Some("x"),
                WsTicketPath::Ws,
                || {
                    assert!(ttl_store.0.try_lock().is_err());
                    now + Duration::from_secs(30)
                },
            ),
            WsTicketConsume::Rejected
        );
    }

    #[test]
    fn origin_and_token_boundaries_do_not_burn() {
        let now = Instant::now();
        let store = WsTicketStore::default();
        let mut rng = SequenceRng(0);
        let too_long = "x".repeat(2049);
        for invalid in ["", too_long.as_str()] {
            assert!(store
                .issue(invalid, WsTicketPath::Ws, &mut rng, no_clock)
                .is_err());
        }
        let origin = "x".repeat(2048);
        let ticket = store
            .issue(&origin, WsTicketPath::Ws, &mut rng, || now)
            .unwrap();
        let token = ticket.expose_secret();
        let uppercase = format!("{PREFIX}{}", token[PREFIX.len()..].to_ascii_uppercase());
        for (candidate, bound_origin) in [
            (token.to_owned() + "0", Some(origin.as_str())),
            (uppercase, Some(origin.as_str())),
            (token.to_owned(), Some("")),
            (token.to_owned(), Some(too_long.as_str())),
        ] {
            assert_eq!(
                store.consume(&candidate, bound_origin, WsTicketPath::Ws, || now),
                WsTicketConsume::Rejected
            );
        }
        assert_eq!(
            store.consume(token, Some(&origin), WsTicketPath::Ws, || now),
            WsTicketConsume::Accepted
        );
    }
    #[test]
    fn capacity_rejects_without_live_eviction_and_expiry_reclaims() {
        let now = Instant::now();
        let store = WsTicketStore::default();
        let mut rng = SequenceRng(0);
        let tickets: Vec<_> = (0..256)
            .map(|_| issue_at(&store, &mut rng, now).unwrap())
            .collect();
        assert!(issue_at(&store, &mut rng, now).is_err());
        assert!(tickets
            .iter()
            .all(|ticket| consume_at(&store, ticket.expose_secret(), now)
                == WsTicketConsume::Accepted));
        let expired: Vec<_> = (0..256)
            .map(|_| issue_at(&store, &mut rng, now).unwrap())
            .collect();
        let reclaimed = issue_at(&store, &mut rng, now + Duration::from_secs(30)).unwrap();
        let expiry = now + Duration::from_secs(30);
        assert_eq!(
            consume_at(&store, expired[0].expose_secret(), expiry),
            WsTicketConsume::Rejected
        );
        assert_eq!(
            consume_at(&store, reclaimed.expose_secret(), expiry),
            WsTicketConsume::Accepted
        );
    }
    #[test]
    fn entropy_failure_and_collision_leave_state_unchanged() {
        let now = Instant::now();
        let store = WsTicketStore::default();
        let original = issue_at(&store, &mut SequenceRng(1), now).unwrap();
        assert!(store
            .issue("attacker", WsTicketPath::Pty, &mut FailRng, no_clock)
            .is_err());
        let failed_token = format!("{PREFIX}{}{}", "ff".repeat(4), "00".repeat(28));
        assert_eq!(
            store.consume(&failed_token, Some("attacker"), WsTicketPath::Pty, || now),
            WsTicketConsume::Rejected
        );
        assert_eq!(
            consume_at(&store, original.expose_secret(), now),
            WsTicketConsume::Accepted
        );
        let collision_store = WsTicketStore::default();
        let original = issue_at(&collision_store, &mut SequenceRng(7), now).unwrap();
        for _ in 0..3 {
            assert!(collision_store
                .issue("attacker", WsTicketPath::Pty, &mut SequenceRng(7), || now)
                .is_err());
        }
        assert_eq!(
            collision_store.consume(
                original.expose_secret(),
                Some("attacker"),
                WsTicketPath::Pty,
                || now
            ),
            WsTicketConsume::Rejected
        );
        assert_eq!(
            consume_at(&collision_store, original.expose_secret(), now),
            WsTicketConsume::Accepted
        );
    }
    #[test]
    fn clock_failures_reject_without_mutation_and_recover() {
        let now = Instant::now();
        let store = WsTicketStore::default();
        let ticket = store
            .issue("origin", WsTicketPath::Ws, &mut SequenceRng(0), || {
                assert!(store.0.try_lock().is_err());
                now + Duration::from_secs(1)
            })
            .unwrap();
        assert_eq!(
            consume_at(&store, ticket.expose_secret(), now),
            WsTicketConsume::Rejected
        );
        let recovery_clock = || {
            assert!(store.0.try_lock().is_err());
            now + Duration::from_secs(1)
        };
        let recovered = store.consume(
            ticket.expose_secret(),
            Some("origin"),
            WsTicketPath::Ws,
            recovery_clock,
        );
        assert_eq!(recovered, WsTicketConsume::Accepted);
        let edge = instant_without_ttl_room(now);
        assert!(edge.checked_add(Duration::from_secs(30)).is_none());
        let overflow_store = WsTicketStore::default();
        assert!(issue_at(&overflow_store, &mut SequenceRng(9), edge).is_err());
        assert_eq!(
            consume_at(
                &overflow_store,
                "mwt1_0900000000000000a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
                edge,
            ),
            WsTicketConsume::Rejected
        );
    }

    #[test]
    fn concurrent_consume_has_exactly_one_winner() {
        let now = Instant::now();
        let store = std::sync::Arc::new(WsTicketStore::default());
        let token = issue_at(&store, &mut SequenceRng(0), now)
            .unwrap()
            .expose_secret()
            .to_owned();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (store, token, barrier) = (store.clone(), token.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    consume_at(&store, &token, now)
                })
            })
            .collect();
        let outcomes: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == WsTicketConsume::Accepted)
                .count(),
            1
        );
        assert!(!outcomes.contains(&WsTicketConsume::Unavailable));
    }

    #[test]
    fn poisoned_store_fails_closed_without_calling_clock() {
        let store = std::sync::Arc::new(WsTicketStore::default());
        let poison = store.clone();
        assert!(std::thread::spawn(move || {
            let _guard = poison.0.lock().unwrap();
            panic!("poison store");
        })
        .join()
        .is_err());
        assert!(store
            .issue("origin", WsTicketPath::Ws, &mut SequenceRng(0), no_clock)
            .is_err());
        assert_eq!(
            store.consume(
                "mwt1_0000000000000000a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
                Some("origin"),
                WsTicketPath::Ws,
                no_clock
            ),
            WsTicketConsume::Unavailable
        );
    }
}
