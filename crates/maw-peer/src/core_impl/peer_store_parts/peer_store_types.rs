#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRemoteIdentity {
    pubkey: Option<String>,
    identity: Option<PeerIdentity>,
}

fn parse_remote_identity(identity: &ProbeRemoteIdentity) -> Option<ParsedRemoteIdentity> {
    let ProbeRemoteIdentity::Body {
        pubkey,
        oracle,
        node,
    } = identity
    else {
        return None;
    };

    let pubkey = pubkey
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let node = node.as_deref().filter(|value| !value.is_empty());
    let identity = node.map(|node| PeerIdentity {
        // A peer that does not report an oracle stores "" — rendered as "-"/"—" downstream —
        // never a fabricated "mawjs" default. A default that looks like probed data is the
        // bug, not the missing data (#677).
        oracle: oracle
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_owned(),
        node: node.to_owned(),
    });

    Some(ParsedRemoteIdentity { pubkey, identity })
}

fn probe_bad_body(message: &str, now: &str) -> ProbePeerResult {
    probe_failure(ProbeLastError {
        code: ProbeErrorCode::BadBody,
        message: message.to_owned(),
        at: now.to_owned(),
    })
}

fn probe_failure(error: ProbeLastError) -> ProbePeerResult {
    ProbePeerResult {
        node: None,
        nickname: None,
        pubkey: None,
        identity: None,
        error: Some(error),
        ..Default::default()
    }
}

/// Peer store record subset used by maw-js `probe-all`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRecord {
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(rename = "addedAt")]
    pub added_at: String,
    #[serde(default, rename = "lastSeen")]
    pub last_seen: Option<String>,
    #[serde(default, rename = "lastError")]
    pub last_error: Option<ProbeLastError>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub pubkey: Option<String>,
    #[serde(default, rename = "pubkeyFirstSeen")]
    pub pubkey_first_seen: Option<String>,
    #[serde(default)]
    pub identity: Option<PeerIdentity>,
    #[serde(default, rename = "oneWay")]
    pub one_way: Option<bool>,
    #[serde(default, rename = "lastSymmetricCheck")]
    pub last_symmetric_check: Option<String>,
    /// Last read-only auth probe: whether OUR signed requests are trusted by this
    /// peer (`POST /api/probe`). `None` = not checked / unreachable.
    #[serde(default, rename = "authOk")]
    pub auth_ok: Option<bool>,
}

/// Order configured peer addresses without I/O. Probe health describes the
/// first address: a newer failure demotes it behind the stable alternatives.
#[must_use]
pub fn ordered_peer_addresses(
    addresses: &[String],
    last_seen: Option<&str>,
    last_error: Option<&ProbeLastError>,
) -> Vec<String> {
    let mut ordered = Vec::new();
    for address in addresses.iter().filter(|address| !address.trim().is_empty()) {
        if !ordered.contains(address) {
            ordered.push(address.clone());
        }
    }
    let primary_failed = last_error.is_some_and(|error| {
        last_seen.is_none_or(|seen| {
            match (parse_timestamp_ms(&error.at), parse_timestamp_ms(seen)) {
                (Some(failed), Some(seen)) => failed >= seen,
                _ => true,
            }
        })
    });
    if primary_failed && ordered.len() > 1 {
        ordered.rotate_left(1);
    }
    ordered
}

/// Peer store file shape, ported from maw-js peers `store.ts` schema v1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerStoreFile {
    pub version: u8,
    #[serde(default)]
    pub peers: BTreeMap<String, PeerRecord>,
}

impl Default for PeerStoreFile {
    fn default() -> Self {
        Self {
            version: 1,
            peers: BTreeMap::new(),
        }
    }
}

/// Stale peer row used by doctor `--fix-stale` preview and mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalePeer {
    pub alias: String,
    pub url: String,
    pub age_ms: Option<u64>,
}

