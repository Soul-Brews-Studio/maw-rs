fn format_scout_warning(error: &str, hint: Option<&str>) -> String {
    if let Some(hint) = hint {
        format!("scout unavailable ({error}: {hint})")
    } else {
        format!("scout unavailable ({error})")
    }
}

/// Structured peer probe failure code, ported from maw-js `probe.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeErrorCode {
    #[serde(rename = "DNS")]
    Dns,
    #[serde(rename = "REFUSED")]
    Refused,
    #[serde(rename = "TIMEOUT")]
    Timeout,
    #[serde(rename = "HTTP_4XX")]
    Http4xx,
    #[serde(rename = "HTTP_5XX")]
    Http5xx,
    #[serde(rename = "TLS")]
    Tls,
    #[serde(rename = "BAD_BODY")]
    BadBody,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

impl ProbeErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dns => "DNS",
            Self::Refused => "REFUSED",
            Self::Timeout => "TIMEOUT",
            Self::Http4xx => "HTTP_4XX",
            Self::Http5xx => "HTTP_5XX",
            Self::Tls => "TLS",
            Self::BadBody => "BAD_BODY",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Deterministic stand-in for JS `Response`/thrown-error shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeFailureInput {
    Http { status: u16, ok: bool },
    CauseCode(String),
    Code(String),
    Name(String),
    NonObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeLastError {
    pub code: ProbeErrorCode,
    pub message: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeMawHandshake {
    LegacyTrue,
    SchemaObject(String),
    EmptyObject,
    OtherTruthy,
    Missing,
}

#[must_use]
pub fn classify_probe_error(input: &ProbeFailureInput) -> ProbeErrorCode {
    match input {
        ProbeFailureInput::Http { status, ok } if !ok && (400..500).contains(status) => {
            ProbeErrorCode::Http4xx
        }
        ProbeFailureInput::Http { status, ok } if !ok && *status >= 500 => ProbeErrorCode::Http5xx,
        ProbeFailureInput::CauseCode(code) | ProbeFailureInput::Code(code) => classify_code(code),
        ProbeFailureInput::Name(name) if name == "AbortError" || name == "TimeoutError" => {
            ProbeErrorCode::Timeout
        }
        ProbeFailureInput::Http { .. }
        | ProbeFailureInput::NonObject
        | ProbeFailureInput::Name(_) => ProbeErrorCode::Unknown,
    }
}

fn classify_code(code: &str) -> ProbeErrorCode {
    match code {
        "ENOTFOUND" | "ENOTIMP" | "EAI_FAIL" | "EAI_AGAIN" | "EAI_NODATA" => ProbeErrorCode::Dns,
        "ECONNREFUSED" | "ConnectionRefused" => ProbeErrorCode::Refused,
        "ETIMEDOUT" | "UND_ERR_CONNECT_TIMEOUT" => ProbeErrorCode::Timeout,
        "UNABLE_TO_VERIFY_LEAF_SIGNATURE" => ProbeErrorCode::Tls,
        _ if code.starts_with("CERT_")
            || code.starts_with("SELF_SIGNED")
            || code.starts_with("DEPTH_ZERO_") =>
        {
            ProbeErrorCode::Tls
        }
        _ => ProbeErrorCode::Unknown,
    }
}

#[must_use]
pub const fn probe_exit_code(code: ProbeErrorCode) -> i32 {
    match code {
        ProbeErrorCode::Dns => 3,
        ProbeErrorCode::Refused => 4,
        ProbeErrorCode::Timeout => 5,
        ProbeErrorCode::Http4xx | ProbeErrorCode::Http5xx => 6,
        ProbeErrorCode::Tls | ProbeErrorCode::BadBody | ProbeErrorCode::Unknown => 2,
    }
}

#[must_use]
pub const fn probe_hint(code: ProbeErrorCode) -> &'static str {
    match code {
        ProbeErrorCode::Dns => "Host does not resolve. Check /etc/hosts, DNS, or VPN.",
        ProbeErrorCode::Refused => "Host resolves but port is closed. Is the peer process running?",
        ProbeErrorCode::Timeout => "Peer did not respond within 2s. Network path may be blocked.",
        ProbeErrorCode::Tls => "TLS handshake failed. Check cert validity / chain.",
        ProbeErrorCode::Http4xx => "Peer responded with a client error. /info endpoint may be missing OR peer is running an old maw version — if you control the peer, try restarting it.",
        ProbeErrorCode::Http5xx => "Peer returned a server error. Server-side fault.",
        ProbeErrorCode::BadBody => "/info responded but body shape was unexpected.",
        ProbeErrorCode::Unknown => "Probe failed for an unclassified reason.",
    }
}

#[must_use]
pub fn is_valid_maw_handshake(maw: &ProbeMawHandshake) -> bool {
    match maw {
        ProbeMawHandshake::LegacyTrue => true,
        ProbeMawHandshake::SchemaObject(schema) => !schema.is_empty(),
        ProbeMawHandshake::EmptyObject
        | ProbeMawHandshake::OtherTruthy
        | ProbeMawHandshake::Missing => false,
    }
}

#[must_use]
pub fn pick_probe_hint(err: &ProbeLastError) -> &'static str {
    if err.code == ProbeErrorCode::Dns && err.message.to_uppercase().contains("ENOTIMP") {
        return "install avahi-daemon (Linux) for mDNS, or add white.local to /etc/hosts";
    }
    probe_hint(err.code)
}

#[must_use]
pub fn format_probe_error(err: &ProbeLastError, url: &str, alias: &str) -> String {
    let hint = pick_probe_hint(err);
    let host = safe_probe_host(url);
    [
        format!(
            "\u{1b}[33m⚠\u{1b}[0m peer handshake failed: \u{1b}[1m{}\u{1b}[0m",
            err.code.as_str()
        ),
        format!("   host: {host}"),
        format!("   error: {}", err.message),
        format!("   hint: {hint}"),
        format!("   retry: maw peers probe {alias}"),
    ]
    .join("\n")
}

#[must_use]
pub fn safe_probe_host(url: &str) -> String {
    let Some(rest) = url.split_once("://").map(|(_, rest)| rest) else {
        return url.to_owned();
    };
    let host = rest.split('/').next().unwrap_or(rest);
    if host.is_empty() {
        url.to_owned()
    } else {
        host.to_owned()
    }
}

/// Parsed `/info` body shape for deterministic `probePeer` ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeInfoBody {
    pub maw: ProbeMawHandshake,
    pub node: Option<String>,
    pub name: Option<String>,
    pub nickname: Option<String>,
}

/// Deterministic stand-in for the maw-js `/info` fetch result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeInfoOutcome {
    Body(ProbeInfoBody),
    HttpStatus { status: u16, ok: bool },
    InvalidJson,
    FetchCode { code: String, message: String },
    FetchCodeWithoutMessage { code: String },
    FetchName { name: String, message: String },
}

/// Deterministic stand-in for the best-effort `/api/identity` fetch result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeRemoteIdentity {
    Body {
        pubkey: Option<String>,
        oracle: Option<String>,
        node: Option<String>,
    },
    Missing,
    HttpError,
    MalformedJson,
    FetchError,
}

/// Peer's self-reported `<oracle>:<node>` identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerIdentity {
    pub oracle: String,
    pub node: String,
}
