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