/// Doctor check-shaped result for peers stale/fix-stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerDoctorCheck {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TofuDecisionKind {
    TofuBootstrap,
    Match,
    Mismatch,
    LegacyFirstContact,
    LegacyAfterPinned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TofuDecision {
    pub kind: TofuDecisionKind,
    pub alias: String,
    pub cached: Option<String>,
    pub observed: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerPubkeyMismatchError {
    pub alias: String,
    pub cached: String,
    pub observed: String,
}

impl PeerPubkeyMismatchError {
    #[must_use]
    pub fn new(
        alias: impl Into<String>,
        cached: impl Into<String>,
        observed: impl Into<String>,
    ) -> Self {
        Self {
            alias: alias.into(),
            cached: cached.into(),
            observed: observed.into(),
        }
    }
}

impl std::fmt::Display for PeerPubkeyMismatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "peer pubkey changed for {}: {}… → {}…; manually `maw peers forget {}` to re-TOFU",
            self.alias,
            prefix16(&self.cached),
            prefix16(&self.observed),
            self.alias
        )
    }
}

impl Error for PeerPubkeyMismatchError {}

/// Deterministic peer-store environment for maw-js path resolution parity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerStoreEnv {
    xdg: MawXdgEnv,
}

impl PeerStoreEnv {
    #[must_use]
    pub fn new(home_dir: impl Into<PathBuf>) -> Self {
        Self {
            xdg: MawXdgEnv::new(home_dir),
        }
    }

    #[must_use]
    pub fn with_vars(
        home_dir: impl Into<PathBuf>,
        vars: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            xdg: MawXdgEnv::with_vars(home_dir, vars),
        }
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.xdg.var(name)
    }

    fn home_dir(&self) -> &Path {
        self.xdg.home_dir()
    }
}

#[must_use]
pub fn empty_peer_store() -> PeerStoreFile {
    PeerStoreFile::default()
}

/// Resolve the active `peers.json` path.
#[must_use]
pub fn peer_store_path(env: &PeerStoreEnv) -> PathBuf {
    env.var("PEERS_FILE")
        .map_or_else(|| maw_state_path(&env.xdg, &["peers.json"]), PathBuf::from)
}

fn legacy_peer_store_path(env: &PeerStoreEnv) -> Option<PathBuf> {
    if env.var("PEERS_FILE").is_some() || env.var("MAW_HOME").is_some() {
        return None;
    }
    let legacy = env.home_dir().join(".maw").join("peers.json");
    (legacy != peer_store_path(env)).then_some(legacy)
}

#[cfg(test)]
mod parse_remote_identity_tests {
    use super::*;

    fn body(oracle: Option<&str>) -> ProbeRemoteIdentity {
        ProbeRemoteIdentity::Body {
            pubkey: Some("pk".to_owned()),
            oracle: oracle.map(str::to_owned),
            node: Some("black".to_owned()),
        }
    }

    #[test]
    fn present_oracle_is_stored_verbatim() {
        let parsed = parse_remote_identity(&body(Some("arra"))).expect("identity");
        assert_eq!(parsed.identity.expect("identity").oracle, "arra");
    }

    #[test]
    fn absent_oracle_stores_empty_never_a_fabricated_default() {
        // #677: a peer that does not report an oracle must NOT be labelled "mawjs" (or any
        // constant). Empty renders as "-"/"—"; the masquerade would make an unknown look probed.
        // Reverting parse_remote_identity to `.unwrap_or("mawjs")` MUST turn this test red.
        let parsed = parse_remote_identity(&body(None)).expect("identity");
        let oracle = parsed.identity.expect("identity").oracle;
        assert_eq!(oracle, "");
        assert_ne!(oracle, "mawjs");
    }

    #[test]
    fn empty_oracle_string_is_treated_as_absent() {
        // The empty case is exactly what /api/identity now emits: the field is omitted →
        // deserializes to None → stored as "". Confirm it never resurrects a default.
        let parsed = parse_remote_identity(&body(Some(""))).expect("identity");
        assert_eq!(parsed.identity.expect("identity").oracle, "");
    }
}
