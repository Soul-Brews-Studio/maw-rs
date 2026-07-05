use maw_cli::run_cli;
use serde_json::Value;

#[test]
fn federation_health_plan_json_classifies_symmetric_pairs() {
    let output = run_cli(&[
        "federation-health".to_owned(),
        "--plan-json".to_owned(),
        "--node".to_owned(),
        "white".to_owned(),
        "--local-url".to_owned(),
        "http://localhost:3456".to_owned(),
        "--peer".to_owned(),
        "http://alpha:3456|alpha|reachable|40|mawjs,pulse|ok".to_owned(),
        "--remote".to_owned(),
        "http://alpha:3456|peer|http://localhost:3456|white|reachable".to_owned(),
        "--peer".to_owned(),
        "http://bravo:3456|bravo|reachable|55||ok".to_owned(),
        "--remote".to_owned(),
        "http://bravo:3456|missing-peers".to_owned(),
        "--peer".to_owned(),
        "http://charlie:3456|-|unreachable|-||clock".to_owned(),
        "--peer".to_owned(),
        "http://delta:3456|delta|reachable|-||ok".to_owned(),
        "--remote".to_owned(),
        "http://delta:3456|fetch-error|network cable unplugged".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");

    assert_eq!(json["command"], "federation-health");
    assert_eq!(json["localNode"], "white");
    assert_eq!(json["healthyPairs"], 1);
    assert_eq!(json["totalPairs"], 4);
    assert_eq!(json["pairs"][0]["pair"], "healthy");
    assert_eq!(json["pairs"][0]["reverse"], true);
    assert_eq!(json["pairs"][0]["agents"][1], "pulse");
    assert_eq!(json["pairs"][1]["pair"], "half-up");
    assert!(json["pairs"][1]["reason"]
        .as_str()
        .unwrap()
        .contains("not in peer"));
    assert_eq!(json["pairs"][2]["pair"], "down");
    assert_eq!(json["pairs"][2]["clockWarning"], true);
    assert_eq!(json["pairs"][3]["pair"], "unknown");
    assert!(json["pairs"][3]["reason"]
        .as_str()
        .unwrap()
        .contains("network cable"));
}

#[test]
fn federation_health_plan_supports_legacy_url_reverse_match() {
    let output = run_cli(&[
        "federation-health".to_owned(),
        "--plan-json".to_owned(),
        "--node".to_owned(),
        "white".to_owned(),
        "--local-url".to_owned(),
        "http://localhost:3456".to_owned(),
        "--peer".to_owned(),
        "http://mba:3456|-|reachable|-||ok".to_owned(),
        "--remote".to_owned(),
        "http://mba:3456|peer|http://localhost:3456|-|reachable".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");
    assert_eq!(json["pairs"][0]["pair"], "healthy");
}

#[test]
fn federation_health_plan_rejects_bad_peer_shape() {
    let output = run_cli(&[
        "federation-health".to_owned(),
        "--peer".to_owned(),
        "bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("--peer must use"),
        "{}",
        output.stderr
    );
}
