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

