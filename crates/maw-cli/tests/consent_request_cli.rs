use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

fn base_args() -> Vec<String> {
    vec![
        "consent-request".to_owned(),
        "--plan-json".to_owned(),
        "--from".to_owned(),
        "neo".to_owned(),
        "--to".to_owned(),
        "mawjs".to_owned(),
        "--action".to_owned(),
        "hey".to_owned(),
        "--summary".to_owned(),
        "hello".to_owned(),
        "--request-id".to_owned(),
        "00112233445566778899aabb".to_owned(),
        "--pin".to_owned(),
        "ABCDEF".to_owned(),
        "--now".to_owned(),
        "1767312000000".to_owned(),
    ]
}

#[test]
fn consent_request_plan_creates_pending_without_echoing_pin() {
    let json = json(&base_args());

    assert_eq!(json["command"], "consent-request");
    assert_eq!(json["ok"], true);
    assert_eq!(json["requestId"], "00112233445566778899aabb");
    assert_eq!(json["pin"], Value::Null);
    assert_eq!(json["pinRedacted"], "ABC-***");
    assert_eq!(json["expiresAt"], "2026-01-02T00:10:00.000Z");
    assert_eq!(json["peerUrl"], Value::Null);
    assert_eq!(json["peerBody"], Value::Null);
    assert_eq!(json["pending"]["summary"], "hello");
    assert_eq!(json["pending"]["pinHash"].as_str().unwrap().len(), 64);
}

#[test]
fn consent_request_plan_models_peer_http_failure_with_redacted_body() {
    let mut args = base_args();
    args.extend([
        "--peer-url".to_owned(),
        "http://peer:3456/".to_owned(),
        "--peer-http-status".to_owned(),
        "500".to_owned(),
    ]);
    let json = json(&args);

    assert_eq!(json["ok"], false);
    assert_eq!(json["error"], "peer rejected request: HTTP 500");
    assert_eq!(json["peerUrl"], "http://peer:3456/api/consent/request");
    assert_eq!(json["peerMethod"], "POST");
    assert_eq!(json["peerBody"]["pin"], Value::Null);
    assert_eq!(json["peerBody"]["pinHash"].as_str().unwrap().len(), 64);
}

#[test]
fn consent_request_plan_rejects_bad_action_and_missing_fields() {
    let bad_action = run_cli(&[
        "consent-request".to_owned(),
        "--action".to_owned(),
        "bad".to_owned(),
    ]);
    assert_eq!(bad_action.code, 2);
    assert!(
        bad_action.stderr.contains("invalid --action"),
        "{}",
        bad_action.stderr
    );

    let missing = run_cli(&["consent-request".to_owned()]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing --from value"),
        "{}",
        missing.stderr
    );
}
