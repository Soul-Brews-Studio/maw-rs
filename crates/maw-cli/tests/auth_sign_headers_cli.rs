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
fn auth_sign_headers_plan_renders_legacy_and_v2_headers() {
    let legacy = run(&[
        "auth",
        "sign-headers",
        "--token",
        "0123456789abcdef-federation-token",
        "--method",
        "GET",
        "--path",
        "/api/send",
        "--now",
        "1700000000",
        "--plan-json",
    ]);
    assert_eq!(legacy.code, 0, "stderr: {}", legacy.stderr);
    assert_eq!(legacy.stderr, "");
    assert!(legacy.stdout.contains("\"command\":\"auth\""));
    assert!(legacy.stdout.contains("\"kind\":\"sign-headers\""));
    assert!(legacy.stdout.contains("\"bodyHash\":\"\""));
    assert!(legacy.stdout.contains("\"X-Maw-Timestamp\":\"1700000000\""));
    assert!(legacy.stdout.contains("\"X-Maw-Signature\":\""));
    assert!(!legacy.stdout.contains("X-Maw-Auth-Version"));

    let v2 = run(&[
        "auth",
        "sign-headers",
        "--token",
        "0123456789abcdef-federation-token",
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--now",
        "1700000000",
        "--body",
        "body",
        "--plan-json",
    ]);
    assert_eq!(v2.code, 0, "stderr: {}", v2.stderr);
    assert!(v2.stdout.contains(
        "\"bodyHash\":\"230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5\""
    ));
    assert!(v2.stdout.contains("\"X-Maw-Auth-Version\":\"v2\""));
    assert_ne!(legacy.stdout, v2.stdout);
}

#[test]
fn auth_sign_headers_plan_rejects_missing_required_inputs() {
    let output = run(&["auth", "sign-headers", "--token", "secret"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth sign-headers: --now is required"));
    assert!(output.stderr.contains("maw-rs auth sign-headers"));
}
