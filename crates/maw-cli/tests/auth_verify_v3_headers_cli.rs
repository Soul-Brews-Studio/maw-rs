use maw_cli::run_cli;

const PEER_KEY: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";
const FROM: &str = "mawjs:m5";
const SIGNED_AT: &str = "1700000000";
const SIG_V3: &str = "64763294c027805cb7c9d5f52641c73cd20d3e7643133402f29c1c175a803435";

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

#[test]
fn auth_verify_v3_from_plan_maps_header_decisions() {
    let verified = run(&[
        "auth",
        "verify-v3-from",
        "--cached-pubkey",
        PEER_KEY,
        "--from",
        FROM,
        "--timestamp",
        SIGNED_AT,
        "--signature-v3",
        SIG_V3,
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--body",
        "body",
        "--now",
        SIGNED_AT,
        "--plan-json",
    ]);
    assert_eq!(verified.code, 0, "stderr: {}", verified.stderr);
    assert_eq!(verified.stderr, "");
    assert!(verified.stdout.contains("\"command\":\"auth\""));
    assert!(verified.stdout.contains("\"kind\":\"verify-v3-from\""));
    assert!(verified.stdout.contains("\"from\":\"mawjs:m5\""));
    assert!(verified.stdout.contains("\"timestamp\":1700000000"));
    assert!(verified
        .stdout
        .contains("\"decision\":{\"kind\":\"accept-verified\""));

    let skew = run(&[
        "auth",
        "verify-v3-from",
        "--cached-pubkey",
        PEER_KEY,
        "--from",
        FROM,
        "--timestamp",
        SIGNED_AT,
        "--signature-v3",
        SIG_V3,
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--body",
        "body",
        "--now",
        "1700000301",
        "--plan-json",
    ]);
    assert_eq!(skew.code, 0, "stderr: {}", skew.stderr);
    assert!(skew
        .stdout
        .contains("\"decision\":{\"kind\":\"refuse-skew\""));
    assert!(skew
        .stdout
        .contains("\"reason\":\"timestamp-out-of-window\""));
    assert!(skew.stdout.contains("\"delta\":301"));

    let tofu = run(&[
        "auth",
        "verify-v3-from",
        "--from",
        FROM,
        "--timestamp",
        SIGNED_AT,
        "--signature-v3",
        SIG_V3,
        "--method",
        "POST",
        "--path",
        "/api/send",
        "--body",
        "body",
        "--now",
        SIGNED_AT,
        "--plan-json",
    ]);
    assert_eq!(tofu.code, 0, "stderr: {}", tofu.stderr);
    assert!(tofu
        .stdout
        .contains("\"decision\":{\"kind\":\"accept-tofu-record\""));
}

#[test]
fn auth_verify_v3_from_plan_rejects_missing_required_inputs() {
    let output = run(&["auth", "verify-v3-from", "--from", FROM]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth verify-v3-from: --timestamp is required"));
    assert!(output.stderr.contains("maw-rs auth verify-v3-from"));
}
