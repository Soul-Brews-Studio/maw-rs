use maw_cli::run_cli;

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

#[test]
fn auth_verify_v1_plan_accepts_current_signature_and_rejects_bad_or_stale() {
    let ok = run(&[
        "auth",
        "verify-v1",
        "--token",
        "0123456789abcdef-federation-token",
        "--method",
        "GET",
        "--path",
        "/api/send",
        "--signed-at",
        "1700000000",
        "--now",
        "1700000299",
        "--signature",
        "a778cbd076e90e0838261643f40865fd127b873c835999ca1c3c37a1b26bb062",
        "--plan-json",
    ]);
    assert_eq!(ok.code, 0, "stderr: {}", ok.stderr);
    assert_eq!(ok.stderr, "");
    assert!(ok.stdout.contains("\"command\":\"auth\""));
    assert!(ok.stdout.contains("\"kind\":\"verify-v1\""));
    assert!(ok.stdout.contains("\"valid\":true"));
    assert!(ok.stdout.contains("\"deltaSec\":299"));
    assert!(ok.stdout.contains("\"windowSec\":300"));
    assert!(ok.stdout.contains("\"reason\":\"ok\""));

    let bad = run(&[
        "auth",
        "verify-v1",
        "--token",
        "0123456789abcdef-federation-token",
        "--method",
        "GET",
        "--path",
        "/api/send",
        "--signed-at",
        "1700000000",
        "--now",
        "1700000299",
        "--signature",
        "deadbeef",
        "--plan-json",
    ]);
    assert_eq!(bad.code, 0, "stderr: {}", bad.stderr);
    assert!(bad.stdout.contains("\"valid\":false"));
    assert!(bad.stdout.contains("\"reason\":\"signature-mismatch\""));

    let stale = run(&[
        "auth",
        "verify-v1",
        "--token",
        "0123456789abcdef-federation-token",
        "--method",
        "GET",
        "--path",
        "/api/send",
        "--signed-at",
        "1700000000",
        "--now",
        "1700000301",
        "--signature",
        "a778cbd076e90e0838261643f40865fd127b873c835999ca1c3c37a1b26bb062",
        "--plan-json",
    ]);
    assert_eq!(stale.code, 0, "stderr: {}", stale.stderr);
    assert!(stale.stdout.contains("\"valid\":false"));
    assert!(stale.stdout.contains("\"deltaSec\":301"));
    assert!(stale
        .stdout
        .contains("\"reason\":\"timestamp-out-of-window\""));
}

#[test]
fn auth_verify_v1_plan_rejects_missing_required_inputs() {
    let output = run(&["auth", "verify-v1", "--token", "secret"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth verify-v1: --signature is required"));
    assert!(output.stderr.contains("maw-rs auth verify-v1"));
}
