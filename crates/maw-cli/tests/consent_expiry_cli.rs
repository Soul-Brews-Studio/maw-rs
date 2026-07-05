use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

#[test]
fn consent_expiry_plan_expires_only_after_deadline() {
    let expired = json(&[
        "consent-expiry".to_owned(),
        "--request".to_owned(),
        "id=req-1,from=a,to=b,action=hey,summary=hello,pin_hash=h1,created_at=1970-01-01T00:00:00.000Z,expires_at=1970-01-01T00:00:01.000Z,status=pending".to_owned(),
        "--now".to_owned(),
        "1001".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(expired["command"], "consent-expiry");
    assert_eq!(expired["expired"], true);
    assert_eq!(expired["before"]["status"], "pending");
    assert_eq!(expired["after"]["status"], "expired");
    assert_eq!(expired["after"]["pin"], Value::Null);

    let still_pending = json(&[
        "consent-expiry".to_owned(),
        "--request".to_owned(),
        "id=req-1,from=a,to=b,action=hey,summary=hello,pin_hash=h1,created_at=1970-01-01T00:00:00.000Z,expires_at=1970-01-01T00:00:01.000Z,status=pending".to_owned(),
        "--now".to_owned(),
        "1000".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(still_pending["expired"], false);
    assert_eq!(still_pending["after"]["status"], "pending");
}

#[test]
fn consent_expiry_plan_preserves_terminal_status_and_rejects_missing_inputs() {
    let approved = json(&[
        "consent-expiry".to_owned(),
        "--request".to_owned(),
        "id=req-2,from=a,to=b,action=hey,summary=hello,pin_hash=h1,created_at=1970-01-01T00:00:00.000Z,expires_at=1970-01-01T00:00:01.000Z,status=approved".to_owned(),
        "--now".to_owned(),
        "1001".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(approved["expired"], false);
    assert_eq!(approved["after"]["status"], "approved");

    let missing = run_cli(&[
        "consent-expiry".to_owned(),
        "--now".to_owned(),
        "1001".to_owned(),
    ]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing --request"),
        "{}",
        missing.stderr
    );
}
