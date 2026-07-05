fn push_stale_routes<S>(
    diff: &mut SyncDiff,
    local_agents: &HashMap<String, String, S>,
    local_node: &str,
    live_by_node: &HashMap<String, Vec<String>>,
) where
    S: BuildHasher,
{
    for (oracle, node) in local_agents {
        if node == "local" || node == local_node {
            continue;
        }
        if live_by_node
            .get(node)
            .is_some_and(|live| !live.contains(oracle))
        {
            diff.stale.push(StaleRoute {
                oracle: oracle.clone(),
                peer_node: node.clone(),
            });
        }
    }
}

/// Apply a federation sync diff to an agents map. Conflicts require `force`; stale requires `prune`.
#[must_use]
pub fn apply_sync_diff<S>(
    current_agents: &HashMap<String, String, S>,
    diff: &SyncDiff,
    opts: SyncApplyOptions,
) -> SyncApplyResult
where
    S: BuildHasher,
{
    let mut agents = current_agents
        .iter()
        .map(|(oracle, node)| (oracle.clone(), node.clone()))
        .collect::<HashMap<_, _>>();
    let mut applied = Vec::new();

    for add in &diff.add {
        agents.insert(add.oracle.clone(), add.peer_node.clone());
        applied.push(format!(
            "+ agents['{}'] = '{}'  (from peer '{}')",
            add.oracle, add.peer_node, add.from_peer
        ));
    }
    if opts.force {
        for conflict in &diff.conflict {
            agents.insert(conflict.oracle.clone(), conflict.proposed.clone());
            applied.push(format!(
                "~ agents['{}']: '{}' → '{}'  (from peer '{}', --force)",
                conflict.oracle, conflict.current, conflict.proposed, conflict.from_peer
            ));
        }
    }
    if opts.prune {
        for stale in &diff.stale {
            agents.remove(&stale.oracle);
            applied.push(format!(
                "- agents['{}']  (was '{}', no longer hosted there)",
                stale.oracle, stale.peer_node
            ));
        }
    }

    SyncApplyResult { agents, applied }
}
