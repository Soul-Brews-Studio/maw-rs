fn peer_add_mismatch_result(
    plan: &PeerAddPlan,
    existing: Option<&PeerRecord>,
    cached: &str,
    observed: &str,
) -> PeerAddResult {
    PeerAddResult {
        alias: plan.alias.clone(),
        overwrote: existing.is_some(),
        peer: existing
            .cloned()
            .unwrap_or_else(|| peer_add_new_record(plan)),
        probe_error: plan.probe.error.clone(),
        pubkey_mismatch: Some(PeerPubkeyMismatchError::new(
            plan.alias.clone(),
            cached,
            observed,
        )),
        peers_after: plan.peers.clone(),
    }
}

/// Deterministic input for maw-js `cmdProbe` peer-cache behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerProbePlan {
    pub alias: String,
    pub now: String,
    pub peers: BTreeMap<String, PeerRecord>,
    pub probe: ProbePeerResult,
    pub remove_before_mutate: bool,
}

/// Deterministic result for maw-js `cmdProbe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerProbeResult {
    pub alias: String,
    pub url: String,
    pub node: Option<String>,
    pub ok: bool,
    pub error: Option<ProbeLastError>,
    pub pubkey_mismatch: Option<PeerPubkeyMismatchError>,
    pub peers_after: BTreeMap<String, PeerRecord>,
}

/// Port of maw-js `cmdProbe` cache/TOFU behavior over deterministic inputs.
///
/// # Errors
///
/// Returns when the alias is not present in the input peer store.
pub fn cmd_peer_probe_from_plan(plan: &PeerProbePlan) -> Result<PeerProbeResult, String> {
    let Some(existing) = plan.peers.get(&plan.alias) else {
        return Err(format!("peer \"{}\" not found", plan.alias));
    };

    let tofu_decision =
        evaluate_peer_identity(&plan.alias, Some(existing), plan.probe.pubkey.as_deref());
    if tofu_decision.kind == TofuDecisionKind::Mismatch {
        return Ok(PeerProbeResult {
            alias: plan.alias.clone(),
            url: existing.url.clone(),
            node: plan.probe.node.clone().or_else(|| existing.node.clone()),
            ok: false,
            error: plan.probe.error.clone(),
            pubkey_mismatch: Some(PeerPubkeyMismatchError::new(
                plan.alias.clone(),
                tofu_decision.cached.unwrap_or_default(),
                tofu_decision.observed.unwrap_or_default(),
            )),
            peers_after: plan.peers.clone(),
        });
    }

    let mut peers_after = plan.peers.clone();
    if plan.remove_before_mutate {
        peers_after.remove(&plan.alias);
    }
    if let Some(peer) = peers_after.get_mut(&plan.alias) {
        if let Some(error) = &plan.probe.error {
            peer.last_error = Some(error.clone());
        } else {
            peer.last_error = None;
            peer.last_seen = Some(plan.now.clone());
            if let Some(node) = &plan.probe.node {
                peer.node = Some(node.clone());
            }
            if let Some(nickname) = &plan.probe.nickname {
                peer.nickname = Some(nickname.clone());
            }
            if let Some(identity) = &plan.probe.identity {
                peer.identity = Some(identity.clone());
            }
        }
        if tofu_decision.kind == TofuDecisionKind::TofuBootstrap
            && peer.pubkey.as_deref().is_none_or(str::is_empty)
        {
            peer.pubkey.clone_from(&tofu_decision.observed);
            peer.pubkey_first_seen = Some(plan.now.clone());
        }
    }

    Ok(PeerProbeResult {
        alias: plan.alias.clone(),
        url: existing.url.clone(),
        node: plan.probe.node.clone().or_else(|| existing.node.clone()),
        ok: plan.probe.error.is_none(),
        error: plan.probe.error.clone(),
        pubkey_mismatch: None,
        peers_after,
    })
}

fn read_peer_store_unlocked(path: &Path) -> PeerStoreFile {
    if !path.exists() {
        return empty_peer_store();
    }
    let Ok(raw) = fs::read_to_string(path) else {
        return empty_peer_store();
    };
    parse_peer_store(&raw).unwrap_or_else(|_| empty_peer_store())
}

fn parse_peer_store(raw: &str) -> Result<PeerStoreFile, String> {
    let value = serde_json::from_str::<serde_json::Value>(raw).map_err(|err| err.to_string())?;
    let peers = match value.get("peers") {
        Some(peers) if peers.is_object() => peers.clone(),
        Some(_) => {
            return Err("invalid store shape (expected { peers: { ... } } object)".to_owned());
        }
        None => serde_json::json!({}),
    };
    serde_json::from_value(serde_json::json!({ "version": 1, "peers": peers }))
        .map_err(|err| err.to_string())
}

fn write_peer_store_atomic(path: &Path, data: &PeerStoreFile) -> io::Result<()> {
    let tmp = tmp_peer_store_path(path);
    let json = serde_json::to_string_pretty(data).map_err(io::Error::other)?;
    fs::write(&tmp, format!("{json}\n"))?;
    fs::rename(tmp, path)
}

fn tmp_peer_store_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.tmp", path.display()))
}

fn corrupt_peer_store_path(path: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    PathBuf::from(format!("{}.corrupt-{stamp}", path.display()))
}

fn prefix16(value: &str) -> &str {
    value.get(..16).unwrap_or(value)
}

/// Deterministic input for maw-js `cmdProbeAll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeAllPlan {
    pub timeout_ms: u64,
    pub now: String,
    pub peers: Vec<(String, PeerRecord)>,
    /// URL → probe result → elapsed milliseconds.
    pub probe_results: Vec<(String, ProbePeerResult, u64)>,
    /// Aliases removed after load and before mutation.
    pub removed_before_mutate: Vec<String>,
}

/// Renderable per-peer probe-all row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeAllRow {
    pub alias: String,
    pub url: String,
    pub node: Option<String>,
    pub last_seen: Option<String>,
    pub ok: bool,
    pub ms: u64,
    pub error: Option<ProbeLastError>,
}

/// Deterministic result for maw-js `cmdProbeAll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeAllResult {
    pub rows: Vec<ProbeAllRow>,
    pub ok_count: usize,
    pub fail_count: usize,
    pub worst_exit_code: i32,
    pub probe_calls: Vec<(String, u64)>,
    pub mutate_calls: usize,
    pub peers_after: BTreeMap<String, PeerRecord>,
}
