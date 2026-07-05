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

