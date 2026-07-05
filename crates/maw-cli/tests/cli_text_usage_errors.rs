use maw_cli::{dispatcher_status, run_cli, CliOutput, DispatchKind};

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn assert_ok_text(args: &[&str], contains: &str) {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stdout.contains(contains),
        "stdout for {args:?} did not contain {contains:?}: {}",
        output.stdout
    );
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
}

fn assert_usage_error(args: &[&str], contains: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(contains),
        "stderr for {args:?} did not contain {contains:?}: {}",
        output.stderr
    );
}

#[test]
fn top_level_help_and_unknown_command_branches_are_covered() {
    assert_ok_text(&[], "usage: maw-rs");
    assert_ok_text(&["help"], "usage: maw-rs");
    assert_ok_text(&["--help"], "usage: maw-rs");
    assert_ok_text(&["-h"], "usage: maw-rs");
    assert_eq!(
        dispatcher_status("definitely-not-a-command"),
        DispatchKind::NativeError
    );
}

#[test]
fn text_rendering_branches_for_existing_plan_surfaces_are_covered() {
    assert_ok_text(
        &[
            "auto-wake",
            "neo",
            "--site",
            "view",
            "--not-live",
            "--fleet-known",
        ],
        "auto-wake neo wake=true",
    );
    assert_ok_text(&["auto-wake", "constants"], "auto-wake constants sites=");
    assert_ok_text(&["auth", "loopback", "--address", "127.0.0.1"], "true");
    assert_ok_text(&["auth", "from-address", "--node", "m5"], "mawjs:m5");
    assert_ok_text(
        &["auth", "hash-body", "--body", "hello"],
        "2cf24dba5fb0a30e",
    );
    assert_ok_text(&["auth", "constants"], "defaultOracle=mawjs");
    assert_ok_text(
        &["auth", "hmac-sign", "--secret", "s", "--payload", "p"],
        "",
    );
    assert_ok_text(
        &[
            "auth",
            "hmac-verify",
            "--secret",
            "s",
            "--payload",
            "p",
            "--signature",
            "bad",
        ],
        "signature-mismatch",
    );
    assert_ok_text(&["auth", "sign-v1", "--token", "t", "--now", "123"], "");
    assert_ok_text(
        &["auth", "sign-headers", "--token", "t", "--now", "123"],
        "X-Maw-Signature",
    );
    assert_ok_text(
        &[
            "auth",
            "verify-v1",
            "--token",
            "t",
            "--signed-at",
            "1",
            "--now",
            "999",
            "--signature",
            "deadbeef",
        ],
        "timestamp-out-of-window",
    );
    assert_ok_text(
        &[
            "auth",
            "from-sign-payload",
            "--from",
            "mawjs:m5",
            "--timestamp",
            "123",
            "--method",
            "post",
            "--path",
            "/api",
            "--body-hash",
            "abc",
        ],
        "POST:/api:123:abc:mawjs:m5",
    );
}

#[test]
fn parser_error_branches_for_common_auth_and_auto_wake_flags_are_covered() {
    assert_usage_error(&["auto-wake", "neo", "--site"], "missing --site value");
    assert_usage_error(
        &["auto-wake", "neo", "--site", "bogus"],
        "invalid --site value",
    );
    assert_usage_error(
        &["auto-wake", "neo", "--manifest-source"],
        "missing --manifest-source value",
    );
    assert_usage_error(
        &["auto-wake", "neo", "--manifest-live", "maybe"],
        "--manifest-live must be true or false",
    );
    assert_usage_error(&["auto-wake", "neo", "extra"], "target already provided");
    assert_usage_error(&["auto-wake"], "missing target");
    assert_usage_error(
        &["auto-wake", "constants", "--bogus"],
        "unknown arg --bogus",
    );

    assert_usage_error(&["auth"], "auth: expected");
    assert_usage_error(&["auth", "bogus"], "unknown subcommand bogus");
    assert_usage_error(&["auth", "sign-v1", "--token"], "missing --token value");
    assert_usage_error(
        &["auth", "sign-v1", "--token", "t", "--now", "not-int"],
        "--now must be an integer",
    );
    assert_usage_error(&["auth", "sign-headers", "--wat"], "unknown argument --wat");
    assert_usage_error(
        &["auth", "verify-v1", "--signature", "sig"],
        "--token is required",
    );
    assert_usage_error(
        &[
            "auth",
            "from-sign-payload",
            "--legacy",
            "--from",
            "mawjs:m5",
            "--body-hash",
            "abc",
        ],
        "--signed-at is required",
    );
    assert_usage_error(
        &["auth", "hmac-sign", "--payload", "p"],
        "--secret is required",
    );
    assert_usage_error(
        &["auth", "hmac-verify", "--secret", "s", "--payload", "p"],
        "--signature is required",
    );
    assert_usage_error(&["auth", "constants", "--bad"], "unknown argument --bad");
    assert_usage_error(&["auth", "hash-body", "--body"], "missing --body value");
    assert_usage_error(
        &["auth", "from-address", "--oracle", "mawjs"],
        "--node is required",
    );
    assert_usage_error(&["auth", "loopback"], "address is required");
}
