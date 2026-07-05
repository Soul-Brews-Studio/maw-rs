use maw_cli::run_cli;

const PEER_KEY: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";
const FROM: &str = "mawjs:m5";
const SIGNED_AT: &str = "2023-11-14T22:13:20.000Z";
const NOW: &str = "1700000000";
const LEGACY_SIG: &str = "102cca45924d32428c10ff346d99cb13b5892b8c9b6a83da94607e379984ed5d";

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

#[test]
fn auth_verify_legacy_from_plan_maps_header_decisions() {
    let verified = run(&[
        "auth",
        "verify-legacy-from",
        "--cached-pubkey",
        PEER_KEY,
        "--from",
        FROM,
        "--signed-at",
        SIGNED_AT,
        "--signature",
        LEGACY_SIG,
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--body",
        "body",
        "--now",
        NOW,
        "--plan-json",
    ]);
    assert_eq!(verified.code, 0, "stderr: {}", verified.stderr);
    assert_eq!(verified.stderr, "");
    assert!(verified.stdout.contains("\"command\":\"auth\""));
    assert!(verified.stdout.contains("\"kind\":\"verify-legacy-from\""));
    assert!(verified.stdout.contains("\"from\":\"mawjs:m5\""));
    assert!(verified
        .stdout
        .contains("\"signedAt\":\"2023-11-14T22:13:20.000Z\""));
    assert!(verified
        .stdout
        .contains("\"decision\":{\"kind\":\"accept-verified\""));

    let mismatch = run(&[
        "auth",
        "verify-legacy-from",
        "--cached-pubkey",
        PEER_KEY,
        "--from",
        FROM,
        "--signed-at",
        SIGNED_AT,
        "--signature",
        LEGACY_SIG,
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--body",
        "tampered",
        "--now",
        NOW,
        "--plan-json",
    ]);
    assert_eq!(mismatch.code, 0, "stderr: {}", mismatch.stderr);
    assert!(mismatch
        .stdout
        .contains("\"decision\":{\"kind\":\"refuse-mismatch\""));
    assert!(mismatch.stdout.contains("\"reason\":\"signature-invalid\""));

    let tofu = run(&[
        "auth",
        "verify-legacy-from",
        "--from",
        FROM,
        "--signed-at",
        SIGNED_AT,
        "--signature",
        LEGACY_SIG,
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--body",
        "body",
        "--now",
        NOW,
        "--plan-json",
    ]);
    assert_eq!(tofu.code, 0, "stderr: {}", tofu.stderr);
    assert!(tofu
        .stdout
        .contains("\"decision\":{\"kind\":\"accept-tofu-record\""));
}

#[test]
fn auth_verify_legacy_from_plan_rejects_missing_required_inputs() {
    let output = run(&["auth", "verify-legacy-from", "--from", FROM]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth verify-legacy-from: --signed-at is required"));
    assert!(output.stderr.contains("maw-rs auth verify-legacy-from"));
}
