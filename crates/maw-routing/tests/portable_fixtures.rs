use maw_routing::{resolve_target, MawConfig, NamedPeer, ResolveResult, Session, Window};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureRoot {
    base_config: Value,
    cases: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    query: String,
    config: Option<Value>,
    sessions: Vec<FixtureSession>,
    expected: ExpectedResolveResult,
}

#[derive(Debug, Deserialize)]
struct FixtureConfig {
    node: Option<String>,
    #[serde(default, rename = "namedPeers")]
    named_peers: Vec<NamedPeerConfig>,
    #[serde(default)]
    peers: Vec<String>,
    #[serde(default)]
    agents: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct NamedPeerConfig {
    name: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct FixtureSession {
    name: String,
    windows: Vec<FixtureWindow>,
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureWindow {
    index: u32,
    name: String,
    active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum ExpectedResolveResult {
    Local {
        target: String,
    },
    Peer {
        #[serde(rename = "peerUrl")]
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

impl From<FixtureConfig> for MawConfig {
    fn from(config: FixtureConfig) -> Self {
        Self {
            node: config.node,
            named_peers: config.named_peers.into_iter().map(Into::into).collect(),
            peers: config.peers,
            agents: config.agents,
        }
    }
}

impl From<NamedPeerConfig> for NamedPeer {
    fn from(peer: NamedPeerConfig) -> Self {
        Self {
            name: peer.name,
            url: peer.url,
        }
    }
}

impl From<FixtureSession> for Session {
    fn from(session: FixtureSession) -> Self {
        Self {
            name: session.name,
            windows: session.windows.into_iter().map(Into::into).collect(),
            source: session.source,
        }
    }
}

impl From<FixtureWindow> for Window {
    fn from(window: FixtureWindow) -> Self {
        Self {
            index: window.index,
            name: window.name,
            active: window.active,
            kind: None,
        }
    }
}

fn expected_result(expected: ExpectedResolveResult) -> ResolveResult {
    match expected {
        ExpectedResolveResult::Local { target } => ResolveResult::Local { target },
        ExpectedResolveResult::Peer {
            peer_url,
            target,
            node,
        } => ResolveResult::Peer {
            peer_url,
            target,
            node,
        },
        ExpectedResolveResult::SelfNode { target } => ResolveResult::SelfNode { target },
        ExpectedResolveResult::Error {
            reason,
            detail,
            hint,
        } => ResolveResult::Error {
            reason,
            detail,
            hint,
        },
    }
}

fn merged_config(base: &Value, overlay: Option<&Value>) -> MawConfig {
    let mut merged = base.clone();
    if let Some(Value::Object(overrides)) = overlay {
        let Value::Object(base_object) = &mut merged else {
            panic!("base config must be an object");
        };
        for (key, value) in overrides {
            base_object.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value::<FixtureConfig>(merged)
        .expect("fixture config shape")
        .into()
}

#[test]
fn routing_fixtures_match_maw_js_portable_spec() {
    let fixtures: FixtureRoot =
        serde_json::from_str(include_str!("fixtures/routing.fixtures.json"))
            .expect("valid routing fixture json");

    for fixture in fixtures.cases {
        let config = merged_config(&fixtures.base_config, fixture.config.as_ref());
        let sessions: Vec<Session> = fixture.sessions.into_iter().map(Into::into).collect();
        let actual = resolve_target(&fixture.query, &config, &sessions);
        assert_eq!(
            actual,
            expected_result(fixture.expected),
            "{}",
            fixture.name
        );
    }
}

#[test]
fn routing_differential_corpus_matches_maw_js_generated_fixtures() {
    // Regenerate with: bun scripts/gen-routing-corpus.ts
    let fixtures: FixtureRoot =
        serde_json::from_str(include_str!("fixtures/differential/generated.json"))
            .expect("valid differential routing corpus json");
    for fixture in fixtures.cases {
        let config = merged_config(&fixtures.base_config, fixture.config.as_ref());
        let sessions: Vec<Session> = fixture.sessions.into_iter().map(Into::into).collect();
        assert_eq!(
            resolve_target(&fixture.query, &config, &sessions),
            expected_result(fixture.expected),
            "{}",
            fixture.name
        );
    }
}
