use maw_cli::run_cli;

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

const REQUEST_ONE: &str = "id=req-1,from=alice:m5,to=bob:m5,action=hey,summary=hello,pin_hash=hash-one,created_at=100,expires_at=200,status=pending";
const REQUEST_TWO: &str = "id=req-2,from=alice:m5,to=bob:m5,action=plugin-install,summary=plugin,pin_hash=hash-two,created_at=300,expires_at=400,status=pending";

#[test]
fn consent_pending_status_plan_updates_matching_request_and_keeps_redacted_order() {
    let output = run(&[
        "consent-pending-status",
        "--request",
        REQUEST_ONE,
        "--request",
        REQUEST_TWO,
        "--set-status",
        "req-1:rejected",
        "--plan-json",
    ]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output
        .stdout
        .contains("\"command\":\"consent-pending-status\""));
    assert!(output.stdout.contains("\"id\":\"req-1\""));
    assert!(output.stdout.contains("\"updated\":true"));
    assert!(output.stdout.contains("\"status\":\"rejected\""));
    let entries = output.stdout.split("\"entries\":").nth(1).unwrap();
    assert!(entries.find("req-2").unwrap() < entries.find("req-1").unwrap());
    assert!(!output.stdout.contains("\"pin\""));
    assert!(!output.stdout.contains("123456"));
}

#[test]
fn consent_pending_status_plan_reports_missing_id_and_rejects_bad_status_update() {
    let missing = run(&[
        "consent-pending-status",
        "--request",
        REQUEST_ONE,
        "--set-status",
        "missing:approved",
        "--plan-json",
    ]);

    assert_eq!(missing.code, 0, "stderr: {}", missing.stderr);
    assert!(missing.stdout.contains("\"id\":\"missing\""));
    assert!(missing.stdout.contains("\"updated\":false"));
    assert!(missing.stdout.contains("\"request\":null"));

    let bad = run(&[
        "consent-pending-status",
        "--set-status",
        "req-1:not-a-status",
    ]);
    assert_eq!(bad.code, 2);
    assert!(bad.stderr.contains("consent-store: invalid status"));
    assert!(bad.stderr.contains("usage: maw-rs consent-pending-status"));
}
