use maw_cli::run_cli;
use serde_json::Value;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn ok_json(values: &[&str]) -> Value {
    let output = run_cli(&args(values));
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid json: {error}\n{}", output.stdout))
}

#[test]
fn discover_inventory_matches_fleet_window_and_scout_oracle_identity() {
    let json = ok_json(&[
        "discover",
        "--peers",
        "both",
        "--discovered",
        "scout-node|scout-host|oracle-only|http://scout:3456",
        "--fleet",
        "fleet.json|7|different-name|sess|oracle-window|owner/repo",
        "--oracle",
        "oracle-window|fleet|-|-|-|owner/repo|-|false|true",
        "--oracle",
        "oracle-only|scout|-|-|-|-|-|false|false",
        "--json",
        "--plan-json",
    ]);

    let records = json["oracles"]
        .get("records")
        .and_then(Value::as_array)
        .unwrap();
    let window_oracle = records
        .iter()
        .find(|record| record["name"] == "oracle-window")
        .unwrap();
    assert_eq!(window_oracle["fleetMatched"], true);

    let scout_oracle = records
        .iter()
        .find(|record| record["name"] == "oracle-only")
        .unwrap();
    assert_eq!(
        scout_oracle["peerUrls"],
        serde_json::json!(["http://scout:3456"])
    );
}

#[test]
fn route_json_includes_error_hint_for_bad_node_agent_shape() {
    let json = ok_json(&[
        "route",
        "--plan-json",
        "--node",
        "white",
        "--query",
        ":ghost",
    ]);

    assert_eq!(json["type"], "error");
    assert_eq!(json["reason"], "empty_node_or_agent");
    assert_eq!(json["hint"], "use node:agent format (e.g. mba:homekeeper)");
}

#[test]
fn ls_recent_json_uses_session_name_as_tie_breaker_for_equal_created_times() {
    let json = ok_json(&[
        "ls",
        "--plan-json",
        "--recent",
        "2",
        "--now",
        "2000",
        "--session-created",
        "beta=100",
        "--session-created",
        "alpha=100",
        "--pane",
        "%1|claude|beta:1.0|beta|100|/repo|1990",
        "--pane",
        "%2|claude|alpha:1.0|alpha|101|/repo|1980",
    ]);

    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions[0]["session"], "alpha");
    assert_eq!(sessions[1]["session"], "beta");
}

#[test]
fn ls_without_fake_panes_uses_live_tmux_defaults_without_error() {
    let json = ok_json(&["ls", "--plan-json"]);

    assert_eq!(json["command"], "ls");
    assert_eq!(json["scope"], "local");
    assert!(json.get("sessions").is_some());
}

#[test]
fn calver_rejects_out_of_range_now_parts() {
    let output = run_cli(&args(&[
        "calver",
        "--now",
        "2026-13-1T0:0",
        "--package-version",
        "26.5.1",
    ]));

    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("out-of-range date/time parts"),
        "{}",
        output.stderr
    );
}
