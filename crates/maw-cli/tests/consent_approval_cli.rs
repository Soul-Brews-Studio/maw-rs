use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

fn base_args(mode: &str) -> Vec<String> {
    vec![
        "consent-approval".to_owned(),
        mode.to_owned(),
        "--plan-json".to_owned(),
        "--request-id".to_owned(),
        "req-ok".to_owned(),
        "--from".to_owned(),
        "neo".to_owned(),
        "--to".to_owned(),
        "mawjs".to_owned(),
        "--action".to_owned(),
        "hey".to_owned(),
        "--summary".to_owned(),
        "hello".to_owned(),
        "--pin".to_owned(),
        "ABCDEF".to_owned(),
        "--created-at".to_owned(),
        "1767312000000".to_owned(),
        "--now".to_owned(),
        "1767312001000".to_owned(),
    ]
}

#[test]
fn consent_approval_plan_approves_pending_request_and_records_trust() {
    let json = json(&base_args("approve"));

    assert_eq!(json["command"], "consent-approval");
    assert_eq!(json["mode"], "approve");
    assert_eq!(json["ok"], true);
    assert_eq!(json["error"], Value::Null);
    assert_eq!(json["pin"], Value::Null);
    assert_eq!(json["entry"]["from"], "neo");
    assert_eq!(json["entry"]["to"], "mawjs");
    assert_eq!(json["entry"]["action"], "hey");
    assert_eq!(json["entry"]["approvedBy"], "human");
    assert_eq!(json["pendingStatus"], "approved");
    assert_eq!(json["trusted"], true);
}

#[test]
fn consent_approval_plan_reports_pin_mismatch_without_trusting() {
    let mut args = base_args("approve");
    let pin_index = args.iter().position(|arg| arg == "ABCDEF").expect("pin");
    args[pin_index] = "ZZZZZZ".to_owned();

    let json = json(&args);

    assert_eq!(json["ok"], false);
    assert_eq!(json["error"], "PIN mismatch");
    assert_eq!(json["entry"], Value::Null);
    assert_eq!(json["pendingStatus"], "pending");
    assert_eq!(json["trusted"], false);
}

#[test]
fn consent_approval_plan_rejects_pending_request_without_trusting() {
    let json = json(&base_args("reject"));

    assert_eq!(json["mode"], "reject");
    assert_eq!(json["ok"], true);
    assert_eq!(json["entry"], Value::Null);
    assert_eq!(json["pendingStatus"], "rejected");
    assert_eq!(json["trusted"], false);
}

#[test]
fn consent_approval_plan_rejects_bad_mode_and_missing_request() {
    let bad_mode = run_cli(&["consent-approval".to_owned(), "maybe".to_owned()]);
    assert_eq!(bad_mode.code, 2);
    assert!(
        bad_mode.stderr.contains("expected approve or reject"),
        "{}",
        bad_mode.stderr
    );

    let missing = run_cli(&["consent-approval".to_owned(), "approve".to_owned()]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing --request-id value"),
        "{}",
        missing.stderr
    );
}
