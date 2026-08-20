use std::time::Duration;

use maw_auth::sign_headers_v3_at;
use serde::Deserialize;

const SEND_PATH: &str = "/api/send";
const WAKE_PATH: &str = "/api/wake";
const POST_METHOD: &str = "POST";

/// Outbound `/api/send` request, signed with maw v3 from-signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSendRequest {
    pub peer_url: String,
    pub target: String,
    pub text: String,
    pub inbox: Option<bool>,
    pub from: String,
    pub federation_token: String,
    pub peer_key: String,
    pub timestamp: i64,
}

/// Outbound `/api/wake` request, signed with maw v3 from-signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerWakeRequest {
    pub peer_url: String,
    pub target: String,
    pub task: Option<String>,
    pub from: String,
    pub federation_token: String,
    pub peer_key: String,
    pub timestamp: i64,
}

/// Parsed `/api/send` response outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSendResponse {
    pub ok: bool,
    pub status: u16,
    pub state: Option<String>,
    pub target: Option<String>,
    pub last_line: Option<String>,
    pub error: Option<String>,
    pub decision: Option<String>,
    /// #709: set when the receiving serve delivered into a pane that does
    /// not look agent-shaped (a plain shell, most often) -- the send still
    /// succeeded, this names why it is probably wrong.
    pub warning: Option<String>,
}

/// Parsed `/api/wake` response outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerWakeResponse {
    pub ok: bool,
    pub status: u16,
    pub target: Option<String>,
    pub error: Option<String>,
}

/// `/api/probe` outcome, with the server's own reason for a 401/403 (#685) --
/// `verify_protected_request_outcome` on the receiving serve already computes
/// a named decision (`refuse-unsigned`, a pubkey-mismatch kind, ...) and puts
/// it in the response body; this is that value, not re-derived.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerProbeAuthResult {
    pub ok: Option<bool>,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PeerProbeWireResponse {
    #[serde(default)]
    decision: Option<String>,
}

fn peer_send_error_message(status: u16, parsed: &PeerSendResponse) -> String {
    let mut msg = format!(
        "remote /api/send returned HTTP {status}: {}",
        parsed.error.as_deref().unwrap_or("request failed")
    );
    if let Some(decision) = parsed
        .decision
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        msg.push_str(" [decision=");
        msg.push_str(decision);
        msg.push(']');
        if let Some(hint) = decision_hint(decision) {
            msg.push_str(" — ");
            msg.push_str(hint);
        }
    }
    msg
}

/// Human hint for a federation `decision` refusal code, so a bare 401 stops
/// masquerading as a permissions problem when it is really an unpinned key or
/// a missing signature.
///
/// A hint names the field that decided the refusal. It must not name a remedy
/// that cannot work: the `refuse-missing-peer-key` arm used to end with "the
/// receiver's serve loaded pubkeys at startup and needs a restart after the
/// peer was added", which stopped being true once serve began hot-reloading
/// `peer_pubkeys` on a short TTL (`maw-cli`'s `serve_hot_reload.rs`). Because
/// the advice shipped inside the error it read as authoritative, and a real
/// two-host bring-up restarted the receiving daemon three times on the
/// strength of it — including once after the peer already showed
/// `authOk: true` — before anyone doubted the string (#822).
fn decision_hint(decision: &str) -> Option<&'static str> {
    Some(match decision {
        // #819 is the case that actually produces this on a peer that looks
        // registered: an entry whose `identity.oracle` is empty is dropped by
        // the receiver's `normalize_from_identity`, so the lookup table holds
        // no pubkey for that sender at all — not a stale one, none.
        "refuse-missing-peer-key" => "the receiver has no pinned pubkey for this sender identity — run `maw peers info <alias>` on the receiver and look at identity.oracle: an empty oracle half is dropped rather than stored (#819), so re-add the peer with `maw peers add <alias> <url> --oracle <name>`. The receiver reloads peers within seconds, so restarting it does not help",
        "refuse-mismatch" => "signature mismatch: the sender's ~/.maw/peer-key differs from the receiver's pinned pubkey (key rotated, or MAW_HOME/MAW_PEER_KEY set differently in a worktree?)",
        "refuse-unsigned" => "the request carried no X-Maw-Signature",
        "refuse-ambiguous-peer-key" => "the receiver has multiple pubkeys pinned for this sender",
        "refuse-skew" => "timestamp skew too large — check both machines' clocks",
        "cache-no-sig" => "no signature was cached for verification",
        _ => return None,
    })
}

