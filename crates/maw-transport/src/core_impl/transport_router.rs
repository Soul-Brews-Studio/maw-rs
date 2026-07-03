// Portable transport classification and failover routing.
//
// This crate mirrors the pure send-order behavior in maw-js
// `src/core/transport/transport.ts` without binding to async runtime or IO.

/// Transport failure reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFailureReason {
    Timeout,
    Unreachable,
    Auth,
    RateLimit,
    Rejected,
    ParseError,
    Unknown,
}

impl TransportFailureReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Unreachable => "unreachable",
            Self::Auth => "auth",
            Self::RateLimit => "rate_limit",
            Self::Rejected => "rejected",
            Self::ParseError => "parse_error",
            Self::Unknown => "unknown",
        }
    }
}

/// Classified transport failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassifiedError {
    pub reason: TransportFailureReason,
    pub retryable: bool,
}

/// Classify common error strings into portable failure reasons.
#[must_use]
pub fn classify_error(err: Option<&str>) -> ClassifiedError {
    let Some(err) = err else {
        return ClassifiedError {
            reason: TransportFailureReason::Unknown,
            retryable: false,
        };
    };
    let msg = err.to_lowercase();
    if contains_any(&msg, &["timeout", "etimedout", "econnreset"]) {
        return ClassifiedError {
            reason: TransportFailureReason::Timeout,
            retryable: true,
        };
    }
    if contains_any(&msg, &["econnrefused", "unreachable", "enetunreach"]) {
        return ClassifiedError {
            reason: TransportFailureReason::Unreachable,
            retryable: true,
        };
    }
    if contains_any(&msg, &["401", "403", "auth", "unauthorized", "forbidden"]) {
        return ClassifiedError {
            reason: TransportFailureReason::Auth,
            retryable: false,
        };
    }
    if msg.contains("429") || msg.contains("too many") || rate_limit_like(&msg) {
        return ClassifiedError {
            reason: TransportFailureReason::RateLimit,
            retryable: true,
        };
    }
    if contains_any(&msg, &["400", "reject", "denied"]) {
        return ClassifiedError {
            reason: TransportFailureReason::Rejected,
            retryable: false,
        };
    }
    if contains_any(&msg, &["parse", "json", "syntax"]) {
        return ClassifiedError {
            reason: TransportFailureReason::ParseError,
            retryable: false,
        };
    }
    ClassifiedError {
        reason: TransportFailureReason::Unknown,
        retryable: false,
    }
}

/// Result of a routed send attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResult {
    pub ok: bool,
    pub via: String,
    pub reason: Option<TransportFailureReason>,
    pub retryable: bool,
}

impl TransportResult {
    #[must_use]
    pub fn success(via: impl Into<String>) -> Self {
        Self {
            ok: true,
            via: via.into(),
            reason: None,
            retryable: false,
        }
    }

    #[must_use]
    pub fn failure(
        via: impl Into<String>,
        reason: TransportFailureReason,
        retryable: bool,
    ) -> Self {
        Self {
            ok: false,
            via: via.into(),
            reason: Some(reason),
            retryable,
        }
    }
}

/// Destination metadata for transport selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransportTarget {
    pub oracle: String,
    pub host: Option<String>,
    pub tmux_target: Option<String>,
}

/// Window shape used by local tmux target resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxTransportWindow {
    pub index: u32,
    pub name: String,
    pub active: bool,
}

/// Session shape used by local tmux target resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxTransportSession {
    pub name: String,
    pub windows: Vec<TmuxTransportWindow>,
}

/// HTTP federation transport configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpTransportConfig {
    pub peers: Vec<String>,
    pub self_host: String,
}

/// Result of an HTTP feed publish attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpPostResult {
    pub ok: bool,
    pub status: u16,
}

/// Captured warning for failed best-effort HTTP feed publishing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpFeedWarning {
    pub peer: String,
    pub reason: String,
}

/// Locally measured federation status: local URL plus one-way reachability to peers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationStatus {
    pub local_url: String,
    pub peers: Vec<FederationPeerStatus>,
}

