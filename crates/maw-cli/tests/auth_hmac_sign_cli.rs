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
fn auth_hmac_sign_plan_returns_deterministic_signature_without_echoing_secret() {
    let output = run(&[
        "auth",
        "hmac-sign",
        "--secret",
        "peer-secret",
        "--payload",
        PAYLOAD,
        "--plan-json",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"auth\""));
    assert!(output.stdout.contains("\"kind\":\"hmac-sign\""));
    assert!(output.stdout.contains("\"payloadLength\":99"));
    assert!(output
        .stdout
        .contains(&format!("\"signature\":\"{SIGNATURE}\"")));
    assert!(!output.stdout.contains("peer-secret"));
}

#[test]
fn auth_hmac_sign_plan_rejects_missing_required_inputs() {
    let output = run(&["auth", "hmac-sign", "--secret", "peer-secret"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth hmac-sign: --payload is required"));
    assert!(output.stderr.contains("maw-rs auth hmac-sign"));
}
