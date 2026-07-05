use maw_cli::run_cli;
use serde_json::Value;

#[test]
fn federation_sync_plan_json_matches_maw_js_diff_and_apply_contract() {
    let output = run_cli(&[
        "federation-sync".to_owned(),
        "--plan-json".to_owned(),
        "--node".to_owned(),
        "oracle-world".to_owned(),
        "--agent".to_owned(),
        "mawjs=local".to_owned(),
        "--agent".to_owned(),
        "volt=white".to_owned(),
        "--agent".to_owned(),
        "oldGuy=white".to_owned(),
        "--identity".to_owned(),
        "white|http://white:3456|white|mawjs,volt,pulse|reachable".to_owned(),
        "--identity".to_owned(),
        "mba|http://mba:3456|mba|homekeeper,netkeeper|reachable".to_owned(),
        "--identity".to_owned(),
        "clinic|http://clinic:3456|clinic-nat||unreachable|timeout".to_owned(),
        "--prune".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");

    assert_eq!(json["command"], "federation-sync");
    assert_eq!(json["node"], "oracle-world");
    assert_eq!(json["dirty"], true);
    assert_eq!(json["diff"]["add"].as_array().expect("add").len(), 3);
    assert_eq!(json["diff"]["stale"][0]["oracle"], "oldGuy");
    assert_eq!(json["diff"]["unreachable"][0]["peerName"], "clinic");
    assert_eq!(json["applied"].as_array().expect("applied").len(), 4);
    assert_eq!(json["agents"]["pulse"], "white");
    assert_eq!(json["agents"]["homekeeper"], "mba");
    assert!(json["agents"].get("oldGuy").is_none());
}

#[test]
fn federation_sync_check_returns_dirty_ci_code_without_applying() {
    let output = run_cli(&[
        "federation-sync".to_owned(),
        "--plan-json".to_owned(),
        "--check".to_owned(),
        "--agent".to_owned(),
        "neo=old-node".to_owned(),
        "--identity".to_owned(),
        "white|https://white.example|white|neo|reachable".to_owned(),
    ]);

    assert_eq!(output.code, 1, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");
    assert_eq!(json["diff"]["conflict"][0]["current"], "old-node");
    assert_eq!(json["applied"].as_array().expect("applied").len(), 0);
    assert_eq!(json["agents"]["neo"], "old-node");
}

#[test]
fn federation_sync_plan_rejects_bad_identity_shape() {
    let output = run_cli(&[
        "federation-sync".to_owned(),
        "--identity".to_owned(),
        "bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("--identity must use"),
        "{}",
        output.stderr
    );
}
