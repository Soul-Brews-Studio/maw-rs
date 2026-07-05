use maw_cli::run_cli;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SessionFixture {
    name: String,
    input: SessionInput,
    expected: String,
}

#[derive(Debug, Deserialize)]
struct SessionInput {
    oracle: String,
    slot: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct NodeFixture {
    name: String,
    input: NodeInput,
    expected: String,
}

#[derive(Debug, Deserialize)]
struct NodeInput {
    host: String,
    user: Option<String>,
}

#[test]
fn identity_session_plan_cli_matches_maw_js_fixtures() {
    let fixtures: Vec<SessionFixture> = serde_json::from_str(include_str!(
        "../../maw-identity/tests/fixtures/canonical-session-name.fixtures.json"
    ))
    .expect("valid canonical session fixtures");

    for fixture in fixtures {
        let mut argv = vec![
            "identity".to_owned(),
            "session-name".to_owned(),
            fixture.input.oracle.clone(),
            "--plan-json".to_owned(),
        ];
        if let Some(slot) = fixture.input.slot {
            argv.push("--slot".to_owned());
            argv.push(slot.to_string());
        }
        let output = run_cli(&argv);
        assert_eq!(output.code, 0, "{}: {}", fixture.name, output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
            });
        assert_eq!(json["command"], "identity", "{}", fixture.name);
        assert_eq!(json["kind"], "sessionName", "{}", fixture.name);
        assert_eq!(
            json["input"]["oracle"], fixture.input.oracle,
            "{}",
            fixture.name
        );
        if let Some(slot) = fixture.input.slot {
            assert_eq!(json["input"]["slot"], slot, "{}", fixture.name);
        } else {
            assert!(json["input"].get("slot").is_none(), "{}", fixture.name);
        }
        assert_eq!(json["canonical"], fixture.expected, "{}", fixture.name);
    }
}

#[test]
fn identity_node_plan_cli_matches_maw_js_fixtures() {
    let fixtures: Vec<NodeFixture> = serde_json::from_str(include_str!(
        "../../maw-identity/tests/fixtures/canonical-node-identity.fixtures.json"
    ))
    .expect("valid canonical node fixtures");

    for fixture in fixtures {
        let mut argv = vec![
            "identity".to_owned(),
            "node-identity".to_owned(),
            fixture.input.host.clone(),
            "--plan-json".to_owned(),
        ];
        if let Some(user) = &fixture.input.user {
            argv.push("--user".to_owned());
            argv.push(user.clone());
        }
        let output = run_cli(&argv);
        assert_eq!(output.code, 0, "{}: {}", fixture.name, output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
            });
        assert_eq!(json["command"], "identity", "{}", fixture.name);
        assert_eq!(json["kind"], "nodeIdentity", "{}", fixture.name);
        assert_eq!(
            json["input"]["host"], fixture.input.host,
            "{}",
            fixture.name
        );
        if let Some(user) = fixture.input.user {
            assert_eq!(json["input"]["user"], user, "{}", fixture.name);
        } else {
            assert!(json["input"].get("user").is_none(), "{}", fixture.name);
        }
        assert_eq!(json["canonical"], fixture.expected, "{}", fixture.name);
    }
}

#[test]
fn identity_plan_rejects_bad_slot_and_misordered_flags() {
    let bad_slot = run_cli(&[
        "identity".to_owned(),
        "session-name".to_owned(),
        "mawjs-codex-oracle".to_owned(),
        "--slot".to_owned(),
        "many".to_owned(),
    ]);
    assert_eq!(bad_slot.code, 2);
    assert!(bad_slot.stderr.contains("--slot must be an integer"));

    let misplaced_user = run_cli(&[
        "identity".to_owned(),
        "--user".to_owned(),
        "alpha".to_owned(),
    ]);
    assert_eq!(misplaced_user.code, 2);
    assert!(misplaced_user
        .stderr
        .contains("--user requires node-identity"));
}
