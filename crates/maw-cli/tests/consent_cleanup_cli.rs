use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

#[test]
fn consent_cleanup_plan_deletes_pending_request_and_reports_remaining_newest_first() {
    let json = json(&[
        "consent-cleanup".to_owned(),
        "--request".to_owned(),
        "id=old,from=a,to=b,action=hey,summary=old,pin_hash=h1,created_at=2026-01-02T00:00:00.000Z,expires_at=2026-01-02T00:01:00.000Z,status=pending".to_owned(),
        "--request".to_owned(),
        "id=new,from=c,to=d,action=team-invite,summary=new,pin_hash=h2,created_at=2026-01-02T00:00:01.000Z,expires_at=2026-01-02T00:01:01.000Z,status=expired".to_owned(),
        "--delete".to_owned(),
        "old".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(json["command"], "consent-cleanup");
    assert_eq!(json["deleted"], true);
    assert_eq!(json["deletedId"], "old");
    assert_eq!(json["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(json["entries"][0]["id"], "new");
    assert_eq!(json["entries"][0]["pin"], Value::Null);
}

#[test]
fn consent_cleanup_plan_reports_missing_delete_and_rejects_missing_id() {
    let json = json(&[
        "consent-cleanup".to_owned(),
        "--request".to_owned(),
        "id=req,from=a,to=b,action=hey,summary=hello,pin_hash=h1,created_at=2026-01-02T00:00:00.000Z,expires_at=2026-01-02T00:01:00.000Z,status=pending".to_owned(),
        "--delete".to_owned(),
        "missing".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(json["deleted"], false);
    assert_eq!(json["entries"][0]["id"], "req");

    let missing = run_cli(&["consent-cleanup".to_owned(), "--delete".to_owned()]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing --delete value"),
        "{}",
        missing.stderr
    );
}