/// Name the layer that actually failed, and carry the real reason up with it.
///
/// `reqwest::Error`'s own `Display` stops at "error sending request for url
/// (...)" — the reason (connection refused, dns failure, a header we built
/// wrong) only lives in the `source()` chain. Calling every one of them a
/// "network error" then points the reader at the wrong machine: a request
/// rejected while being built never reached the network at all, so the peer,
/// its port and its firewall are all innocent. Both halves of that cost a
/// federation debug session on 2026-07-28, where the only way to tell
/// client-side failure from an unreachable peer was to re-issue the request
/// by hand with curl.
fn request_error_message(action: &str, url: &str, error: &reqwest::Error) -> String {
    let layer = if error.is_builder() {
        "invalid request (never sent)"
    } else if error.is_connect() {
        "connect failed"
    } else if error.is_timeout() {
        "timed out"
    } else if error.is_redirect() {
        "too many redirects"
    } else if error.is_decode() {
        "malformed response"
    } else {
        "network error"
    };
    format!(
        "{layer} {action} {url}: {}",
        error_cause_chain(error, url)
    )
}

fn peer_send_transport_code(message: &str) -> Option<&'static str> {
    let message = message.strip_prefix("connect failed posting ")?.rsplit_once(": ")?.1.to_ascii_lowercase();
    Some(if message.contains("timeout") || message.contains("timed out") {
        "TIMEOUT"
    } else if message.contains("refused") {
        "REFUSED"
    } else {
        "UNKNOWN"
    })
}
/// Flatten an error's `source()` chain into one line, dropping links that only
/// repeat the URL the caller already prints.
fn error_cause_chain(error: &(dyn std::error::Error + 'static), url: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut current = Some(error);
    while let Some(link) = current {
        let text = link.to_string();
        if !text.contains(url) && !parts.contains(&text) {
            parts.push(text);
        }
        current = link.source();
    }
    if parts.is_empty() {
        "no further detail".to_owned()
    } else {
        parts.join(": ")
    }
}

struct PeerAuth<'a> {
    from: &'a str,
    federation_token: &'a str,
    peer_key: &'a str,
    timestamp: i64,
}

impl PeerSendResponse {
    #[must_use]
    pub fn delivered_or_queued(&self) -> bool {
        self.ok
            && matches!(
                self.state.as_deref().unwrap_or("queued"),
                "delivered" | "queued"
            )
    }
}

/// Concrete reqwest/rustls HTTP adapter for maw federation endpoints.
#[derive(Clone)]
pub struct ReqwestHttpTransportIo {
    pub(crate) client: reqwest::Client,
    timeout_ms: u64,
}

impl ReqwestHttpTransportIo {
    /// Build a reqwest client with rustls-only TLS features.
    ///
    /// # Errors
    ///
    /// Returns reqwest builder errors.
    pub fn new(timeout_ms: u64) -> Result<Self, String> {
        let timeout = Duration::from_millis(timeout_ms);
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .map_err(|error| format!("http client build failed: {error}"))?;
        Ok(Self { client, timeout_ms })
    }

