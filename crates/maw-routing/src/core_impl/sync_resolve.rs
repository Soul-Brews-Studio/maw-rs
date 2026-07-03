// Portable target routing resolver.
//
// This crate mirrors the pure, sync behavior in maw-js `src/core/routing.ts`
// that is covered by `test/spec/routing.fixtures.json`.

use std::{collections::HashMap, hash::BuildHasher};

/// Tmux window metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub index: u32,
    pub name: String,
    pub active: bool,
}

/// Tmux session metadata. `source` is `None`/`local` for writable local sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub windows: Vec<Window>,
    pub source: Option<String>,
}

/// Named peer config entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedPeer {
    pub name: String,
    pub url: String,
}

/// Minimal config surface needed by the portable resolver.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MawConfig {
    pub node: Option<String>,
    pub named_peers: Vec<NamedPeer>,
    pub peers: Vec<String>,
    pub agents: HashMap<String, String>,
}

/// Identity advertised by a federation peer's `/api/identity` surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    pub peer_name: String,
    pub url: String,
    pub node: String,
    pub agents: Vec<String>,
    pub reachable: bool,
    pub error: Option<String>,
}

/// Oracles present on a reachable peer but missing locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncAdd {
    pub oracle: String,
    pub peer_node: String,
    pub from_peer: String,
}

/// Local route points at a reachable peer that no longer hosts the oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleRoute {
    pub oracle: String,
    pub peer_node: String,
}

/// Existing route conflicts with a reachable peer claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConflict {
    pub oracle: String,
    pub current: String,
    pub proposed: String,
    pub from_peer: String,
}

/// Peer identity fetch failed; sync keeps local routes intact for this peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreachablePeer {
    pub peer_name: String,
    pub url: String,
    pub error: Option<String>,
}

/// Pure federation-sync diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncDiff {
    pub add: Vec<SyncAdd>,
    pub stale: Vec<StaleRoute>,
    pub conflict: Vec<SyncConflict>,
    pub unreachable: Vec<UnreachablePeer>,
}

/// Options for applying a federation-sync diff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncApplyOptions {
    pub force: bool,
    pub prune: bool,
}

/// Pure result of applying a federation-sync diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncApplyResult {
    pub agents: HashMap<String, String>,
    pub applied: Vec<String>,
}

/// Return oracles hosted by this node. Both explicit node routes and `local` count.
#[must_use]
pub fn hosted_agents<S>(agents: &HashMap<String, String, S>, node: &str) -> Vec<String>
where
    S: BuildHasher,
{
    agents
        .iter()
        .filter(|(_, route)| route.as_str() == node || route.as_str() == "local")
        .map(|(oracle, _)| oracle.clone())
        .collect()
}

/// Compute federation sync changes without touching config or network.
#[must_use]
pub fn compute_sync_diff<S>(
    local_agents: &HashMap<String, String, S>,
    peer_identities: &[PeerIdentity],
    local_node: &str,
) -> SyncDiff
where
    S: BuildHasher,
{
    let mut diff = SyncDiff::default();
    let mut live_by_node = HashMap::<String, Vec<String>>::new();
    let mut peer_name_by_node = HashMap::<String, String>::new();

    for peer in peer_identities {
        if !peer.reachable {
            diff.unreachable.push(UnreachablePeer {
                peer_name: peer.peer_name.clone(),
                url: peer.url.clone(),
                error: peer.error.clone(),
            });
            continue;
        }
        live_by_node
            .entry(peer.node.clone())
            .or_insert_with(|| peer.agents.clone());
        peer_name_by_node
            .entry(peer.node.clone())
            .or_insert_with(|| peer.peer_name.clone());
    }

    push_sync_adds_and_conflicts(
        &mut diff,
        local_agents,
        peer_identities,
        local_node,
        &peer_name_by_node,
    );
    push_stale_routes(&mut diff, local_agents, local_node, &live_by_node);
    diff
}

fn push_sync_adds_and_conflicts<S>(
    diff: &mut SyncDiff,
    local_agents: &HashMap<String, String, S>,
    peer_identities: &[PeerIdentity],
    local_node: &str,
    peer_name_by_node: &HashMap<String, String>,
) where
    S: BuildHasher,
{
    let mut claimed_by_first = Vec::<String>::new();
    for peer in peer_identities {
        if !peer.reachable || peer_name_by_node.get(&peer.node) != Some(&peer.peer_name) {
            continue;
        }
        for oracle in &peer.agents {
            if claimed_by_first.contains(oracle) {
                continue;
            }
            claimed_by_first.push(oracle.clone());

            let Some(current) = local_agents.get(oracle) else {
                diff.add.push(SyncAdd {
                    oracle: oracle.clone(),
                    peer_node: peer.node.clone(),
                    from_peer: peer.peer_name.clone(),
                });
                continue;
            };
            if current == "local" || current == local_node || current == &peer.node {
                continue;
            }
            diff.conflict.push(SyncConflict {
                oracle: oracle.clone(),
                current: current.clone(),
                proposed: peer.node.clone(),
                from_peer: peer.peer_name.clone(),
            });
        }
    }
}

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

