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
}

/// Parsed `/api/wake` response outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerWakeResponse {
    pub ok: bool,
    pub status: u16,
    pub target: Option<String>,
    pub error: Option<String>,
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
/// masquerading as a permissions problem when it is really a stale key cache
/// or a missing signature.
fn decision_hint(decision: &str) -> Option<&'static str> {
    Some(match decision {
        "refuse-missing-peer-key" => "sender's pubkey is not in the receiver's peers.json — or the receiver's serve loaded pubkeys at startup and needs a restart after the peer was added",
        "refuse-mismatch" => "signature mismatch: the sender's ~/.maw/peer-key differs from the receiver's pinned pubkey (key rotated, or MAW_HOME/MAW_PEER_KEY set differently in a worktree?)",
        "refuse-unsigned" => "the request carried no X-Maw-Signature",
        "refuse-ambiguous-peer-key" => "the receiver has multiple pubkeys pinned for this sender",
        "refuse-skew" => "timestamp skew too large — check both machines' clocks",
        "cache-no-sig" => "no signature was cached for verification",
        _ => return None,
    })
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
        };
        if status >= 400 {
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
        if status >= 400 {
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
    ) -> Result<Option<bool>, String> {
        let (status, _text) = self
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
        Ok(match status {
            200..=299 => Some(true),
            401 | 403 => Some(false),
            _ => None,
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
            .map_err(|error| format!("network error posting {url}: {error}"))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|error| format!("network error reading {url}: {error}"))?;
        Ok((status, text))
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
        assert!(msg.contains("restart"));
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