    #[must_use]
    pub const fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// POST a signed maw v3 `/api/send` request.
    ///
    /// # Errors
    ///
    /// Returns a clear transport/auth/parse error string on failure.
    pub async fn send_peer(&self, request: &PeerSendRequest) -> Result<PeerSendResponse, String> {
        let body = peer_send_body(&request.target, &request.text, request.inbox)?;
        let (status, text) = self
            .post_signed_json(
                &request.peer_url,
                SEND_PATH,
                &body,
                PeerAuth {
                    from: &request.from,
                    federation_token: &request.federation_token,
                    peer_key: &request.peer_key,
                    timestamp: request.timestamp,
                },
            )
            .await?;
        let wire = serde_json::from_str::<PeerSendWireResponse>(&text)
            .map_err(|error| format!("failed to parse /api/send response: {error}; body={text}"))?;
        let parsed = PeerSendResponse {
            ok: wire.ok.unwrap_or(false),
            status,
            state: wire.state,
            target: wire.target,
            last_line: wire.last_line,
            error: wire.error,
            decision: wire.decision,
            warning: wire.warning,
        };
        if status >= 300 {
            return Err(peer_send_error_message(status, &parsed));
        }
        if !parsed.delivered_or_queued() {
            return Err(format!(
                "remote /api/send failed: state={} error={}",
                parsed.state.as_deref().unwrap_or("-"),
                parsed
                    .error
                    .as_deref()
                    .unwrap_or("remote returned ok=false")
            ));
        }
        Ok(parsed)
    }

    /// Try addresses in order, advancing only for connect-phase failures.
    /// # Errors
    /// Stops on post-connect/HTTP/build/sign/parse/read errors.
    pub async fn send_peer_addresses(
        &self,
        request: &PeerSendRequest,
        addresses: &[String],
    ) -> Result<PeerSendResponse, String> {
        let mut failures = Vec::new();
        for address in addresses {
            let mut attempt = request.clone();
            attempt.peer_url.clone_from(address);
            match self.send_peer(&attempt).await {
                Ok(response) => return Ok(response),
                Err(message) => match peer_send_transport_code(&message) {
                    Some(code) => failures.push(format!("{address}: {code} ({message})")),
                    None => return Err(message),
                },
            }
        }
        if failures.is_empty() {
            Err("no peer addresses configured".to_owned())
        } else {
            Err(format!("all peer addresses failed: {}", failures.join("; ")))
        }
    }

    /// POST a signed maw v3 `/api/wake` request.
    ///
    /// # Errors
    ///
    /// Returns a clear transport/auth/parse error string on failure.
    pub async fn wake_peer(&self, request: &PeerWakeRequest) -> Result<PeerWakeResponse, String> {
        let body = peer_wake_body(&request.target, request.task.as_deref())?;
        let (status, text) = self
            .post_signed_json(
                &request.peer_url,
                WAKE_PATH,
                &body,
                PeerAuth {
                    from: &request.from,
                    federation_token: &request.federation_token,
                    peer_key: &request.peer_key,
                    timestamp: request.timestamp,
                },
            )
            .await?;
        let wire = serde_json::from_str::<PeerWakeWireResponse>(&text)
            .map_err(|error| format!("failed to parse /api/wake response: {error}; body={text}"))?;
        let parsed = PeerWakeResponse {
            ok: wire.ok.unwrap_or(false),
            status,
            target: wire.target,
            error: wire.error,
        };
        if status >= 300 {
            return Err(format!(
                "remote /api/wake returned HTTP {status}: {}",
                parsed.error.as_deref().unwrap_or("request failed")
            ));
        }
        if !parsed.ok {
            return Err(format!(
                "remote /api/wake failed: error={}",
                parsed
                    .error
                    .as_deref()
                    .unwrap_or("remote returned ok=false")
            ));
        }
        Ok(parsed)
    }

