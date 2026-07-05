use maw_cli::run_cli;
use serde_json::Value;

#[test]
fn federation_identity_plan_json_lists_local_and_explicit_node_agents() {
    let output = run_cli(&[
        "federation-identity".to_owned(),
        "--plan-json".to_owned(),
        "--node".to_owned(),
        "white".to_owned(),
        "--url".to_owned(),
        "http://white:3456".to_owned(),
        "--agent".to_owned(),
        "mawjs=local".to_owned(),
        "--agent".to_owned(),
        "pulse=white".to_owned(),
        "--agent".to_owned(),
        "homekeeper=mba".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");

    assert_eq!(json["command"], "federation-identity");
    assert_eq!(json["node"], "white");
    assert_eq!(json["url"], "http://white:3456");
    assert_eq!(json["agents"].as_array().expect("agents").len(), 2);
    assert_eq!(json["agents"][0], "mawjs");
    assert_eq!(json["agents"][1], "pulse");
    assert_eq!(json["routes"]["homekeeper"], "mba");
}

#[test]
fn federation_identity_plan_defaults_node_and_url_without_agents() {
    let output = run_cli(&["federation-identity".to_owned(), "--plan-json".to_owned()]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");
    assert_eq!(json["node"], "local");
    assert_eq!(json["url"], "");
    assert_eq!(json["agents"].as_array().expect("agents").len(), 0);
}

#[test]
fn federation_identity_plan_rejects_bad_agent_shape() {
    let output = run_cli(&[
        "federation-identity".to_owned(),
        "--agent".to_owned(),
        "bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("--agent must use"),
        "{}",
        output.stderr
    );
}
