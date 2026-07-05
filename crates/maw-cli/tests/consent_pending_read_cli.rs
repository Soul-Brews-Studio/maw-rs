use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

#[test]
fn consent_pending_read_plan_returns_matching_request_without_plaintext_pin() {
    let json = json(&[
        "consent-pending-read".to_owned(),
        "--request".to_owned(),
        "id=old,from=a,to=b,action=hey,summary=old,pin_hash=h1,created_at=2026-01-02T00:00:00.000Z,expires_at=2026-01-02T00:01:00.000Z,status=pending".to_owned(),
        "--request".to_owned(),
        "id=new,from=c,to=d,action=team-invite,summary=new,pin_hash=h2,created_at=2026-01-02T00:00:01.000Z,expires_at=2026-01-02T00:01:01.000Z,status=expired".to_owned(),
        "--id".to_owned(),
        "new".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(json["command"], "consent-pending-read");
    assert_eq!(json["found"], true);
    assert_eq!(json["id"], "new");
    assert_eq!(json["request"]["id"], "new");
    assert_eq!(json["request"]["status"], "expired");
    assert_eq!(json["request"]["pin"], Value::Null);
}

#[test]
fn consent_pending_read_plan_reports_missing_and_rejects_missing_id() {
    let json = json(&[
        "consent-pending-read".to_owned(),
        "--request".to_owned(),
        "id=req,from=a,to=b,action=hey,summary=hello,pin_hash=h1,created_at=2026-01-02T00:00:00.000Z,expires_at=2026-01-02T00:01:00.000Z,status=pending".to_owned(),
        "--id".to_owned(),
        "missing".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(json["found"], false);
    assert_eq!(json["id"], "missing");
    assert_eq!(json["request"], Value::Null);

    let missing = run_cli(&["consent-pending-read".to_owned()]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing --id value"),
        "{}",
        missing.stderr
    );
}
