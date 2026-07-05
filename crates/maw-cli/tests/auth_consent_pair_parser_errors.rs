use maw_cli::{dispatcher_status, run_cli, CliOutput, DispatchKind};

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn assert_usage_error(args: &[&str], expected: &str, usage: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains(usage),
        "stderr for {args:?} did not contain usage {usage:?}: {}",
        output.stderr
    );
    assert!(
        output.stdout.is_empty(),
        "stdout for {args:?}: {}",
        output.stdout
    );
}

fn assert_ok_text(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stdout.contains(expected),
        "stdout for {args:?} did not contain {expected:?}: {}",
        output.stdout
    );
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
}

#[test]
fn top_level_help_shows_only_live_ported_surfaces() {
    for args in [Vec::<&str>::new(), vec!["help"], vec!["--help"], vec!["-h"]] {
        assert_ok_text(&args, "ported commands:");
        assert_ok_text(&args, "a|attach <target>");
        assert_ok_text(&args, "ls [--compact|-c]");
        assert_ok_text(&args, "portable parity commands are intentionally hidden");
    }

    let output = run(&[]);
    assert!(!output.stdout.contains("auth verify-request"));
    assert!(!output.stdout.contains("consent-request --from <from>"));
    assert!(!output
        .stdout
        .contains("pair-api <generate|probe|accept|status>"));

    assert_eq!(
        dispatcher_status("not-a-real-command"),
        DispatchKind::NativeError
    );
}

#[test]
fn auth_parser_errors_include_usage_for_newer_subcommands() {
    assert_usage_error(
        &["auth"],
        "auth: expected sign-v1",
        "usage: maw-rs auth sign-v1",
    );
    assert_usage_error(
        &["auth", "sign-v3", "--peer-key"],
        "auth: missing --peer-key value",
        "maw-rs auth sign-v3",
    );
    assert_usage_error(
        &["auth", "sign-v3", "--peer-key", "k", "--from"],
        "auth: missing --from value",
        "maw-rs auth sign-v3",
    );
    assert_usage_error(
        &[
            "auth",
            "sign-v3",
            "--peer-key",
            "k",
            "--from",
            "mawjs:m5",
            "--now",
            "soon",
        ],
        "auth: --now must be an integer",
        "maw-rs auth sign-v3",
    );
    assert_usage_error(
        &["auth", "verify-request", "--header"],
        "auth: missing --header value",
        "maw-rs auth verify-request",
    );
    assert_usage_error(
        &["auth", "verify-request", "--header", "not-a-header"],
        "auth verify-request: --header must be key=value",
        "maw-rs auth verify-request",
    );
    assert_usage_error(
        &["auth", "verify-request", "--now", "nan"],
        "auth: --now must be an integer",
        "maw-rs auth verify-request",
    );
    assert_usage_error(
        &["auth", "verify-legacy-from", "--from", "mawjs:m5"],
        "auth verify-legacy-from: --signed-at is required",
        "maw-rs auth verify-legacy-from",
    );
    assert_usage_error(
        &[
            "auth",
            "verify-v3-from",
            "--from",
            "mawjs:m5",
            "--timestamp",
            "bad",
        ],
        "auth verify-v3-from: --timestamp must be an integer",
        "maw-rs auth verify-v3-from",
    );
    assert_usage_error(
        &["auth", "from-sign-payload", "--from", "mawjs:m5"],
        "auth from-sign-payload: --timestamp is required",
        "maw-rs auth from-sign-payload",
    );
}

#[test]
fn consent_parser_errors_include_usage_and_specific_missing_values() {
    assert_usage_error(
        &["consent-request", "--from"],
        "consent-request: missing --from value",
        "usage: maw-rs consent-request",
    );
    assert_usage_error(
        &[
            "consent-request",
            "--from",
            "neo",
            "--to",
            "mawjs",
            "--action",
            "bad",
        ],
        "consent-request: invalid --action value",
        "usage: maw-rs consent-request",
    );
    assert_usage_error(
        &["consent-request", "--peer-http-status", "not-u16"],
        "consent-request: --peer-http-status must be u16",
        "usage: maw-rs consent-request",
    );
    assert_usage_error(
        &["consent-request", "--peer-network-error"],
        "consent-request: missing --peer-network-error value",
        "usage: maw-rs consent-request",
    );
    assert_usage_error(
        &["consent-request", "--unexpected"],
        "consent-request: unknown argument --unexpected",
        "usage: maw-rs consent-request",
    );
    assert_usage_error(
        &["consent-approval"],
        "consent-approval: expected approve or reject",
        "usage: maw-rs consent-approval",
    );
    assert_usage_error(
        &["consent-approval", "maybe"],
        "consent-approval: expected approve or reject",
        "usage: maw-rs consent-approval",
    );
    assert_usage_error(
        &["consent-approval", "approve", "--created-at", "later"],
        "consent-approval: --created-at must be an integer",
        "usage: maw-rs consent-approval",
    );
    assert_usage_error(
        &["consent-approval", "reject", "--seed-pin"],
        "consent-approval: missing --seed-pin value",
        "usage: maw-rs consent-approval",
    );
}

#[test]
fn pair_parser_errors_include_usage_for_shape_and_endpoint_branches() {
    assert_usage_error(
        &["pair-code", "--code"],
        "pair-code: missing --code value",
        "usage: maw-rs pair-code",
    );
    assert_usage_error(
        &["pair-code", "--bytes", ""],
        "pair-code: --bytes must use comma-separated u8 values",
        "usage: maw-rs pair-code",
    );
    assert_usage_error(
        &["pair-code", "--code", "ABC234", "--bytes", "1,2,3"],
        "pair-code: expected exactly one of --code or --bytes",
        "usage: maw-rs pair-code",
    );
    assert_usage_error(
        &["pair-code", "constants", "--bad"],
        "pair-code constants: unknown argument --bad",
        "usage: maw-rs pair-code constants",
    );
    assert_usage_error(
        &["pair-api"],
        "pair-api: expected generate, probe, accept, or status",
        "usage: maw-rs pair-api",
    );
    assert_usage_error(
        &["pair-api", "probe", "--port", "not-u16"],
        "pair-api: --port must be a u16",
        "usage: maw-rs pair-api",
    );
    assert_usage_error(
        &["pair-api", "probe", "--code", "ABC234", "--now", "later"],
        "pair-api: --now must be a non-negative integer",
        "usage: maw-rs pair-api",
    );
    assert_usage_error(
        &[
            "pair-api",
            "generate",
            "--code",
            "ABC234",
            "--now",
            "1",
            "--expires-sec",
            "soon",
        ],
        "pair-api: --expires-sec must be a non-negative integer",
        "usage: maw-rs pair-api",
    );
    assert_usage_error(
        &[
            "pair-api",
            "status",
            "--code",
            "ABC234",
            "--now",
            "1",
            "--seed-code",
            "ABC234:not-ms:0",
        ],
        "pair-api: --seed-code ttl_ms must be a non-negative integer",
        "usage: maw-rs pair-api",
    );
    assert_usage_error(
        &[
            "pair-api",
            "accept",
            "--code",
            "ABC234",
            "--now",
            "1",
            "--seed-accepted",
            "remote",
        ],
        "pair-api: --seed-accepted must be node=url",
        "usage: maw-rs pair-api",
    );
    assert_usage_error(
        &["pair-api", "probe", "--code", "ABC234", "--now", "1"],
        "pair-api: missing --node value",
        "usage: maw-rs pair-api",
    );
}
