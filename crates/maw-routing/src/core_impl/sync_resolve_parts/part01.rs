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
    pub kind: Option<RepoKind>,
}

/// Tmux session metadata. `source` is `None`/`local` for writable local sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub windows: Vec<Window>,
    pub source: Option<String>,
}

/// Declared repository kind for a routeable window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoKind {
    Oracle,
    Project,
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