    /// Read-only auth probe: POST a signed `/api/probe` (which verifies the
    /// v3 from-signature and returns `{ok:true, sessions:[]}` with NO side
    /// effect) so a probe can tell whether OUR signed requests are trusted by
    /// this peer without delivering a real message. `Some(true)` on 2xx,
    /// `Some(false)` on 401/403 (auth refused), `None` on any other outcome.
    ///
    /// # Errors
    ///
    /// Returns a transport error string on network failure.
    pub async fn probe_peer_auth(
        &self,
        request: &PeerWakeRequest,
    ) -> Result<PeerProbeAuthResult, String> {
        let (status, text) = self
            .post_signed_json(
                &request.peer_url,
                "/api/probe",
                "{}",
                PeerAuth {
                    from: &request.from,
                    federation_token: &request.federation_token,
                    peer_key: &request.peer_key,
                    timestamp: request.timestamp,
                },
            )
            .await?;
        // #685: the reason for a 401/403 (pubkey mismatch, refused-unsigned,
        // token mismatch, ...) is already in the response body -- carry it
        // through instead of reducing straight to a bare bool, the same
        // treatment #671 gave `fetch_error`.
        let reason = serde_json::from_str::<PeerProbeWireResponse>(&text)
            .ok()
            .and_then(|wire| wire.decision);
        Ok(match status {
            200..=299 => PeerProbeAuthResult { ok: Some(true), reason: None },
            401 | 403 => PeerProbeAuthResult { ok: Some(false), reason },
            _ => PeerProbeAuthResult { ok: None, reason: None },
        })
    }

    async fn post_signed_json(
        &self,
        peer_url: &str,
        path: &str,
        body: &str,
        auth: PeerAuth<'_>,
    ) -> Result<(u16, String), String> {
        let headers = sign_headers_v3_at(
            auth.federation_token,
            auth.peer_key,
            auth.from,
            POST_METHOD,
            path,
            Some(body.as_bytes()),
            auth.timestamp,
        )?;
        let url = format!("{}{}", peer_url.trim_end_matches('/'), path);
        let mut builder = self
            .client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_owned());
        for (name, value) in headers.to_btree_map() {
            builder = builder.header(name.as_str(), value.as_str());
        }

        let response = builder
            .send()
            .await
            .map_err(|error| request_error_message("posting", &url, &error))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|error| request_error_message("reading", &url, &error))?;
        Ok((status, text))
    }
}

#[cfg(test)]
mod transport_error_tests {
    use super::{error_cause_chain, PeerAuth, ReqwestHttpTransportIo};

    /// Nothing listens on port 1, so this is a refusal, not a timeout — and it
    /// needs no network beyond loopback.
    const REFUSED_URL: &str = "http://127.0.0.1:1";

    fn auth(from: &str) -> PeerAuth<'_> {
        PeerAuth {
            from,
            federation_token: "token",
            peer_key: "peer-key",
            timestamp: 1_782_345_900,
        }
    }

    async fn post_error(peer_url: &str, from: &str) -> String {
        let io = ReqwestHttpTransportIo::new(2000).expect("client");
        io.post_signed_json(peer_url, "/api/send", "{}", auth(from))
            .await
            .expect_err("must fail")
    }

    #[tokio::test]
    async fn refused_connection_names_the_layer_and_the_reason() {
        let message = post_error(REFUSED_URL, "maw-rs:black").await;
        assert!(
            message.starts_with("connect failed posting"),
            "should name the layer that failed: {message}"
        );
        assert!(
            message.to_lowercase().contains("refused"),
            "the actual reason lives in source() and must be carried up: {message}"
        );
        // The bare reqwest Display prints the URL again after we already have it.
        assert_eq!(message.matches(REFUSED_URL).count(), 1, "{message}");
    }

    /// A from-address carrying a control character cannot go in a header, so
    /// the request dies while still being built. Calling that a "network
    /// error" sends the reader to inspect a peer that was never contacted —
    /// the port here is closed and stays irrelevant to the failure.
    #[tokio::test]
    async fn unsendable_header_is_not_reported_as_a_network_error() {
        let message = post_error(REFUSED_URL, "black\nX-Injected: 1").await;
        assert!(
            message.starts_with("invalid request (never sent) posting"),
            "a header we built wrong is ours, not the network's: {message}"
        );
        assert!(
            !message.contains("network error"),
            "must not blame the network: {message}"
        );
    }

    /// Refutes the first hypothesis raised when federation broke on
    /// 2026-07-28: that a non-ASCII oracle name produced a header reqwest
    /// would reject. It does not — HTTP allows bytes 128-255 in a header
    /// value as obs-text, so `ψ` travels fine and the request reaches the
    /// network like any other. Recorded as a test so the idea is not
    /// re-litigated by reading the code.
    #[tokio::test]
    async fn non_ascii_from_address_still_reaches_the_network() {
        let message = post_error(REFUSED_URL, "ψ:black").await;
        assert!(
            message.starts_with("connect failed posting"),
            "a non-ASCII from-address is sendable, not a builder error: {message}"
        );
    }

    #[test]
    fn cause_chain_drops_links_that_only_repeat_the_url() {
        #[derive(Debug)]
        struct Link(&'static str, Option<Box<Link>>);
        impl std::fmt::Display for Link {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.0)
            }
        }
        impl std::error::Error for Link {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                self.1.as_deref().map(|link| link as &(dyn std::error::Error + 'static))
            }
        }

        let chain = Link(
            "error sending request for url (http://peer/api/send)",
            Some(Box::new(Link("tcp connect error", Some(Box::new(Link("Connection refused", None)))))),
        );
        assert_eq!(
            error_cause_chain(&chain, "http://peer/api/send"),
            "tcp connect error: Connection refused"
        );
    }

    #[test]
    fn cause_chain_without_usable_links_still_says_something() {
        #[derive(Debug)]
        struct Only;
        impl std::fmt::Display for Only {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("http://peer/api/send")
            }
        }
        impl std::error::Error for Only {}
        assert_eq!(
            error_cause_chain(&Only, "http://peer/api/send"),
            "no further detail"
        );
    }
}

