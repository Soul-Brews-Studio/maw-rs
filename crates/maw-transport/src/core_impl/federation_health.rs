/// Portable HTTP federation fallback transport.
pub struct HttpFederationTransport<Io> {
    config: HttpTransportConfig,
    io: Io,
    connected: bool,
    message_handlers: usize,
    presence_handlers: usize,
    feed_handlers: usize,
}

impl<Io> HttpFederationTransport<Io> {
    #[must_use]
    pub const fn new(config: HttpTransportConfig, io: Io) -> Self {
        Self {
            config,
            io,
            connected: false,
            message_handlers: 0,
            presence_handlers: 0,
            feed_handlers: 0,
        }
    }

    #[must_use]
    pub const fn connected(&self) -> bool {
        self.connected
    }

    pub fn connect(&mut self) {
        self.connected = !self.config.peers.is_empty();
    }

    pub const fn disconnect(&mut self) {
        self.connected = false;
    }

    pub const fn on_message(&mut self) {
        self.message_handlers += 1;
    }

    pub const fn on_presence(&mut self) {
        self.presence_handlers += 1;
    }

    pub const fn on_feed(&mut self) {
        self.feed_handlers += 1;
    }

    #[must_use]
    pub const fn handler_counts(&self) -> (usize, usize, usize) {
        (
            self.message_handlers,
            self.presence_handlers,
            self.feed_handlers,
        )
    }

    pub const fn publish_presence(&self) {}

    #[must_use]
    pub const fn io(&self) -> &Io {
        &self.io
    }
}

impl<Io> HttpFederationTransport<Io>
where
    Io: HttpTransportIo,
{
    #[must_use]
    pub fn name(&self) -> &'static str {
        "http-federation"
    }

    #[must_use]
    pub fn can_reach(&self, target: &TransportTarget) -> bool {
        !self.config.peers.is_empty() && !is_local_host(target.host.as_deref())
    }

    /// Send to the first remote sourced session whose window name contains the oracle query.
    pub fn send(&mut self, target: &TransportTarget, message: &str) -> bool {
        let Ok(local_sessions) = self.io.list_local_sessions() else {
            return false;
        };
        let Ok(all_sessions) = self.io.get_all_sessions(&local_sessions) else {
            return false;
        };
        let query = target.oracle.to_lowercase();
        for session in &all_sessions {
            let Some(source) = &session.source else {
                continue;
            };
            if source == "local" {
                continue;
            }
            let matches = session
                .windows
                .iter()
                .any(|window| window.name.to_lowercase().contains(&query));
            if !matches {
                continue;
            }
            let single = [session.clone()];
            let Some(tmux_target) = self.io.find_target_window(&single, &target.oracle) else {
                continue;
            };
            return self
                .io
                .send_peer_keys(source, &tmux_target, message)
                .unwrap_or(false);
        }
        false
    }

    /// Publish a feed event to every configured peer and return warnings for rejected posts.
    pub fn publish_feed(&mut self, event_json: &str) -> Vec<HttpFeedWarning> {
        let peers = self.config.peers.clone();
        let timeout = self.io.timeout_for("http");
        let mut warnings = Vec::new();
        for peer in peers {
            let url = format!("{peer}/api/feed");
            if let Err(reason) = self.io.post_peer_feed(&url, "POST", event_json, timeout) {
                warnings.push(HttpFeedWarning { peer, reason });
            }
        }
        warnings
    }
}

