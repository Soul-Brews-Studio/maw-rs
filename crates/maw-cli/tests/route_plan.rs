use maw_cli::run_cli;
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
    expected: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureConfig {
    node: Option<String>,
    #[serde(default)]
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

fn merged_config(base: &Value, overlay: Option<&Value>) -> FixtureConfig {
    let mut merged = base.clone();
    if let Some(Value::Object(overrides)) = overlay {
        let Value::Object(base_object) = &mut merged else {
            panic!("base config must be an object");
        };
        for (key, value) in overrides {
            base_object.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value(merged).expect("fixture config")
}

#[test]
fn route_plan_cli_matches_maw_js_routing_fixtures() {
    let fixtures: FixtureRoot = serde_json::from_str(include_str!(
        "../../maw-routing/tests/fixtures/routing.fixtures.json"
    ))
    .expect("valid routing fixtures");

    for fixture in fixtures.cases {
        let config = merged_config(&fixtures.base_config, fixture.config.as_ref());
        let mut argv = vec![
            "route".to_owned(),
            "--plan-json".to_owned(),
            "--query".to_owned(),
            fixture.query.clone(),
        ];
        if let Some(node) = config.node {
            argv.push("--node".to_owned());
            argv.push(node);
        }
        for peer in config.named_peers {
            argv.push("--named-peer".to_owned());
            argv.push(format!("{}={}", peer.name, peer.url));
        }
        for peer in config.peers {
            argv.push("--peer".to_owned());
            argv.push(peer);
        }
        for (agent, node) in config.agents {
            argv.push("--agent".to_owned());
            argv.push(format!("{agent}={node}"));
        }
        for session in &fixture.sessions {
            argv.push("--session".to_owned());
            argv.push(session.name.clone());
            if let Some(source) = &session.source {
                argv.push("--source".to_owned());
                argv.push(source.clone());
            }
            for window in &session.windows {
                argv.push("--window".to_owned());
                argv.push(format!(
                    "{}:{}:{}",
                    window.index, window.name, window.active
                ));
            }
        }

        let output = run_cli(&argv);
        assert_eq!(output.code, 0, "{} stderr: {}", fixture.name, output.stderr);
        let actual: Value = serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
            panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
        });
        assert_eq!(actual["command"], "route", "{}", fixture.name);
        assert_eq!(actual["query"], fixture.query, "{}", fixture.name);
        for key in [
            "type", "target", "peerUrl", "node", "reason", "detail", "hint",
        ] {
            assert_eq!(
                actual.get(key),
                fixture.expected.get(key),
                "{} key {key}",
                fixture.name
            );
        }
    }
}

#[test]
fn route_plan_json_includes_error_hint() {
    let output = run_cli(&[
        "route".to_owned(),
        "--plan-json".to_owned(),
        "--query".to_owned(),
        String::new(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let actual: Value = serde_json::from_str(&output.stdout).expect("route json");
    assert_eq!(actual["type"], "error");
    assert_eq!(actual["hint"], "usage: maw hey <agent> <message>");
}

#[test]
fn route_plan_colon_numeric_prefers_window_index_over_name_suffix() {
    let output = run_cli(&[
        "route".to_owned(),
        "--plan-json".to_owned(),
        "--query".to_owned(),
        "188-maw-rs:1".to_owned(),
        "--session".to_owned(),
        "188-maw-rs".to_owned(),
        "--window".to_owned(),
        "1:maw-rs-oracle:true".to_owned(),
        "--window".to_owned(),
        "2:maw-rs-codex-1:false".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let actual: Value = serde_json::from_str(&output.stdout).expect("route json");
    assert_eq!(actual["type"], "local");
    assert_eq!(actual["target"], "188-maw-rs:1");
}

#[test]
fn route_plan_colon_full_window_name_stays_exact() {
    let output = run_cli(&[
        "route".to_owned(),
        "--plan-json".to_owned(),
        "--query".to_owned(),
        "188-maw-rs:maw-rs-codex-1".to_owned(),
        "--session".to_owned(),
        "188-maw-rs".to_owned(),
        "--window".to_owned(),
        "2:maw-rs-codex-10:false".to_owned(),
        "--window".to_owned(),
        "1:maw-rs-codex-1:true".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let actual: Value = serde_json::from_str(&output.stdout).expect("route json");
    assert_eq!(actual["type"], "local");
    assert_eq!(actual["target"], "188-maw-rs:1");
}

#[test]
fn route_plan_rejects_window_without_session() {
    let argv = vec![
        "route".to_owned(),
        "--query".to_owned(),
        "local-oracle".to_owned(),
        "--window".to_owned(),
        "1:local-oracle:true".to_owned(),
    ];

    let output = run_cli(&argv);
    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("--window must follow a --session"),
        "{}",
        output.stderr
    );
}