#[cfg(test)]
mod federation_sync_tests {
    use super::*;

    fn peer(peer_name: &str, node: &str, agents: &[&str], reachable: bool) -> PeerIdentity {
        PeerIdentity {
            peer_name: peer_name.to_owned(),
            url: format!("http://{peer_name}:3456"),
            node: node.to_owned(),
            agents: agents.iter().map(ToString::to_string).collect(),
            reachable,
            error: (!reachable).then(|| "stub".to_owned()),
        }
    }

    #[test]
    fn hosted_agents_includes_explicit_node_and_local_entries() {
        let mut agents = HashMap::new();
        agents.insert("pulse".to_owned(), "white".to_owned());
        agents.insert("mawjs".to_owned(), "white".to_owned());
        agents.insert("volt-colab-ml".to_owned(), "local".to_owned());
        agents.insert("homekeeper".to_owned(), "mba".to_owned());

        let mut hosted = hosted_agents(&agents, "white");
        hosted.sort();

        assert_eq!(hosted, ["mawjs", "pulse", "volt-colab-ml"]);
    }

    #[test]
    fn sync_diff_adds_new_oracles_but_preserves_local_routes() {
        let diff = compute_sync_diff(
            &HashMap::from([
                ("mawjs".to_owned(), "local".to_owned()),
                ("homekeeper".to_owned(), "mba".to_owned()),
            ]),
            &[peer(
                "white",
                "white",
                &["mawjs", "volt-colab-ml", "pulse"],
                true,
            )],
            "oracle-world",
        );
        let mut additions = diff
            .add
            .iter()
            .map(|add| add.oracle.clone())
            .collect::<Vec<_>>();
        additions.sort();

        assert_eq!(additions, ["pulse", "volt-colab-ml"]);
        assert!(diff.conflict.is_empty());
        assert!(diff.stale.is_empty());
        assert!(diff.unreachable.is_empty());
    }

    #[test]
    fn sync_diff_reports_conflict_when_foreign_route_claimed_elsewhere() {
        let diff = compute_sync_diff(
            &HashMap::from([("mawjs".to_owned(), "mba".to_owned())]),
            &[peer("white", "white", &["mawjs"], true)],
            "oracle-world",
        );

        assert_eq!(
            diff.conflict,
            vec![SyncConflict {
                oracle: "mawjs".to_owned(),
                current: "mba".to_owned(),
                proposed: "white".to_owned(),
                from_peer: "white".to_owned(),
            }]
        );
    }

    #[test]
    fn duplicate_oracle_claims_keep_first_peer_winner() {
        let diff = compute_sync_diff(
            &HashMap::new(),
            &[
                peer("white", "white", &["ghost"], true),
                peer("mba", "mba", &["ghost"], true),
            ],
            "oracle-world",
        );

        assert_eq!(diff.add.len(), 1);
        assert_eq!(diff.add[0].peer_node, "white");
        assert!(diff.conflict.is_empty());
    }

    #[test]
    fn stale_only_flags_reachable_peer_routes_and_skips_local() {
        let diff = compute_sync_diff(
            &HashMap::from([
                ("oldGuy".to_owned(), "white".to_owned()),
                ("localGuy".to_owned(), "oracle-world".to_owned()),
            ]),
            &[peer("white", "white", &["mawjs"], true)],
            "oracle-world",
        );

        assert_eq!(
            diff.stale,
            vec![StaleRoute {
                oracle: "oldGuy".to_owned(),
                peer_node: "white".to_owned(),
            }]
        );
    }

    #[test]
    fn unreachable_peers_are_tracked_but_not_marked_stale() {
        let diff = compute_sync_diff(
            &HashMap::from([("oldGuy".to_owned(), "mba".to_owned())]),
            &[peer("mba", "mba", &[], false)],
            "oracle-world",
        );

        assert!(diff.add.is_empty());
        assert!(diff.stale.is_empty());
        assert!(diff.conflict.is_empty());
        assert_eq!(diff.unreachable.len(), 1);
        assert_eq!(diff.unreachable[0].peer_name, "mba");
    }

