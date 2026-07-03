#[cfg(test)]
mod tests {
include!("tests_impl/runner_parser_attach.rs");
include!("tests_impl/send_kill_resolver.rs");
include!("tests_impl/send_text_pending.rs");
include!("tests_impl/client_runner_edges.rs");
include!("tests_impl/tag_recovery_edges.rs");
}

/// Parsed `session:window.pane` tmux target parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPaneTargetParts {
    pub session: String,
    pub window: String,
    pub pane: String,
}

/// Live tmux pane projection used by discovery inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverLivePane {
    pub source: String,
    pub id: String,
    pub target: String,
    pub session: String,
    pub window: String,
    pub pane: String,
    pub command: Option<String>,
    pub title: Option<String>,
    pub pid: Option<u32>,
    pub cwd: Option<String>,
    pub last_activity: Option<u64>,
    pub awake: bool,
    pub matches: Vec<String>,
}

/// Result of pure live-state projection from already-listed tmux panes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxLiveStateResult {
    pub source: String,
    pub live: Vec<DiscoverLivePane>,
    pub warnings: Vec<String>,
}

/// Peer target decorated with tmux liveness metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTargetWithLive {
    pub name: Option<String>,
    pub url: String,
    pub source: maw_peer::PeerSourceKind,
    pub node: Option<String>,
    pub oracle: Option<String>,
    pub awake: bool,
    pub live_targets: Vec<String>,
    pub live_sessions: Vec<String>,
}

/// Parse a tmux pane target shaped like `session:window.pane`.
#[must_use]
pub fn parse_tmux_pane_target(target: &str) -> Option<TmuxPaneTargetParts> {
    let colon = target.find(':')?;
    let dot = target.rfind('.')?;
    if colon == 0 || dot <= colon + 1 || dot == target.len() - 1 {
        return None;
    }
    Some(TmuxPaneTargetParts {
        session: target[..colon].to_owned(),
        window: target[colon + 1..dot].to_owned(),
        pane: target[dot + 1..].to_owned(),
    })
}

/// Resolve live tmux state from already-collected panes and peer targets.
#[must_use]
pub fn resolve_tmux_live_state(
    peers: &[maw_peer::PeerTarget],
    panes: &[TmuxPane],
) -> TmuxLiveStateResult {
    let mut live = panes
        .iter()
        .map(|pane| tmux_pane_to_live_pane(pane, peers))
        .collect::<Vec<_>>();
    live.sort_by(|left, right| left.target.cmp(&right.target));
    TmuxLiveStateResult {
        source: "tmux".to_owned(),
        live,
        warnings: Vec::new(),
    }
}

/// Mark peer targets awake when their configured signals match live tmux panes.
#[must_use]
pub fn mark_peer_targets_live(
    peers: &[maw_peer::PeerTarget],
    live: &[DiscoverLivePane],
) -> Vec<PeerTargetWithLive> {
    peers
        .iter()
        .map(|peer| {
            let peer_signals = normalized_peer_signals(peer);
            let matching = live
                .iter()
                .filter(|pane| {
                    pane_signals(pane)
                        .iter()
                        .any(|signal| peer_signals.iter().any(|peer_signal| peer_signal == signal))
                })
                .collect::<Vec<_>>();
            PeerTargetWithLive {
                name: peer.name.clone(),
                url: peer.url.clone(),
                source: peer.source,
                node: peer.node.clone(),
                oracle: peer.oracle.clone(),
                awake: !matching.is_empty(),
                live_targets: matching.iter().map(|pane| pane.target.clone()).collect(),
                live_sessions: unique_preserve_order(
                    matching.iter().map(|pane| pane.session.clone()).collect(),
                ),
            }
        })
        .collect()
}

fn tmux_pane_to_live_pane(pane: &TmuxPane, peers: &[maw_peer::PeerTarget]) -> DiscoverLivePane {
    let parsed =
        parse_tmux_pane_target(&pane.target).unwrap_or_else(|| fallback_target_parts(&pane.target));
    let mut live = DiscoverLivePane {
        source: "tmux".to_owned(),
        id: pane.id.clone(),
        target: pane.target.clone(),
        session: parsed.session,
        window: parsed.window,
        pane: parsed.pane,
        command: empty_to_none(&pane.command),
        title: empty_to_none(&pane.title),
        pid: pane.pid,
        cwd: pane.cwd.as_deref().and_then(empty_to_none),
        last_activity: pane.last_activity,
        awake: true,
        matches: Vec::new(),
    };
    let live_signals = pane_signals(&live);
    live.matches = peers
        .iter()
        .filter(|peer| {
            let peer_signals = normalized_peer_signals(peer);
            live_signals
                .iter()
                .any(|signal| peer_signals.iter().any(|peer_signal| peer_signal == signal))
        })
        .filter_map(|peer| {
            peer.name
                .clone()
                .or_else(|| peer.node.clone())
                .or_else(|| peer.oracle.clone())
        })
        .collect();
    live
}

fn fallback_target_parts(target: &str) -> TmuxPaneTargetParts {
    let session = target
        .split_once(':')
        .map_or(target, |(session, _)| session);
    TmuxPaneTargetParts {
        session: session.to_owned(),
        window: String::new(),
        pane: String::new(),
    }
}

fn pane_signals(pane: &DiscoverLivePane) -> Vec<String> {
    let mut signals = Vec::new();
    signals.extend(normalized_aliases(Some(&pane.session)));
    signals.extend(normalized_aliases(Some(&pane.window)));
    signals.extend(normalized_aliases(pane.title.as_deref()));
    if let Some(cwd) = pane.cwd.as_deref().and_then(path_basename) {
        signals.extend(normalized_aliases(Some(cwd)));
    }
    signals
}

fn normalized_peer_signals(peer: &maw_peer::PeerTarget) -> Vec<String> {
    let mut signals = Vec::new();
    signals.extend(normalized_aliases(peer.name.as_deref()));
    signals.extend(normalized_aliases(peer.node.as_deref()));
    signals.extend(normalized_aliases(peer.oracle.as_deref()));
    signals
}

fn normalized_aliases(value: Option<&str>) -> Vec<String> {
    let Some(normalized) = normalize_signal(value) else {
        return Vec::new();
    };
    let without_numeric = strip_numeric_prefix(&normalized).to_owned();
    let without_oracle = strip_oracle_suffix(&normalized).to_owned();
    let without_both = strip_oracle_suffix(strip_numeric_prefix(&normalized)).to_owned();
    unique_preserve_order(vec![
        normalized,
        without_numeric,
        without_oracle,
        without_both,
    ])
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect()
}

fn normalize_signal(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim().to_lowercase();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn strip_numeric_prefix(value: &str) -> &str {
    let Some((prefix, rest)) = value.split_once('-') else {
        return value;
    };
    if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        rest
    } else {
        value
    }
}

fn strip_oracle_suffix(value: &str) -> &str {
    value.strip_suffix("-oracle").unwrap_or(value)
}

fn path_basename(path: &str) -> Option<&str> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
}

fn empty_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn unique_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    }
    out
}

#[cfg(test)]
mod coverage_gap_tests {
include!("coverage_gap_tests_impl/helper_live_state_edges.rs");
}