/// One peer row from the local federation status baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationPeerStatus {
    pub url: String,
    pub node: Option<String>,
    pub reachable: bool,
    pub latency: Option<u64>,
    pub agents: Vec<String>,
    pub clock_warning: bool,
}

/// One peer row reported by a remote peer's federation status endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationPeerView {
    pub url: Option<String>,
    pub node: Option<String>,
    pub reachable: Option<bool>,
}

/// Remote `/api/federation/status` result supplied by the IO adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerFederationStatusResult {
    Ok(PeerFederationStatus),
    MissingPeers,
    HttpStatus(u16),
    FetchError(String),
}

/// Decoded remote federation status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerFederationStatus {
    pub peers: Vec<FederationPeerView>,
}

/// Symmetric pair-health classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairHealth {
    Healthy,
    HalfUp,
    Down,
    Unknown,
}

impl PairHealth {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::HalfUp => "half-up",
            Self::Healthy => "healthy",
            Self::Unknown => "unknown",
        }
    }
}

/// Pair-health row for a single local peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairStatus {
    pub url: String,
    pub node: Option<String>,
    pub pair: PairHealth,
    pub forward: bool,
    pub reverse: Option<bool>,
    pub reason: Option<String>,
    pub latency: Option<u64>,
    pub agents: Vec<String>,
    pub clock_warning: bool,
}

/// Complete symmetric federation status summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymmetricFederationStatus {
    pub local_url: String,
    pub local_node: String,
    pub pairs: Vec<PairStatus>,
    pub healthy_pairs: usize,
    pub total_pairs: usize,
}

/// Side-effect seam for HTTP federation transport.
pub trait HttpTransportIo {
    /// List local sessions before aggregating remote peer sessions.
    ///
    /// # Errors
    ///
    /// Returns an implementation-specific error string when local listing fails.
    fn list_local_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String>;

    /// Return local + remote sessions, preserving any source metadata.
    ///
    /// # Errors
    ///
    /// Returns an implementation-specific error string when aggregation fails.
    fn get_all_sessions(
        &mut self,
        local_sessions: &[TmuxTransportSession],
    ) -> Result<Vec<TransportSession>, String>;

    /// Resolve a window in a single remote session.
    fn find_target_window(&mut self, sessions: &[TransportSession], query: &str) -> Option<String>;

    /// Send keys to a remote peer/source.
    ///
    /// # Errors
    ///
    /// Returns an implementation-specific error string when peer send fails.
    fn send_peer_keys(&mut self, source: &str, target: &str, message: &str)
        -> Result<bool, String>;

    /// POST a feed event to a peer.
    ///
    /// # Errors
    ///
    /// Returns an implementation-specific error string when publishing fails.
    fn post_peer_feed(
        &mut self,
        url: &str,
        method: &str,
        body: &str,
        timeout_ms: u64,
    ) -> Result<HttpPostResult, String>;

    /// Return configured timeout for a named transport.
    fn timeout_for(&self, transport: &str) -> u64;
}

/// Session shape used by HTTP federation, including source peer metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportSession {
    pub name: String,
    pub source: Option<String>,
    pub windows: Vec<TmuxTransportWindow>,
}

impl From<TmuxTransportSession> for TransportSession {
    fn from(value: TmuxTransportSession) -> Self {
        Self {
            name: value.name,
            source: None,
            windows: value.windows,
        }
    }
}

/// Minimal portable transport trait.
pub trait Transport {
    fn name(&self) -> &str;
    fn connected(&self) -> bool;
    fn can_reach(&self, target: &TransportTarget) -> bool;
    /// Send a message through this transport.
    ///
    /// # Errors
    ///
    /// Returns an error string when the transport attempted delivery but failed.
    /// The router classifies that error to decide whether to fail over.
    fn send(&mut self, target: &TransportTarget, message: &str, from: &str)
        -> Result<bool, String>;
}

/// Ordered transport router. First successful reachable transport wins.
#[derive(Default)]
pub struct TransportRouter<T> {
    transports: Vec<T>,
}

