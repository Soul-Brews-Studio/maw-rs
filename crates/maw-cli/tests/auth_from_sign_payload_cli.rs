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
fn auth_from_sign_payload_plan_renders_v3_and_legacy_payloads() {
    let v3 = run(&[
        "auth",
        "from-sign-payload",
        "--from",
        "mawjs:m5",
        "--timestamp",
        "1700000000",
        "--method",
        "post",
        "--path",
        "/api/send",
        "--body-hash",
        "230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5",
        "--plan-json",
    ]);
    assert_eq!(v3.code, 0, "stderr: {}", v3.stderr);
    assert_eq!(v3.stderr, "");
    assert!(v3.stdout.contains("\"command\":\"auth\""));
    assert!(v3.stdout.contains("\"kind\":\"from-sign-payload\""));
    assert!(v3.stdout.contains("\"version\":\"v3\""));
    assert!(v3.stdout.contains("\"from\":\"mawjs:m5\""));
    assert!(v3.stdout.contains("\"timestamp\":1700000000"));
    assert!(v3.stdout.contains("\"signedAt\":null"));
    assert!(v3.stdout.contains("\"method\":\"POST\""));
    assert!(v3.stdout.contains("\"path\":\"/api/send\""));
    assert!(v3.stdout.contains(
        "\"bodyHash\":\"230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5\""
    ));
    assert!(v3.stdout.contains(
        "\"payload\":\"POST:/api/send:1700000000:230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5:mawjs:m5\""
    ));

    let legacy = run(&[
        "auth",
        "from-sign-payload",
        "--legacy",
        "--from",
        "mawjs:m5",
        "--signed-at",
        "2023-11-14T22:13:20.000Z",
        "--method",
        "post",
        "--path",
        "/api/send",
        "--body-hash",
        "230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5",
        "--plan-json",
    ]);
    assert_eq!(legacy.code, 0, "stderr: {}", legacy.stderr);
    assert!(legacy.stdout.contains("\"version\":\"legacy\""));
    assert!(legacy.stdout.contains("\"timestamp\":null"));
    assert!(legacy
        .stdout
        .contains("\"signedAt\":\"2023-11-14T22:13:20.000Z\""));
    assert!(legacy.stdout.contains(
        "\"payload\":\"mawjs:m5\\n2023-11-14T22:13:20.000Z\\nPOST\\n/api/send\\n230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5\""
    ));
}

#[test]
fn auth_from_sign_payload_plan_rejects_missing_required_inputs() {
    let missing_v3_time = run(&["auth", "from-sign-payload", "--from", "mawjs:m5"]);
    assert_eq!(missing_v3_time.code, 2);
    assert!(missing_v3_time
        .stderr
        .contains("auth from-sign-payload: --timestamp is required"));
    assert!(missing_v3_time
        .stderr
        .contains("maw-rs auth from-sign-payload"));

    let missing_legacy_time = run(&[
        "auth",
        "from-sign-payload",
        "--legacy",
        "--from",
        "mawjs:m5",
    ]);
    assert_eq!(missing_legacy_time.code, 2);
    assert!(missing_legacy_time
        .stderr
        .contains("auth from-sign-payload: --signed-at is required with --legacy"));
}
