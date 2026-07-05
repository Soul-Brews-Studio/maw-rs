/// Evaluate and persist a peer identity TOFU decision.
///
/// # Errors
///
/// Returns mismatch or peer-store IO failures from [`apply_tofu_decision`].
pub fn tofu_record_peer_identity(
    env: &PeerStoreEnv,
    alias: &str,
    peer: Option<&PeerRecord>,
    observed: Option<&str>,
    now: &str,
) -> Result<TofuDecision, TofuApplyError> {
    let decision = evaluate_peer_identity(alias, peer, observed);
    apply_tofu_decision(env, &decision, now)?;
    Ok(decision)
}

/// Clear a cached pubkey for `alias`.
///
/// # Errors
///
/// Returns peer-store mutation write failures.
pub fn forget_peer_pubkey(env: &PeerStoreEnv, alias: &str) -> io::Result<&'static str> {
    let mut outcome = "not-found";
    mutate_peer_store(env, |data| {
        let Some(peer) = data.peers.get_mut(alias) else {
            outcome = "not-found";
            return;
        };
        if peer.pubkey.is_none() {
            outcome = "no-pubkey";
            return;
        }
        peer.pubkey = None;
        peer.pubkey_first_seen = None;
        outcome = "cleared";
    })?;
    Ok(outcome)
}

#[derive(Debug)]
pub enum TofuApplyError {
    Io(io::Error),
    Mismatch(PeerPubkeyMismatchError),
}

impl std::fmt::Display for TofuApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Mismatch(error) => error.fmt(f),
        }
    }
}

impl Error for TofuApplyError {}

impl From<io::Error> for TofuApplyError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<PeerPubkeyMismatchError> for TofuApplyError {
    fn from(value: PeerPubkeyMismatchError) -> Self {
        Self::Mismatch(value)
    }
}

/// Deterministic input for maw-js `cmdAdd` peer-cache behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAddPlan {
    pub alias: String,
    pub url: String,
    pub node: Option<String>,
    pub authenticated_pubkey: Option<String>,
    pub authenticated_identity: Option<PeerIdentity>,
    pub mark_symmetric_check: bool,
    pub one_way: Option<bool>,
    pub now: String,
    pub peers: BTreeMap<String, PeerRecord>,
    pub probe: ProbePeerResult,
}

/// Deterministic result for maw-js `cmdAdd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAddResult {
    pub alias: String,
    pub overwrote: bool,
    pub peer: PeerRecord,
    pub probe_error: Option<ProbeLastError>,
    pub pubkey_mismatch: Option<PeerPubkeyMismatchError>,
    pub peers_after: BTreeMap<String, PeerRecord>,
}

/// Port of maw-js `cmdAdd` cache/TOFU behavior over deterministic inputs.
///
/// # Errors
///
/// Returns maw-js-compatible alias or URL validation failures.
pub fn cmd_peer_add_from_plan(plan: &PeerAddPlan) -> Result<PeerAddResult, String> {
    if let Some(message) = validate_peer_alias(&plan.alias) {
        return Err(message);
    }
    if let Some(message) = validate_peer_url(&plan.url) {
        return Err(message);
    }

    let observed_pubkey = plan
        .authenticated_pubkey
        .as_deref()
        .or(plan.probe.pubkey.as_deref());
    let existing = plan.peers.get(&plan.alias);
    if let (Some(authenticated), Some(probed)) = (
        plan.authenticated_pubkey.as_deref(),
        plan.probe.pubkey.as_deref(),
    ) {
        if authenticated != probed {
            return Ok(peer_add_mismatch_result(
                plan,
                existing,
                authenticated,
                probed,
            ));
        }
    }
    let tofu_decision = evaluate_peer_identity(&plan.alias, existing, observed_pubkey);
    if tofu_decision.kind == TofuDecisionKind::Mismatch {
        let cached = tofu_decision.cached.unwrap_or_default();
        let observed = tofu_decision.observed.unwrap_or_default();
        return Ok(peer_add_mismatch_result(plan, existing, &cached, &observed));
    }

    let mut peer = peer_add_new_record(plan);
    if let Some(existing) = existing {
        peer_add_apply_existing(plan, existing, &tofu_decision, &mut peer);
    } else if tofu_decision.kind == TofuDecisionKind::TofuBootstrap {
        peer.pubkey.clone_from(&tofu_decision.observed);
        peer.pubkey_first_seen = Some(plan.now.clone());
    }
    if existing.is_none() && plan.mark_symmetric_check {
        peer.last_symmetric_check = Some(plan.now.clone());
        peer.one_way = Some(plan.one_way.unwrap_or(plan.probe.error.is_some()));
    }

    let overwrote = plan.peers.contains_key(&plan.alias);
    let mut peers_after = plan.peers.clone();
    peers_after.insert(plan.alias.clone(), peer.clone());

    Ok(PeerAddResult {
        alias: plan.alias.clone(),
        overwrote,
        peer,
        probe_error: plan.probe.error.clone(),
        pubkey_mismatch: None,
        peers_after,
    })
}

fn peer_add_new_record(plan: &PeerAddPlan) -> PeerRecord {
    PeerRecord {
        url: plan.url.clone(),
        node: plan.node.clone().or_else(|| plan.probe.node.clone()),
        added_at: plan.now.clone(),
        last_seen: plan.probe.error.is_none().then(|| plan.now.clone()),
        last_error: plan.probe.error.clone(),
        nickname: plan.probe.nickname.clone(),
        pubkey: None,
        pubkey_first_seen: None,
        identity: plan
            .probe
            .identity
            .clone()
            .or_else(|| plan.authenticated_identity.clone()),
        one_way: None,
        last_symmetric_check: None,
    }
}

fn peer_add_apply_existing(
    plan: &PeerAddPlan,
    existing: &PeerRecord,
    tofu_decision: &TofuDecision,
    peer: &mut PeerRecord,
) {
    if existing
        .pubkey
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        peer.pubkey.clone_from(&existing.pubkey);
        peer.pubkey_first_seen
            .clone_from(&existing.pubkey_first_seen);
    } else if tofu_decision.kind == TofuDecisionKind::TofuBootstrap {
        peer.pubkey.clone_from(&tofu_decision.observed);
        peer.pubkey_first_seen = Some(plan.now.clone());
    }
    if peer.identity.is_none() {
        peer.identity.clone_from(&existing.identity);
    }
    if plan.mark_symmetric_check {
        peer.last_symmetric_check = Some(plan.now.clone());
        peer.one_way = Some(plan.one_way.unwrap_or(plan.probe.error.is_some()));
    } else if existing.last_symmetric_check.is_some() {
        peer.last_symmetric_check
            .clone_from(&existing.last_symmetric_check);
        peer.one_way = existing.one_way;
    }
}

