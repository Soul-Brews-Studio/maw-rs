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
}

/// Parsed `/api/wake` response outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerWakeResponse {
    pub ok: bool,
    pub status: u16,
    pub target: Option<String>,
    pub error: Option<String>,
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
        };
        if status >= 400 {
            return Err(format!(
                "remote /api/send returned HTTP {status}: {}",
                parsed.error.as_deref().unwrap_or("request failed")
            ));
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