impl<T> TransportRouter<T>
where
    T: Transport,
{
    #[must_use]
    pub const fn new() -> Self {
        Self {
            transports: Vec::new(),
        }
    }

    pub fn register(&mut self, transport: T) {
        self.transports.push(transport);
    }

    pub fn send(&mut self, target: &TransportTarget, message: &str, from: &str) -> TransportResult {
        for transport in &mut self.transports {
            if !transport.connected() || !transport.can_reach(target) {
                continue;
            }

            match transport.send(target, message, from) {
                Ok(true) => return TransportResult::success(transport.name()),
                Ok(false) => {}
                Err(err) => {
                    let classified = classify_error(Some(&err));
                    if !classified.retryable {
                        return TransportResult::failure(
                            transport.name(),
                            classified.reason,
                            classified.retryable,
                        );
                    }
                }
            }
        }
        TransportResult::failure("none", TransportFailureReason::Unreachable, false)
    }
}

/// Side-effect seam for the local tmux transport.
pub trait TmuxTransportIo {
    /// Send a message to a concrete tmux target.
    ///
    /// # Errors
    ///
    /// Returns an implementation-specific error string when tmux rejects delivery.
    fn send_to_tmux(&mut self, target: &str, message: &str) -> Result<(), String>;

    /// List local tmux sessions for oracle-name resolution.
    ///
    /// # Errors
    ///
    /// Returns an implementation-specific error string when session listing fails.
    fn list_tmux_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String>;

    /// Resolve an oracle query to a tmux target from already-listed sessions.
    fn find_tmux_window(
        &mut self,
        sessions: &[TmuxTransportSession],
        query: &str,
    ) -> Option<String>;
}

/// Portable local fast-path tmux transport.
pub struct TmuxLocalTransport<Io> {
    io: Io,
    connected: bool,
    message_handlers: usize,
    presence_handlers: usize,
    feed_handlers: usize,
}

impl<Io> TmuxLocalTransport<Io> {
    #[must_use]
    pub const fn new(io: Io) -> Self {
        Self {
            io,
            connected: false,
            message_handlers: 0,
            presence_handlers: 0,
            feed_handlers: 0,
        }
    }

    #[must_use]
    pub const fn connected(&self) -> bool {
        self.connected
    }

    pub const fn connect(&mut self) {
        self.connected = true;
    }

    pub const fn disconnect(&mut self) {
        self.connected = false;
    }

    pub const fn on_message(&mut self) {
        self.message_handlers += 1;
    }

    pub const fn on_presence(&mut self) {
        self.presence_handlers += 1;
    }

    pub const fn on_feed(&mut self) {
        self.feed_handlers += 1;
    }

    #[must_use]
    pub const fn handler_counts(&self) -> (usize, usize, usize) {
        (
            self.message_handlers,
            self.presence_handlers,
            self.feed_handlers,
        )
    }

    pub const fn publish_presence(&self) {}

    pub const fn publish_feed(&self) {}
}

impl<Io> TmuxLocalTransport<Io>
where
    Io: TmuxTransportIo,
{
    #[must_use]
    pub fn name(&self) -> &'static str {
        "tmux"
    }

    #[must_use]
    pub fn can_reach(&self, target: &TransportTarget) -> bool {
        is_local_host(target.host.as_deref())
    }

    /// Send using explicit `tmux_target` or by scanning sessions and resolving the oracle name.
    pub fn send(&mut self, target: &TransportTarget, message: &str) -> bool {
        if !self.can_reach(target) {
            return false;
        }
        let tmux_target = if let Some(tmux_target) = &target.tmux_target {
            tmux_target.clone()
        } else {
            let Ok(sessions) = self.io.list_tmux_sessions() else {
                return false;
            };
            let Some(resolved) = self.io.find_tmux_window(&sessions, &target.oracle) else {
                return false;
            };
            resolved
        };
        self.io.send_to_tmux(&tmux_target, message).is_ok()
    }

    #[must_use]
    pub const fn io(&self) -> &Io {
        &self.io
    }
}
