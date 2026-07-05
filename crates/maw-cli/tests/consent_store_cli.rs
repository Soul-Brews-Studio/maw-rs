use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

#[test]
fn consent_store_trust_plan_lists_sorted_entries_and_trust_key() {
    let json = json(&[
        "consent-store".to_owned(),
        "trust".to_owned(),
        "--entry".to_owned(),
        "from=b,to=c,action=hey,approved_at=2026-01-02T00:00:01.000Z,approved_by=human,request_id=req-b".to_owned(),
        "--entry".to_owned(),
        "from=a,to=b,action=plugin-install,approved_at=2026-01-02T00:00:00.000Z,approved_by=auto".to_owned(),
        "--check".to_owned(),
        "a:b:plugin-install".to_owned(),
        "--key".to_owned(),
        "a:b:plugin-install".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(json["command"], "consent-store");
    assert_eq!(json["mode"], "trust");
    assert_eq!(json["trusted"], true);
    assert_eq!(json["trustKey"], "a→b:plugin-install");
    assert_eq!(json["entries"][0]["from"], "a");
    assert_eq!(json["entries"][0]["approvedBy"], "auto");
    assert_eq!(json["entries"][0]["requestId"], Value::Null);
    assert_eq!(json["entries"][1]["from"], "b");
    assert_eq!(json["entries"][1]["requestId"], "req-b");
}

#[test]
fn consent_store_pending_plan_lists_newest_first_and_updates_status() {
    let json = json(&[
        "consent-store".to_owned(),
        "pending".to_owned(),
        "--request".to_owned(),
        "id=old,from=a,to=b,action=hey,summary=old,pin_hash=h1,created_at=2026-01-02T00:00:00.000Z,expires_at=2026-01-02T00:01:00.000Z,status=pending".to_owned(),
        "--request".to_owned(),
        "id=new,from=c,to=d,action=team-invite,summary=new,pin_hash=h2,created_at=2026-01-02T00:00:01.000Z,expires_at=2026-01-02T00:01:01.000Z,status=pending".to_owned(),
        "--set-status".to_owned(),
        "old:rejected".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(json["mode"], "pending");
    assert_eq!(json["updated"], true);
    assert_eq!(json["entries"][0]["id"], "new");
    assert_eq!(json["entries"][0]["status"], "pending");
    assert_eq!(json["entries"][1]["id"], "old");
    assert_eq!(json["entries"][1]["status"], "rejected");
    assert_eq!(json["entries"][1]["pinHash"], "h1");
    assert_eq!(json["entries"][1]["pin"], Value::Null);
}

#[test]
fn consent_store_plan_rejects_bad_mode_and_malformed_entry() {
    let bad_mode = run_cli(&["consent-store".to_owned(), "wat".to_owned()]);
    assert_eq!(bad_mode.code, 2);
    assert!(
        bad_mode.stderr.contains("expected trust or pending"),
        "{}",
        bad_mode.stderr
    );

    let bad_entry = run_cli(&[
        "consent-store".to_owned(),
        "trust".to_owned(),
        "--entry".to_owned(),
        "from=a,to=b".to_owned(),
    ]);
    assert_eq!(bad_entry.code, 2);
    assert!(
        bad_entry.stderr.contains("missing action"),
        "{}",
        bad_entry.stderr
    );
}
