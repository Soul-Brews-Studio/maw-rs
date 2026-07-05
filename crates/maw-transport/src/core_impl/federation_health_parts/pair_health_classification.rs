fn classify_ok_peer_view(
    base: &FederationStatus,
    local_node: &str,
    peer: &FederationPeerStatus,
    peer_peers: &[FederationPeerView],
) -> PairStatus {
    let local = peer_peers
        .iter()
        .find(|candidate| matches_local_peer(candidate, local_node, &base.local_url));

    let Some(local) = local else {
        return pair_status(
            peer,
            PairHealth::HalfUp,
            true,
            Some(false),
            Some("local node not in peer's peer list"),
        );
    };

    if local.reachable == Some(false) {
        return pair_status(
            peer,
            PairHealth::HalfUp,
            true,
            Some(false),
            Some("peer's view of local is unreachable"),
        );
    }

    pair_status(peer, PairHealth::Healthy, true, Some(true), None::<String>)
}

fn matches_local_peer(candidate: &FederationPeerView, local_node: &str, local_url: &str) -> bool {
    if candidate
        .node
        .as_deref()
        .is_some_and(|node| !local_node.is_empty() && node == local_node)
    {
        return true;
    }
    candidate.url.as_deref() == Some(local_url)
}

fn remote_status_for<'a>(
    remote_statuses: &'a [(String, PeerFederationStatusResult)],
    url: &str,
) -> Option<&'a PeerFederationStatusResult> {
    remote_statuses
        .iter()
        .find_map(|(peer_url, status)| (peer_url == url).then_some(status))
}

fn pair_status(
    peer: &FederationPeerStatus,
    pair: PairHealth,
    forward: bool,
    reverse: Option<bool>,
    reason: Option<impl Into<String>>,
) -> PairStatus {
    PairStatus {
        url: peer.url.clone(),
        node: peer.node.clone(),
        pair,
        forward,
        reverse,
        reason: reason.map(Into::into),
        latency: peer.latency,
        agents: peer.agents.clone(),
        clock_warning: peer.clock_warning,
    }
}
