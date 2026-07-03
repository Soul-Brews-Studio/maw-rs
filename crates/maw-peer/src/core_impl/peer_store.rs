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
        oracle: oracle
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("mawjs")
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
    }
}

/// Peer store record subset used by maw-js `probe-all`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerRecord {
    pub url: String,
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

fn readable_peer_store_path(env: &PeerStoreEnv) -> PathBuf {
    let primary = peer_store_path(env);
    if primary.exists() {
        return primary;
    }
    legacy_peer_store_path(env)
        .filter(|path| path.exists())
        .unwrap_or(primary)
}

/// Load peers with stale tmp cleanup and corruption quarantine.
#[must_use]
pub fn load_peer_store(env: &PeerStoreEnv) -> PeerStoreFile {
    clear_stale_peer_store_tmp(env);
    let path = readable_peer_store_path(env);
    if !path.exists() {
        return empty_peer_store();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return empty_peer_store();
    };
    match parse_peer_store(&raw) {
        Ok(store) => store,
        Err(error) => {
            let aside = corrupt_peer_store_path(&path);
            let _ = fs::rename(&path, aside);
            eprintln!(
                "\u{1b}[33m⚠\u{1b}[0m peers store at {} failed to parse ({error}); moved aside",
                path.display()
            );
            empty_peer_store()
        }
    }
}

/// Save peers via temp-file then rename, mirroring maw-js writeAtomic.
///
/// # Errors
///
/// Returns directory creation, JSON serialization, write, or rename failures.
pub fn save_peer_store(env: &PeerStoreEnv, data: &PeerStoreFile) -> io::Result<()> {
    let path = peer_store_path(env);
    create_peer_store_parent_dir(&path)?;
    write_peer_store_atomic(&path, data)
}

/// Read-modify-write peers, re-reading current contents before mutation.
///
/// # Errors
///
/// Returns directory creation, JSON serialization, write, or rename failures.
pub fn mutate_peer_store(
    env: &PeerStoreEnv,
    mutate: impl FnOnce(&mut PeerStoreFile),
) -> io::Result<PeerStoreFile> {
    let path = peer_store_path(env);
    create_peer_store_parent_dir(&path)?;
    let read_path = if path.exists() {
        path.clone()
    } else {
        readable_peer_store_path(env)
    };
    let mut store = read_peer_store_unlocked(&read_path);
    mutate(&mut store);
    write_peer_store_atomic(&path, &store)?;
    Ok(store)
}

fn create_peer_store_parent_dir(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent),
        None => Ok(()),
    }
}

/// Best-effort stale `.tmp` cleanup for primary and legacy peer stores.
pub fn clear_stale_peer_store_tmp(env: &PeerStoreEnv) {
    for path in [Some(peer_store_path(env)), legacy_peer_store_path(env)]
        .into_iter()
        .flatten()
    {
        let _ = fs::remove_file(tmp_peer_store_path(&path));
    }
}

/// Enumerate stale peers from the peer store in stable alias order.
#[must_use]
pub fn stale_peers(env: &PeerStoreEnv, now_ms: u64) -> Vec<StalePeer> {
    let ttl_ms = parse_stale_ttl_ms(env.var("MAW_PEER_STALE_TTL_MS"));
    load_peer_store(env)
        .peers
        .into_iter()
        .filter(|(_, peer)| is_peer_stale(peer, ttl_ms, now_ms))
        .map(|(alias, peer)| {
            let age_ms = stale_age_ms(&peer, now_ms);
            StalePeer {
                alias,
                url: peer.url,
                age_ms,
            }
        })
        .collect()
}

/// Return the maw-js `peers:stale` doctor check shape.
#[must_use]
pub fn stale_peer_check(env: &PeerStoreEnv, now_ms: u64) -> PeerDoctorCheck {
    let stale = stale_peers(env, now_ms);
    if stale.is_empty() {
        return PeerDoctorCheck {
            name: "peers:stale".to_owned(),
            ok: true,
            message: "no stale peers".to_owned(),
        };
    }
    let days = parse_stale_ttl_ms(env.var("MAW_PEER_STALE_TTL_MS")) / 86_400_000;
    PeerDoctorCheck {
        name: "peers:stale".to_owned(),
        ok: false,
        message: format!(
            "{} stale peer{} (>{days}d) — run 'maw doctor --fix-stale' to remove",
            stale.len(),
            if stale.len() == 1 { "" } else { "s" }
        ),
    }
}

