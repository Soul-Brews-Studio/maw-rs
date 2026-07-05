use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

#[test]
fn consent_trust_revoke_plan_removes_matching_trust_and_keeps_sorted_remaining_entries() {
    let json = json(&[
        "consent-trust-revoke".to_owned(),
        "--entry".to_owned(),
        "from=b,to=c,action=hey,approved_at=2026-01-02T00:00:01.000Z,approved_by=human,request_id=req-b".to_owned(),
        "--entry".to_owned(),
        "from=a,to=b,action=plugin-install,approved_at=2026-01-02T00:00:00.000Z,approved_by=auto".to_owned(),
        "--revoke".to_owned(),
        "a:b:plugin-install".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(json["command"], "consent-trust-revoke");
    assert_eq!(json["revoked"], true);
    assert_eq!(json["revokedKey"], "a→b:plugin-install");
    assert_eq!(json["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(json["entries"][0]["from"], "b");
    assert_eq!(json["entries"][0]["requestId"], "req-b");
}

#[test]
fn consent_trust_revoke_plan_reports_missing_revoke_and_rejects_bad_key() {
    let json = json(&[
        "consent-trust-revoke".to_owned(),
        "--entry".to_owned(),
        "from=a,to=b,action=hey,approved_at=2026-01-02T00:00:00.000Z,approved_by=human".to_owned(),
        "--revoke".to_owned(),
        "missing:node:hey".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(json["revoked"], false);
    assert_eq!(json["entries"][0]["from"], "a");

    let bad = run_cli(&[
        "consent-trust-revoke".to_owned(),
        "--revoke".to_owned(),
        "bad-key".to_owned(),
    ]);
    assert_eq!(bad.code, 2);
    assert!(
        bad.stderr.contains("key must use from:to:action"),
        "{}",
        bad.stderr
    );
}
