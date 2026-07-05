use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

#[test]
fn consent_trust_check_plan_reports_trusted_key_and_matching_entry() {
    let json = json(&[
        "consent-trust-check".to_owned(),
        "--entry".to_owned(),
        "from=a,to=b,action=hey,approved_at=2026-01-02T00:00:00.000Z,approved_by=human,request_id=req-a".to_owned(),
        "--check".to_owned(),
        "a:b:hey".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(json["command"], "consent-trust-check");
    assert_eq!(json["trusted"], true);
    assert_eq!(json["trustKey"], "a→b:hey");
    assert_eq!(json["entry"]["from"], "a");
    assert_eq!(json["entry"]["requestId"], "req-a");
}

#[test]
fn consent_trust_check_plan_preserves_asymmetry_and_rejects_missing_check() {
    let json = json(&[
        "consent-trust-check".to_owned(),
        "--entry".to_owned(),
        "from=a,to=b,action=hey,approved_at=2026-01-02T00:00:00.000Z,approved_by=human".to_owned(),
        "--check".to_owned(),
        "b:a:hey".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(json["trusted"], false);
    assert_eq!(json["trustKey"], "b→a:hey");
    assert_eq!(json["entry"], Value::Null);

    let missing = run_cli(&["consent-trust-check".to_owned()]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing --check value"),
        "{}",
        missing.stderr
    );
}