    #[test]
    fn apply_sync_diff_adds_forces_and_prunes_when_requested() {
        let diff = SyncDiff {
            add: vec![SyncAdd {
                oracle: "pulse".to_owned(),
                peer_node: "white".to_owned(),
                from_peer: "white".to_owned(),
            }],
            conflict: vec![SyncConflict {
                oracle: "mawjs".to_owned(),
                current: "mba".to_owned(),
                proposed: "white".to_owned(),
                from_peer: "white".to_owned(),
            }],
            stale: vec![StaleRoute {
                oracle: "oldGuy".to_owned(),
                peer_node: "white".to_owned(),
            }],
            unreachable: Vec::new(),
        };

        let result = apply_sync_diff(
            &HashMap::from([
                ("mawjs".to_owned(), "mba".to_owned()),
                ("oldGuy".to_owned(), "white".to_owned()),
            ]),
            &diff,
            SyncApplyOptions {
                force: true,
                prune: true,
            },
        );

        assert_eq!(
            result.agents,
            HashMap::from([
                ("mawjs".to_owned(), "white".to_owned()),
                ("pulse".to_owned(), "white".to_owned()),
            ])
        );
        assert_eq!(result.applied.len(), 3);
    }

    #[test]
    fn duplicate_node_identity_keeps_first_peer_claims_only() {
        let diff = compute_sync_diff(
            &HashMap::new(),
            &[
                peer("white-a", "white", &["pulse"], true),
                peer("white-b", "white", &["ghost"], true),
            ],
            "oracle-world",
        );

        assert_eq!(diff.add.len(), 1);
        assert_eq!(diff.add[0].oracle, "pulse");
    }

}

/// Routing resolution result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult {
    Local {
        target: String,
    },
    Peer {
        peer_url: String,
        target: String,
        node: String,
    },
    SelfNode {
        target: String,
    },
    Error {
        reason: String,
        detail: String,
        hint: Option<String>,
    },
}

/// Resolve a user query to a local target, peer target, self-node target, or error.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn resolve_target(query: &str, config: &MawConfig, sessions: &[Session]) -> ResolveResult {
    if query.is_empty() {
        return error(
            "empty_query",
            "no target specified",
            Some("usage: maw hey <agent> <message>"),
        );
    }

    let writable: Vec<Session> = sessions
        .iter()
        .filter(|session| {
            !session.name.ends_with("-view")
                && session
                    .source
                    .as_deref()
                    .is_none_or(|source| source == "local")
        })
        .cloned()
        .collect();
    let self_node = config.node.as_deref().unwrap_or("local");

    if !query.contains(':') {
        if let Some(result) =
            resolve_session_alias_window_target(query, &writable, RouteType::Local)
        {
            return result;
        }
    }

    if let Some(local_target) = find_window(&writable, query) {
        return ResolveResult::Local {
            target: local_target,
        };
    }

    if query.contains(':') && !query.contains('/') {
        let (node_name, agent_name) = query.split_once(':').unwrap_or(("", ""));
        if node_name.is_empty() || agent_name.is_empty() {
            return error(
                "empty_node_or_agent",
                format!("invalid format: '{query}'"),
                Some("use node:agent format (e.g. mba:homekeeper)"),
            );
        }

        if node_name == self_node || node_name == "local" {
            if let Some(result) =
                resolve_session_alias_window_target(agent_name, &writable, RouteType::SelfNode)
            {
                return result;
            }
            if let Some(self_target) = find_window(&writable, agent_name) {
                return ResolveResult::SelfNode {
                    target: self_target,
                };
            }
            return error(
                "self_not_running",
                format!("'{agent_name}' not found in local sessions on {self_node}"),
                Some(format!("maw wake {agent_name}")),
            );
        }

        if let Some(peer_url) = find_peer_url(node_name, config) {
            return ResolveResult::Peer {
                peer_url,
                target: agent_name.to_owned(),
                node: node_name.to_owned(),
            };
        }

        return error(
            "unknown_node",
            format!("node '{node_name}' not in namedPeers or peers"),
            Some("add to maw.config.json namedPeers"),
        );
    }

    let stripped_query = query.strip_suffix("-oracle").unwrap_or(query);
    let agent_node = config
        .agents
        .get(query)
        .or_else(|| config.agents.get(stripped_query));

    if let Some(agent_node) = agent_node {
        if agent_node == self_node {
            return error(
                "self_not_running",
                format!("'{query}' mapped to {self_node} (local) but not found in sessions"),
                Some(format!("maw wake {query}")),
            );
        }
        if let Some(peer_url) = find_peer_url(agent_node, config) {
            return ResolveResult::Peer {
                peer_url,
                target: query.to_owned(),
                node: agent_node.clone(),
            };
        }
        return error(
            "no_peer_url",
            format!("'{query}' mapped to node '{agent_node}' but no URL found"),
            Some(format!("add {agent_node} to maw.config.json namedPeers")),
        );
    }

    error(
        "not_found",
        format!("'{query}' not in local sessions or agents map"),
        Some("check: maw ls"),
    )
}

