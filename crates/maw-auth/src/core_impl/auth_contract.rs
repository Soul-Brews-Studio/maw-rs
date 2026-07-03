// Federation auth pure helpers ported from maw-js `src/lib/federation-auth.ts`.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    net::IpAddr,
    sync::{Arc, Mutex},
};

type HmacSha256 = Hmac<Sha256>;

pub const WINDOW_SEC: i64 = 300;
pub const DEFAULT_ORACLE: &str = "mawjs";
pub const PAIR_CODE_ALPHABET: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headers(BTreeMap<String, String>);

impl Headers {
    #[must_use]
    pub fn new(entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>) -> Self {
        let mut map = BTreeMap::new();
        for (key, value) in entries {
            map.insert(key.into().to_lowercase(), value.into());
        }
        Self(map)
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(&name.to_lowercase()).map(String::as_str)
    }

    #[must_use]
    pub fn to_btree_map(&self) -> BTreeMap<String, String> {
        self.0.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V3Signature {
    pub signature: String,
    pub body_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromAddressConfig {
    pub oracle: Option<String>,
    pub node: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPairIdentity {
    pub node: String,
    pub oracle: String,
    pub url: String,
    pub pubkey: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsentAction {
    Hey,
    TeamInvite,
    PluginInstall,
}

impl ConsentAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hey => "hey",
            Self::TeamInvite => "team-invite",
            Self::PluginInstall => "plugin-install",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovedBy {
    Human,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustEntry {
    pub from: String,
    pub to: String,
    pub action: ConsentAction,
    pub approved_at: String,
    pub approved_by: ApprovedBy,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRequest {
    pub id: String,
    pub from: String,
    pub to: String,
    pub action: ConsentAction,
    pub summary: String,
    pub pin_hash: String,
    pub created_at: String,
    pub expires_at: String,
    pub status: ConsentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerPendingRequest {
    pub id: String,
    pub from: String,
    pub to: String,
    pub action: ConsentAction,
    pub summary: String,
    pub pin_hash: String,
    pub created_at: String,
    pub expires_at: String,
    pub status: ConsentStatus,
    pub pin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerPostResult {
    Skipped,
    Ok,
    HttpStatus(u16),
    NetworkError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRequestArgs {
    pub from: String,
    pub to: String,
    pub action: ConsentAction,
    pub summary: String,
    pub peer_url: Option<String>,
    pub request_id: String,
    pub pin: String,
    pub now_ms: i64,
    pub peer_post: PeerPostResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRequestResult {
    pub ok: bool,
    pub request_id: Option<String>,
    pub pin: Option<String>,
    pub expires_at: Option<String>,
    pub error: Option<String>,
    pub already_trusted: bool,
    pub peer_url: Option<String>,
    pub peer_method: Option<String>,
    pub peer_body: Option<PeerPendingRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentApprovalResult {
    pub ok: bool,
    pub error: Option<String>,
    pub entry: Option<TrustEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct ConsentStore {
    trust: BTreeMap<String, TrustEntry>,
    pending: BTreeMap<String, PendingRequest>,
}

impl ConsentStore {
    pub fn record_trust(&mut self, entry: TrustEntry) {
        self.trust
            .insert(trust_key(&entry.from, &entry.to, entry.action), entry);
    }

    #[must_use]
    pub fn remove_trust(&mut self, from: &str, to: &str, action: ConsentAction) -> bool {
        self.trust.remove(&trust_key(from, to, action)).is_some()
    }

    #[must_use]
    pub fn is_trusted(&self, from: &str, to: &str, action: ConsentAction) -> bool {
        self.trust.contains_key(&trust_key(from, to, action))
    }

    #[must_use]
    pub fn list_trust(&self) -> Vec<TrustEntry> {
        let mut entries = self.trust.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.approved_at.cmp(&b.approved_at));
        entries
    }

    pub fn write_pending(&mut self, request: PendingRequest) {
        self.pending.insert(request.id.clone(), request);
    }

    #[must_use]
    pub fn read_pending(&self, id: &str) -> Option<PendingRequest> {
        self.pending.get(id).cloned()
    }

    #[must_use]
    pub fn list_pending(&self) -> Vec<PendingRequest> {
        let mut entries = self.pending.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries
    }

    pub fn update_status(&mut self, id: &str, status: ConsentStatus) -> bool {
        let Some(request) = self.pending.get_mut(id) else {
            return false;
        };
        request.status = status;
        true
    }

    pub fn delete_pending(&mut self, id: &str) -> bool {
        self.pending.remove(id).is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairEntry {
    pub code: String,
    pub expires_at: u64,
    pub consumed: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairApiConfig {
    pub node: String,
    pub oracle: String,
    pub port: u16,
    pub base_url: String,
    pub federation_token: String,
    pub pubkey: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairApiGenerateResult {
    pub status: u16,
    pub ok: bool,
    pub code: String,
    pub expires_at: u64,
    pub ttl_ms: u64,
    pub node: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairApiProbeResult {
    pub status: u16,
    pub ok: bool,
    pub error: Option<String>,
    pub node: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairAcceptInput {
    pub node: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairApiAcceptResult {
    pub status: u16,
    pub ok: bool,
    pub error: Option<String>,
    pub node: Option<String>,
    pub url: Option<String>,
    pub federation_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairApiStatusResult {
    pub status: u16,
    pub ok: bool,
    pub error: Option<String>,
    pub consumed: Option<bool>,
    pub remote_node: Option<String>,
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPairInput {
    pub node: String,
    pub oracle: Option<String>,
    pub url: String,
    pub zid: String,
    pub pubkey: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoPairAddOutcome {
    Ok { one_way: bool },
    PubkeyMismatch(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairApiAutoResult {
    pub status: u16,
    pub ok: bool,
    pub error: Option<String>,
    pub node: Option<String>,
    pub oracle: Option<String>,
    pub url: Option<String>,
    pub pubkey: Option<String>,
    pub proof: Option<String>,
    pub one_way: Option<bool>,
    pub add_alias: Option<String>,
    pub add_url: Option<String>,
    pub add_node: Option<String>,
    pub add_pubkey: Option<String>,
    pub add_identity_oracle: Option<String>,
    pub add_identity_node: Option<String>,
    pub mark_symmetric_check: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RecentHelloStore {
    seen_at: BTreeMap<String, u64>,
}

impl RecentHelloStore {
    pub fn record(&mut self, zid: &str, now_ms: u64) {
        self.seen_at.insert(zid.to_owned(), now_ms);
    }

    #[must_use]
    pub fn is_recent(&self, zid: &str, now_ms: u64) -> bool {
        const RECENT_HELLO_MS: u64 = 60_000;
        self.seen_at
            .get(zid)
            .is_some_and(|seen_at| now_ms.saturating_sub(*seen_at) <= RECENT_HELLO_MS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupResult {
    Live(PairEntry),
    NotFound,
    Expired,
    Consumed,
}

#[derive(Debug, Clone, Default)]
pub struct PairCodeStore {
    entries: BTreeMap<String, PairEntry>,
    accepted: BTreeMap<String, PairAcceptInput>,
}

impl PairCodeStore {
    #[must_use]
    pub fn register_at(&mut self, code: &str, ttl_ms: u64, now_ms: u64) -> PairEntry {
        let entry = PairEntry {
            code: normalize_pair_code(code),
            expires_at: now_ms.saturating_add(ttl_ms),
            consumed: false,
            created_at: now_ms,
        };
        self.entries.insert(entry.code.clone(), entry.clone());
        entry
    }

    #[must_use]
    pub fn lookup_at(&self, code: &str, now_ms: u64) -> LookupResult {
        let Some(entry) = self.entries.get(&normalize_pair_code(code)).cloned() else {
            return LookupResult::NotFound;
        };
        if entry.consumed {
            return LookupResult::Consumed;
        }
        if now_ms > entry.expires_at {
            return LookupResult::Expired;
        }
        LookupResult::Live(entry)
    }

    #[must_use]
    pub fn consume_at(&mut self, code: &str, now_ms: u64) -> LookupResult {
        let key = normalize_pair_code(code);
        match self.lookup_at(&key, now_ms) {
            LookupResult::Live(mut entry) => {
                entry.consumed = true;
                self.entries.insert(key, entry.clone());
                LookupResult::Live(entry)
            }
            other => other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromVerifyDecision {
    AcceptLegacy {
        reason: String,
    },
    AcceptTofuRecord {
        reason: String,
        from: String,
    },
    AcceptVerified {
        reason: String,
        from: String,
    },
    RefuseUnsigned {
        reason: String,
        from: Option<String>,
    },
    RefuseMismatch {
        reason: String,
        from: String,
    },
    RefuseSkew {
        reason: String,
        from: String,
        delta: i64,
    },
    RefuseMalformed {
        reason: String,
    },
}

impl FromVerifyDecision {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::AcceptLegacy { .. } => "accept-legacy",
            Self::AcceptTofuRecord { .. } => "accept-tofu-record",
            Self::AcceptVerified { .. } => "accept-verified",
            Self::RefuseUnsigned { .. } => "refuse-unsigned",
            Self::RefuseMismatch { .. } => "refuse-mismatch",
            Self::RefuseSkew { .. } => "refuse-skew",
            Self::RefuseMalformed { .. } => "refuse-malformed",
        }
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestAuthDecision {
    Accept { who: String },
    Reject { reason: String },
}

impl RequestAuthDecision {
    #[must_use]
    pub fn is_accept(&self) -> bool {
        matches!(self, Self::Accept { .. })
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Accept { .. } => None,
            Self::Reject { reason } => Some(reason),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ed25519TofuStore {
    pins: BTreeMap<String, String>,
    backing_file: Option<std::path::PathBuf>,
    poisoned: bool,
}

impl Ed25519TofuStore {
    #[must_use]
    pub fn file_backed(path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        match ed25519_tofu_load_pins(&path) {
            Ed25519TofuLoad::Ok(pins) => Self {
                pins,
                backing_file: Some(path),
                poisoned: false,
            },
            Ed25519TofuLoad::NotFound => Self {
                pins: BTreeMap::new(),
                backing_file: Some(path),
                poisoned: false,
            },
            Ed25519TofuLoad::Corrupt => Self {
                pins: BTreeMap::new(),
                backing_file: Some(path),
                poisoned: true,
            },
        }
    }

    #[must_use]
    pub fn pinned(&self, from: &str) -> Option<&str> {
        self.pins.get(from).map(String::as_str)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pins.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    #[must_use]
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    pub fn pin_first_contact(&mut self, from: &str, pubkey_hex: &str) -> bool {
        if self.poisoned
            || self.pins.contains_key(from)
            || !ed25519_tofu_valid_pin(from, pubkey_hex)
        {
            return false;
        }
        self.pins.insert(from.to_owned(), pubkey_hex.to_owned());
        if self.ed25519_tofu_persist().is_err() {
            self.pins.remove(from);
            return false;
        }
        true
    }

    fn ed25519_tofu_persist(&self) -> Result<(), std::io::Error> {
        let Some(path) = &self.backing_file else {
            return Ok(());
        };
        ed25519_tofu_write_atomic(path, &self.pins)
    }
}

enum Ed25519TofuLoad {
    Ok(BTreeMap<String, String>),
    NotFound,
    Corrupt,
}

fn ed25519_tofu_load_pins(path: &std::path::Path) -> Ed25519TofuLoad {
    let Ok(path) = ed25519_tofu_safe_path(path) else {
        return Ed25519TofuLoad::Corrupt;
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ed25519TofuLoad::NotFound;
        }
        Err(_) => return Ed25519TofuLoad::Corrupt,
    };
    let Ok(parsed) = serde_json::from_slice::<BTreeMap<String, String>>(&bytes) else {
        return Ed25519TofuLoad::Corrupt;
    };
    if parsed
        .iter()
        .all(|(from, pubkey)| ed25519_tofu_valid_pin(from, pubkey))
    {
        Ed25519TofuLoad::Ok(parsed)
    } else {
        Ed25519TofuLoad::Corrupt
    }
}

fn ed25519_tofu_valid_pin(from: &str, pubkey_hex: &str) -> bool {
    ed25519_tofu_valid_from(from) && ed25519_tofu_valid_pubkey(pubkey_hex)
}

fn ed25519_tofu_valid_from(from: &str) -> bool {
    let value = from.trim();
    !value.is_empty()
        && value == from
        && !value.starts_with('-')
        && !value.contains("..")
        && !value.contains('/')
        && !value.contains('\\')
        && value.chars().all(|ch| !ch.is_control())
}

fn ed25519_tofu_valid_pubkey(pubkey_hex: &str) -> bool {
    pubkey_hex.len() == 64 && pubkey_hex.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn ed25519_tofu_write_atomic(
    path: &std::path::Path,
    pins: &BTreeMap<String, String>,
) -> Result<(), std::io::Error> {
    let final_path = ed25519_tofu_safe_path(path)?;
    let tmp_path = final_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(pins).map_err(std::io::Error::other)?;
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(tmp_path, final_path)
}

fn ed25519_tofu_safe_path(path: &std::path::Path) -> Result<std::path::PathBuf, std::io::Error> {
    if ed25519_tofu_has_traversal(path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "tofu path traversal",
        ));
    }
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "tofu path missing parent",
        ));
    };
    std::fs::create_dir_all(parent)?;
    let parent = parent.canonicalize()?;
    let Some(name) = path.file_name() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "tofu path missing file name",
        ));
    };
    let final_path = parent.join(name);
    if final_path.starts_with(&parent) {
        Ok(final_path)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "tofu path escapes parent",
        ))
    }
}

fn ed25519_tofu_has_traversal(path: &std::path::Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    })
}

pub type Ed25519TofuPins = Arc<Mutex<Ed25519TofuStore>>;

#[derive(Debug, Clone)]
pub struct RequestAuthParts {
    pub method: String,
    pub path: String,
    pub headers: Headers,
    pub body: Option<Vec<u8>>,
    pub peer_ip: Option<IpAddr>,
    pub workspace_key: Option<String>,
    pub cached_pubkey: Option<String>,
    pub ed25519_pins: Option<Ed25519TofuPins>,
    pub now: i64,
}

pub trait VerifyRequestInput {
    type Decision;

    fn verify_request_input(&self) -> Self::Decision;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyRequestArgs {
    pub method: String,
    pub path: String,
    pub headers: Headers,
    pub body: Option<Vec<u8>>,
    pub cached_pubkey: Option<String>,
    pub now: i64,
}

#[must_use]
pub fn hash_body(body: Option<&[u8]>) -> String {
    let Some(body) = body else {
        return String::new();
    };
    if body.is_empty() {
        return String::new();
    }
    hex_lower(&Sha256::digest(body))
}

#[must_use]
pub fn sign(token: &str, method: &str, path: &str, timestamp: i64, body_hash: &str) -> String {
    let payload = if body_hash.is_empty() {
        format!("{method}:{path}:{timestamp}")
    } else {
        format!("{method}:{path}:{timestamp}:{body_hash}")
    };
    hmac_sha256_hex(token, &payload)
}

#[must_use]
pub fn verify(
    token: &str,
    method: &str,
    path: &str,
    timestamp: i64,
    signature: &str,
    body_hash: &str,
    now: i64,
) -> bool {
    let delta = (now - timestamp).abs();
    if delta > WINDOW_SEC {
        return false;
    }
    let expected = sign(token, method, path, timestamp, body_hash);
    expected.len() == signature.len() && constant_time_eq(expected.as_bytes(), signature.as_bytes())
}

