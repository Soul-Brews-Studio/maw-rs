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
fn auth_sign_v1_plan_matches_legacy_and_body_hash_payload_shapes() {
    let legacy = run(&[
        "auth",
        "sign-v1",
        "--token",
        "0123456789abcdef-federation-token",
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--now",
        "1700000000",
        "--plan-json",
    ]);
    assert_eq!(legacy.code, 0, "stderr: {}", legacy.stderr);
    assert_eq!(legacy.stderr, "");
    assert!(legacy.stdout.contains("\"command\":\"auth\""));
    assert!(legacy.stdout.contains("\"kind\":\"sign-v1\""));
    assert!(legacy.stdout.contains("\"method\":\"POST\""));
    assert!(legacy.stdout.contains("\"path\":\"/api/send\""));
    assert!(legacy.stdout.contains("\"timestamp\":1700000000"));
    assert!(legacy.stdout.contains("\"bodyHash\":\"\""));
    assert!(legacy.stdout.contains("\"signature\":\""));

    let with_body_hash = run(&[
        "auth",
        "sign-v1",
        "--token",
        "0123456789abcdef-federation-token",
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--now",
        "1700000000",
        "--body-hash",
        "230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5",
        "--plan-json",
    ]);
    assert_eq!(with_body_hash.code, 0, "stderr: {}", with_body_hash.stderr);
    assert!(with_body_hash.stdout.contains(
        "\"bodyHash\":\"230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5\""
    ));
    assert_ne!(legacy.stdout, with_body_hash.stdout);
}

#[test]
fn auth_sign_v1_plan_rejects_missing_required_inputs() {
    let output = run(&["auth", "sign-v1", "--token", "secret"]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("auth sign-v1: --now is required"));
    assert!(output.stderr.contains("maw-rs auth sign-v1"));
}
