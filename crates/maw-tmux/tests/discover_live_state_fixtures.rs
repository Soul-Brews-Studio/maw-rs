use maw_tmux::{
    mark_peer_targets_live, parse_tmux_pane_target, resolve_tmux_live_state, LivePeerTarget,
    TmuxPane,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    peers: Vec<PeerFixture>,
    panes: Vec<PaneFixture>,
    expected: ExpectedFixture,
}

#[derive(Debug, Deserialize)]
struct PeerFixture {
    name: Option<String>,
    url: String,
    source: SourceFixture,
    node: Option<String>,
    oracle: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SourceFixture {
    Config,
    Scout,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaneFixture {
    id: String,
    command: String,
    target: String,
    title: String,
    pid: Option<u32>,
    cwd: Option<String>,
    last_activity: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedFixture {
    live_targets: Vec<String>,
    sessions: Vec<String>,
    windows: Vec<String>,
    panes: Vec<String>,
    matches: Vec<Vec<String>>,
    awake_peers: Vec<String>,
}

impl From<PeerFixture> for LivePeerTarget {
    fn from(value: PeerFixture) -> Self {
        Self {
            name: value.name,
            url: value.url,
            source: match value.source {
                SourceFixture::Config => "config",
                SourceFixture::Scout => "scout",
            }
            .to_owned(),
            node: value.node,
            oracle: value.oracle,
        }
    }
}

impl From<PaneFixture> for TmuxPane {
    fn from(value: PaneFixture) -> Self {
        Self {
            id: value.id,
            command: value.command,
            target: value.target,
            title: value.title,
            pid: value.pid,
            cwd: value.cwd,
            last_activity: value.last_activity,
        }
    }
}

#[test]
fn discover_tmux_live_state_fixtures_match_maw_js_portable_spec() {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "fixtures/discover-tmux-live-state.fixtures.json"
    ))
    .expect("valid discover tmux live state fixture json");

    for fixture in fixtures {
        let peers: Vec<LivePeerTarget> = fixture.peers.into_iter().map(Into::into).collect();
        let panes: Vec<TmuxPane> = fixture.panes.into_iter().map(Into::into).collect();
        let result = resolve_tmux_live_state(&peers, &panes);
        let marked_peers = mark_peer_targets_live(&peers, &result.live);

        assert_eq!(
            result
                .live
                .iter()
                .map(|pane| pane.target.clone())
                .collect::<Vec<_>>(),
            fixture.expected.live_targets,
            "live targets: {}",
            fixture.name
        );
        assert_eq!(
            result
                .live
                .iter()
                .map(|pane| pane.session.clone())
                .collect::<Vec<_>>(),
            fixture.expected.sessions,
            "sessions: {}",
            fixture.name
        );
        assert_eq!(
            result
                .live
                .iter()
                .map(|pane| pane.window.clone())
                .collect::<Vec<_>>(),
            fixture.expected.windows,
            "windows: {}",
            fixture.name
        );
        assert_eq!(
            result
                .live
                .iter()
                .map(|pane| pane.pane.clone())
                .collect::<Vec<_>>(),
            fixture.expected.panes,
            "panes: {}",
            fixture.name
        );
        assert_eq!(
            result
                .live
                .iter()
                .map(|pane| pane.matches.clone())
                .collect::<Vec<_>>(),
            fixture.expected.matches,
            "matches: {}",
            fixture.name
        );
        assert_eq!(
            marked_peers
                .iter()
                .filter(|peer| peer.awake)
                .map(|peer| {
                    peer.name
                        .clone()
                        .or_else(|| peer.node.clone())
                        .unwrap_or_else(|| peer.url.clone())
                })
                .collect::<Vec<_>>(),
            fixture.expected.awake_peers,
            "awake peers: {}",
            fixture.name
        );
    }
}

#[test]
fn target_parser_is_portable() {
    let parsed = parse_tmux_pane_target("101-mawjs:agent.0").expect("valid target");
    assert_eq!(parsed.session, "101-mawjs");
    assert_eq!(parsed.window, "agent");
    assert_eq!(parsed.pane, "0");
    assert!(parse_tmux_pane_target("not-a-pane-target").is_none());
}