/// Remove stale peers through the peer-store mutation path.
///
/// # Errors
///
/// Returns peer-store mutation write failures.
pub fn remove_stale_peers(env: &PeerStoreEnv, now_ms: u64) -> io::Result<PeerDoctorCheck> {
    let stale = stale_peers(env, now_ms);
    if stale.is_empty() {
        return Ok(PeerDoctorCheck {
            name: "peers:fix-stale".to_owned(),
            ok: true,
            message: "no stale peers".to_owned(),
        });
    }
    let mut removed = 0;
    mutate_peer_store(env, |data| {
        for stale_peer in &stale {
            removed += usize::from(data.peers.remove(&stale_peer.alias).is_some());
        }
    })?;
    Ok(PeerDoctorCheck {
        name: "peers:fix-stale".to_owned(),
        ok: true,
        message: format!(
            "removed {removed} stale peer{}",
            if removed == 1 { "" } else { "s" }
        ),
    })
}

#[must_use]
pub fn evaluate_peer_identity(
    alias: &str,
    peer: Option<&PeerRecord>,
    observed: Option<&str>,
) -> TofuDecision {
    let cached = peer
        .and_then(|peer| peer.pubkey.as_deref())
        .filter(|value| !value.is_empty());
    let observed = observed.filter(|value| !value.is_empty());

    let alias_string = alias.to_owned();
    let cached_string = cached.map(str::to_owned);
    let observed_string = observed.map(str::to_owned);

    match (cached, observed) {
        (None, Some(_)) => TofuDecision {
            kind: TofuDecisionKind::TofuBootstrap,
            alias: alias_string,
            cached: None,
            observed: observed_string,
            message: format!("[tofu] caching pubkey for {alias} (first sight)"),
        },
        (None, None) => TofuDecision {
            kind: TofuDecisionKind::LegacyFirstContact,
            alias: alias_string,
            cached: None,
            observed: None,
            message: format!("[tofu] {alias} did not advertise a pubkey (legacy peer; no pin established)"),
        },
        (Some(cached), None) => TofuDecision {
            kind: TofuDecisionKind::LegacyAfterPinned,
            alias: alias_string,
            cached: cached_string,
            observed: None,
            message: format!(
                "[tofu] {alias} previously advertised pubkey {}… but this response omits it; accepting during alpha migration, will hard-fail at v27",
                prefix16(cached)
            ),
        },
        (Some(cached), Some(observed)) if cached == observed => TofuDecision {
            kind: TofuDecisionKind::Match,
            alias: alias_string,
            cached: cached_string,
            observed: observed_string,
            message: format!("[tofu] {alias} pubkey verified"),
        },
        (Some(cached), Some(observed)) => TofuDecision {
            kind: TofuDecisionKind::Mismatch,
            alias: alias_string,
            cached: cached_string,
            observed: observed_string,
            message: PeerPubkeyMismatchError::new(alias, cached, observed).to_string(),
        },
    }
}

/// Persist a TOFU decision.
///
/// # Errors
///
/// Returns a structured mismatch error when the cached and observed pubkeys differ,
/// or an IO error if the bootstrap mutation cannot be written.
pub fn apply_tofu_decision(
    env: &PeerStoreEnv,
    decision: &TofuDecision,
    now: &str,
) -> Result<(), TofuApplyError> {
    match decision.kind {
        TofuDecisionKind::TofuBootstrap => {
            mutate_peer_store(env, |data| {
                let Some(peer) = data.peers.get_mut(&decision.alias) else {
                    return;
                };
                if peer
                    .pubkey
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                {
                    return;
                }
                peer.pubkey.clone_from(&decision.observed);
                peer.pubkey_first_seen = Some(now.to_owned());
            })?;
            Ok(())
        }
        TofuDecisionKind::Mismatch => Err(PeerPubkeyMismatchError::new(
            decision.alias.clone(),
            decision.cached.clone().unwrap_or_default(),
            decision.observed.clone().unwrap_or_default(),
        )
        .into()),
        TofuDecisionKind::Match
        | TofuDecisionKind::LegacyFirstContact
        | TofuDecisionKind::LegacyAfterPinned => Ok(()),
    }
}