#[cfg(test)]
mod decision_tests {
    use super::{decision_hint, peer_send_error_message, PeerSendResponse};

    fn resp(error: Option<&str>, decision: Option<&str>) -> PeerSendResponse {
        PeerSendResponse {
            ok: false,
            status: 401,
            state: None,
            target: None,
            last_line: None,
            error: error.map(str::to_owned),
            decision: decision.map(str::to_owned),
            warning: None,
        }
    }

    #[test]
    fn error_message_surfaces_decision_and_hint() {
        let msg = peer_send_error_message(
            401,
            &resp(Some("unauthorized"), Some("refuse-missing-peer-key")),
        );
        assert!(msg.contains("HTTP 401"));
        assert!(msg.contains("[decision=refuse-missing-peer-key]"));
        assert!(msg.contains("identity.oracle"));
    }

    /// #822: the hint used to end with "the receiver's serve loaded pubkeys at
    /// startup and needs a restart after the peer was added". Serve hot-reloads
    /// `peer_pubkeys` on a short TTL (`PEER_PUBKEY_RELOAD_TTL_SECS`, applied by
    /// `serve_hot_reload.rs`), so that restart is an action with no effect, and
    /// it was followed three times during a real two-host bring-up before
    /// anyone doubted it. Pin the replacement: name the field the match is
    /// actually decided on (#819), and let "restart" survive only inside its
    /// own denial.
    #[test]
    fn missing_peer_key_hint_names_the_field_and_never_prescribes_a_restart() {
        let hint = decision_hint("refuse-missing-peer-key").expect("hint for a known decision");
        assert!(hint.contains("identity.oracle"), "{hint}");
        assert!(hint.contains("--oracle"), "{hint}");
        let denial = "restarting it does not help";
        assert!(hint.contains(denial), "{hint}");
        assert!(!hint.replace(denial, "").contains("restart"), "{hint}");
    }

    #[test]
    fn error_message_without_decision_is_unchanged() {
        let msg = peer_send_error_message(500, &resp(Some("boom"), None));
        assert_eq!(msg, "remote /api/send returned HTTP 500: boom");
    }

    #[test]
    fn unknown_decision_shows_code_but_no_hint() {
        assert!(decision_hint("refuse-skew").is_some());
        assert!(decision_hint("something-new").is_none());
        let msg = peer_send_error_message(401, &resp(None, Some("something-new")));
        assert!(msg.contains("[decision=something-new]"));
    }
}
