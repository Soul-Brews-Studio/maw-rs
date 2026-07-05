use maw_cli::run_cli;

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

const PAYLOAD: &str = "POST:/api/send:1700000000:230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5:mawjs:m5";
const SIGNATURE: &str = "7f6e02fac8aaa8b55f83a25cd80ceefb3cf1595c68714fb0f8f6a9106a88e1de";

#[test]
fn auth_hmac_verify_plan_reports_ok_mismatch_and_malformed_signature() {
    let ok = run(&[
        "auth",
        "hmac-verify",
        "--secret",
        "peer-secret",
        "--payload",
        PAYLOAD,
        "--signature",
        SIGNATURE,
        "--plan-json",
    ]);
    assert_eq!(ok.code, 0, "stderr: {}", ok.stderr);
    assert_eq!(ok.stderr, "");
    assert!(ok.stdout.contains("\"command\":\"auth\""));
    assert!(ok.stdout.contains("\"kind\":\"hmac-verify\""));
    assert!(ok.stdout.contains("\"valid\":true"));
    assert!(ok.stdout.contains("\"reason\":\"ok\""));
    assert!(ok.stdout.contains("\"payloadLength\":99"));

    let mismatch = run(&[
        "auth",
        "hmac-verify",
        "--secret",
        "wrong-secret",
        "--payload",
        PAYLOAD,
        "--signature",
        SIGNATURE,
        "--plan-json",
    ]);
    assert_eq!(mismatch.code, 0, "stderr: {}", mismatch.stderr);
    assert!(mismatch.stdout.contains("\"valid\":false"));
    assert!(mismatch
        .stdout
        .contains("\"reason\":\"signature-mismatch\""));

    let malformed = run(&[
        "auth",
        "hmac-verify",
        "--secret",
        "peer-secret",
        "--payload",
        PAYLOAD,
        "--signature",
        "not-hex",
        "--plan-json",
    ]);
    assert_eq!(malformed.code, 0, "stderr: {}", malformed.stderr);
    assert!(malformed.stdout.contains("\"valid\":false"));
    assert!(malformed
        .stdout
        .contains("\"reason\":\"signature-malformed\""));
}

#[test]
fn auth_hmac_verify_plan_rejects_missing_required_inputs() {
    let output = run(&["auth", "hmac-verify", "--secret", "peer-secret"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth hmac-verify: --payload is required"));
    assert!(output.stderr.contains("maw-rs auth hmac-verify"));
}
