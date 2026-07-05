use maw_cli::run_cli;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    mode: ModeFixture,
    config: ConfigFixture,
    discoveries: DiscoveriesFixture,
    expected: ExpectedFixture,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFixture {
    #[serde(default)]
    peers: Vec<String>,
    #[serde(default)]
    named_peers: Vec<NamedPeerFixture>,
}

#[derive(Debug, Deserialize)]
struct NamedPeerFixture {
    name: String,
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedFixture {
    urls: Vec<String>,
    names: Vec<Option<String>>,
    sources: Vec<String>,
    warnings: Vec<String>,
    fetch_calls: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ModeFixture {
    Config,
    Scout,
    Both,
}

impl ModeFixture {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Scout => "scout",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Deserialize)]
struct DiscoveriesFixture {
    ok: bool,
    #[serde(default)]
    peers: Vec<DiscoveryRowFixture>,
    error: Option<String>,
    hint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscoveryRowFixture {
    node: Option<String>,
    oracle: Option<String>,
    host: Option<String>,
    locators: Vec<String>,
}

#[test]
fn peer_sources_plan_cli_matches_maw_js_fixtures() {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../maw-peer/tests/fixtures/peer-source-resolver.fixtures.json"
    ))
    .expect("valid peer source fixtures");

    for fixture in fixtures {
        let mut argv = vec![
            "peer-sources".to_owned(),
            "--plan-json".to_owned(),
            "--mode".to_owned(),
            fixture.mode.as_str().to_owned(),
        ];
        for peer in &fixture.config.peers {
            argv.push("--peer".to_owned());
            argv.push(peer.clone());
        }
        for peer in &fixture.config.named_peers {
            argv.push("--named-peer".to_owned());
            argv.push(format!("{}={}", peer.name, peer.url));
        }
        if fixture.discoveries.ok {
            argv.push("--discovery-ok".to_owned());
            for peer in &fixture.discoveries.peers {
                argv.push("--discovered".to_owned());
                argv.push(format!(
                    "{}|{}|{}|{}",
                    peer.node.as_deref().unwrap_or("-"),
                    peer.host.as_deref().unwrap_or("-"),
                    peer.oracle.as_deref().unwrap_or("-"),
                    peer.locators.join(",")
                ));
            }
        } else {
            argv.push("--discovery-error".to_owned());
            argv.push(fixture.discoveries.error.clone().unwrap_or_default());
            if let Some(hint) = &fixture.discoveries.hint {
                argv.push("--discovery-hint".to_owned());
                argv.push(hint.clone());
            }
        }

        let output = run_cli(&argv);
        assert_eq!(output.code, 0, "{} stderr: {}", fixture.name, output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
            });
        assert_eq!(json["command"], "peer-sources", "{}", fixture.name);
        assert_eq!(json["mode"], fixture.mode.as_str(), "{}", fixture.name);
        assert_eq!(
            json["fetchCalls"], fixture.expected.fetch_calls,
            "{}",
            fixture.name
        );
        let peers = json["peers"].as_array().expect("peers array");
        assert_eq!(peers.len(), fixture.expected.urls.len(), "{}", fixture.name);
        for (idx, peer) in peers.iter().enumerate() {
            assert_eq!(peer["url"], fixture.expected.urls[idx], "{}", fixture.name);
            assert_eq!(
                peer["source"], fixture.expected.sources[idx],
                "{}",
                fixture.name
            );
            match &fixture.expected.names[idx] {
                Some(name) => assert_eq!(peer["name"], *name, "{}", fixture.name),
                None => assert!(peer.get("name").is_none(), "{}", fixture.name),
            }
        }
        let warnings = json["warnings"].as_array().expect("warnings array");
        for expected in &fixture.expected.warnings {
            assert!(
                warnings.iter().any(|warning| warning
                    .as_str()
                    .is_some_and(|actual| actual.contains(expected))),
                "{} missing warning {expected:?}: {warnings:?}",
                fixture.name
            );
        }
    }
}

#[test]
fn peer_sources_plan_rejects_bad_named_peer_shape() {
    let argv = vec![
        "peer-sources".to_owned(),
        "--mode".to_owned(),
        "config".to_owned(),
        "--named-peer".to_owned(),
        "missing-equals".to_owned(),
    ];

    let output = run_cli(&argv);
    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("--named-peer must use"),
        "{}",
        output.stderr
    );
}