fn is_local_host(host: Option<&str>) -> bool {
    matches!(host, None | Some("local" | "localhost"))
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn rate_limit_like(msg: &str) -> bool {
    msg.contains("rate") && msg.contains("limit")
}

#[must_use]
pub fn classify_symmetric_federation_status(
    base: &FederationStatus,
    remote_statuses: &[(String, PeerFederationStatusResult)],
    local_node: &str,
) -> SymmetricFederationStatus {
    let pairs = base
        .peers
        .iter()
        .map(|peer| classify_peer_pair(base, remote_statuses, local_node, peer))
        .collect::<Vec<_>>();
    let healthy_pairs = pairs
        .iter()
        .filter(|pair| pair.pair == PairHealth::Healthy)
        .count();
    let total_pairs = pairs.len();

    SymmetricFederationStatus {
        local_url: base.local_url.clone(),
        local_node: local_node.to_owned(),
        pairs,
        healthy_pairs,
        total_pairs,
    }
}

fn classify_peer_pair(
    base: &FederationStatus,
    remote_statuses: &[(String, PeerFederationStatusResult)],
    local_node: &str,
    peer: &FederationPeerStatus,
) -> PairStatus {
    if !peer.reachable {
        return pair_status(
            peer,
            PairHealth::Down,
            false,
            None,
            Some("forward unreachable"),
        );
    }

    match remote_status_for(remote_statuses, &peer.url) {
        Some(PeerFederationStatusResult::Ok(status)) => {
            classify_ok_peer_view(base, local_node, peer, &status.peers)
        }
        Some(PeerFederationStatusResult::MissingPeers) => {
            classify_ok_peer_view(base, local_node, peer, &[])
        }
        Some(PeerFederationStatusResult::HttpStatus(status)) => pair_status(
            peer,
            PairHealth::Unknown,
            true,
            None,
            Some(format!("peer /api/federation/status returned {status}")),
        ),
        Some(PeerFederationStatusResult::FetchError(error)) => pair_status(
            peer,
            PairHealth::Unknown,
            true,
            None,
            Some(format!("peer status fetch failed: {error}")),
        ),
        None => pair_status(
            peer,
            PairHealth::Unknown,
            true,
            None,
            Some("peer /api/federation/status returned 0"),
        ),
    }
}

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

#[cfg(test)]
mod federation_symmetric_tests {
    use super::*;

    fn base(peers: Vec<FederationPeerStatus>) -> FederationStatus {
        FederationStatus {
            local_url: "http://localhost:3456".to_owned(),
            peers,
        }
    }

    fn peer(url: &str, reachable: bool, node: Option<&str>) -> FederationPeerStatus {
        FederationPeerStatus {
            url: url.to_owned(),
            node: node.map(str::to_owned),
            reachable,
            latency: Some(40),
            agents: Vec::new(),
            clock_warning: false,
        }
    }

    fn remote(peers: Vec<FederationPeerView>) -> PeerFederationStatusResult {
        PeerFederationStatusResult::Ok(PeerFederationStatus { peers })
    }

    fn view(url: &str, node: Option<&str>, reachable: bool) -> FederationPeerView {
        FederationPeerView {
            url: Some(url.to_owned()),
            node: node.map(str::to_owned),
            reachable: Some(reachable),
        }
    }

    #[test]
    fn no_peers_reports_empty_pair_counts() {
        let status = classify_symmetric_federation_status(&base(Vec::new()), &[], "white");

        assert!(status.pairs.is_empty());
        assert_eq!(status.total_pairs, 0);
        assert_eq!(status.healthy_pairs, 0);
        assert_eq!(status.local_node, "white");
    }

    #[test]
    fn reachable_peer_with_reachable_reverse_view_is_healthy() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                remote(vec![view("http://localhost:3456", Some("white"), true)]),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Healthy);
        assert!(status.pairs[0].forward);
        assert_eq!(status.pairs[0].reverse, Some(true));
        assert_eq!(status.healthy_pairs, 1);
    }

    #[test]
    fn reachable_peer_missing_local_view_is_half_up() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[("http://mba:3456".to_owned(), remote(Vec::new()))],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::HalfUp);
        assert!(status.pairs[0].forward);
        assert_eq!(status.pairs[0].reverse, Some(false));
        assert!(status.pairs[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("not in peer")));
    }

    #[test]
    fn reachable_peer_marking_local_unreachable_is_half_up() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                remote(vec![view("http://localhost:3456", Some("white"), false)]),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::HalfUp);
        assert!(status.pairs[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("unreachable")));
    }

    #[test]
    fn forward_unreachable_peer_is_down_without_reverse_check() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", false, None)]),
            &[],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Down);
        assert!(!status.pairs[0].forward);
        assert_eq!(status.pairs[0].reverse, None);
        assert_eq!(
            status.pairs[0].reason.as_deref(),
            Some("forward unreachable")
        );
    }

    #[test]
    fn non_ok_peer_status_is_unknown() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                PeerFederationStatusResult::HttpStatus(500),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Unknown);
        assert!(status.pairs[0].forward);
        assert_eq!(status.pairs[0].reverse, None);
        assert!(status.pairs[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("returned 500")));
    }

    #[test]
    fn fetch_error_peer_status_is_unknown_with_reason() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                PeerFederationStatusResult::FetchError("network cable unplugged".to_owned()),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Unknown);
        assert!(status.pairs[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("network cable")));
    }

    #[test]
    fn local_node_identity_match_wins_when_url_differs() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                remote(vec![view("http://10.0.0.1:3456", Some("white"), true)]),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Healthy);
    }

    #[test]
    fn local_url_match_supports_legacy_peer_without_node_identity() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, None)]),
            &[(
                "http://mba:3456".to_owned(),
                remote(vec![view("http://localhost:3456", None, true)]),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Healthy);
    }

    #[test]
    fn mixed_three_peer_mesh_counts_one_healthy_one_half_up_one_down() {
        let status = classify_symmetric_federation_status(
            &base(vec![
                peer("http://alpha:3456", true, Some("alpha")),
                peer("http://bravo:3456", true, Some("bravo")),
                peer("http://charlie:3456", false, None),
            ]),
            &[
                (
                    "http://alpha:3456".to_owned(),
                    remote(vec![view("http://localhost:3456", Some("white"), true)]),
                ),
                ("http://bravo:3456".to_owned(), remote(Vec::new())),
            ],
            "white",
        );
        let mut pair_healths = status
            .pairs
            .iter()
            .map(|pair| pair.pair)
            .collect::<Vec<_>>();
        pair_healths.sort_unstable_by_key(|state| state.as_str());

        assert_eq!(status.total_pairs, 3);
        assert_eq!(status.healthy_pairs, 1);
        assert_eq!(
            pair_healths,
            vec![PairHealth::Down, PairHealth::HalfUp, PairHealth::Healthy]
        );
    }

    #[test]
    fn peer_response_without_peers_field_is_half_up_defensively() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                PeerFederationStatusResult::MissingPeers,
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::HalfUp);
    }
}
