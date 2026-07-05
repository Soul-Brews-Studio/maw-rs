
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

