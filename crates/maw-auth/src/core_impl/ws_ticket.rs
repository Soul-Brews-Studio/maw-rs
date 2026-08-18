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
}
