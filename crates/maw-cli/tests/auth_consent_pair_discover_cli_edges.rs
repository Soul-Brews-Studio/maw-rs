#![allow(clippy::too_many_lines)]

use maw_cli::{run_cli, CliOutput};

const PENDING_REQ: &str = "id=req-1,from=neo,to=mawjs,action=hey,summary=hello,pin_hash=hash,created_at=2026-01-02T00:00:00.000Z,expires_at=2026-01-02T00:01:00.000Z,status=pending";
const TRUST_ENTRY: &str = "from=neo,to=mawjs,action=hey,approved_at=2026-01-02T00:00:00.000Z,approved_by=auto,request_id=req-1";

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn assert_usage(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
    assert!(
        output.stdout.is_empty(),
        "stdout for {args:?}: {}",
        output.stdout
    );
}

fn assert_text(args: &[&str], expected: &str) {
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
fn auth_and_plugin_invoke_remaining_error_edges_are_covered() {
    assert_usage(
        &[
            "auth",
            "sign-v3",
            "--peer-key",
            "not-hex",
            "--from",
            "",
            "--method",
            "POST",
            "--path",
            "/api/send",
            "--now",
            "1700000000",
        ],
        "fromAddress is required",
    );

    assert_usage(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            ".",
            "--runtime-version",
        ],
        "missing --runtime-version value",
    );
}

#[test]
fn consent_approval_missing_and_invalid_required_edges_are_covered() {
    let base = [
        "consent-approval",
        "approve",
        "--request-id",
        "req-1",
        "--from",
        "neo",
        "--to",
        "mawjs",
        "--action",
        "hey",
        "--summary",
        "hello",
        "--pin",
        "ABCDEF",
        "--created-at",
        "1700000000000",
        "--now",
        "1700000000001",
    ];
    assert_text(&base, "mode=approve");

    assert_usage(
        &[
            "consent-approval",
            "approve",
            "--request-id",
            "req-1",
            "--from",
            "neo",
            "--to",
            "mawjs",
            "--action",
            "hey",
            "--summary",
            "hello",
            "--pin",
            "ABCDEF",
            "--created-at",
            "1700000000000",
            "--now",
            "bad",
        ],
        "--now must be an integer",
    );
    assert_usage(
        &[
            "consent-approval",
            "approve",
            "--request-id",
            "req-1",
            "--from",
            "neo",
            "--to",
            "mawjs",
            "--action",
            "hey",
            "--summary",
            "hello",
            "--pin",
            "ABCDEF",
            "--created-at",
            "1700000000000",
            "--seed-pin",
            "SEED",
        ],
        "missing --now value",
    );
    for (args, expected) in [
        (
            &[
                "consent-approval",
                "approve",
                "--from",
                "neo",
                "--to",
                "mawjs",
                "--action",
                "hey",
                "--summary",
                "hello",
                "--pin",
                "ABCDEF",
                "--created-at",
                "1",
                "--now",
                "1",
            ][..],
            "missing --request-id value",
        ),
        (
            &[
                "consent-approval",
                "approve",
                "--request-id",
                "req-1",
                "--to",
                "mawjs",
                "--action",
                "hey",
                "--summary",
                "hello",
                "--pin",
                "ABCDEF",
                "--created-at",
                "1",
                "--now",
                "1",
            ][..],
            "missing --from value",
        ),
        (
            &[
                "consent-approval",
                "approve",
                "--request-id",
                "req-1",
                "--from",
                "neo",
                "--action",
                "hey",
                "--summary",
                "hello",
                "--pin",
                "ABCDEF",
                "--created-at",
                "1",
                "--now",
                "1",
            ][..],
            "missing --to value",
        ),
        (
            &[
                "consent-approval",
                "approve",
                "--request-id",
                "req-1",
                "--from",
                "neo",
                "--to",
                "mawjs",
                "--summary",
                "hello",
                "--pin",
                "ABCDEF",
                "--created-at",
                "1",
                "--now",
                "1",
            ][..],
            "missing --action value",
        ),
        (
            &[
                "consent-approval",
                "approve",
                "--request-id",
                "req-1",
                "--from",
                "neo",
                "--to",
                "mawjs",
                "--action",
                "hey",
                "--pin",
                "ABCDEF",
                "--created-at",
                "1",
                "--now",
                "1",
            ][..],
            "missing --summary value",
        ),
        (
            &[
                "consent-approval",
                "approve",
                "--request-id",
                "req-1",
                "--from",
                "neo",
                "--to",
                "mawjs",
                "--action",
                "hey",
                "--summary",
                "hello",
                "--created-at",
                "1",
                "--now",
                "1",
            ][..],
            "missing --pin value",
        ),
        (
            &[
                "consent-approval",
                "approve",
                "--request-id",
                "req-1",
                "--from",
                "neo",
                "--to",
                "mawjs",
                "--action",
                "hey",
                "--summary",
                "hello",
                "--pin",
                "ABCDEF",
                "--now",
                "1",
            ][..],
            "missing --created-at value",
        ),
    ] {
        assert_usage(args, expected);
    }
}

#[test]
fn consent_store_and_pending_parse_error_edges_are_covered() {
    assert_usage(
        &["consent-store", "trust", "--entry"],
        "missing --entry value",
    );
    assert_usage(
        &["consent-store", "pending", "--request", "bad"],
        "expected key=value fields",
    );
    assert_usage(
        &["consent-store", "trust", "--key", "bad"],
        "must use from:to:action",
    );
    assert_usage(
        &["consent-expiry", "--request", "bad"],
        "expected key=value fields",
    );
    assert_usage(
        &["consent-cleanup", "--request", "bad"],
        "expected key=value fields",
    );
    assert_usage(
        &["consent-trust-revoke", "--entry", "bad"],
        "expected key=value fields",
    );
    assert_usage(
        &["consent-trust-check", "--entry", "bad"],
        "expected key=value fields",
    );
    assert_usage(
        &["consent-pending-read", "--request", "bad"],
        "expected key=value fields",
    );
    assert_usage(
        &["consent-pending-status", "--request", "bad"],
        "expected key=value fields",
    );

    assert_text(
        &[
            "consent-store",
            "pending",
            "--request",
            PENDING_REQ,
            "--set-status",
            "req-1:approved",
        ],
        "updated=true",
    );
    assert_text(
        &[
            "consent-trust-revoke",
            "--entry",
            TRUST_ENTRY,
            "--revoke",
            "neo:mawjs:hey",
        ],
        "revoked=true",
    );
}

#[test]
fn pair_code_store_consumed_text_and_pair_api_missing_now_are_covered() {
    assert_text(
        &[
            "pair-code-store",
            "consume",
            "--code",
            "ABC-DEF",
            "--now",
            "2000",
            "--seed-code",
            "ABC-DEF:10000:1000",
        ],
        "state=live",
    );
    assert_usage(
        &[
            "pair-api", "probe", "--code", "ABC-DEF", "--node", "m5", "--oracle", "mawjs",
        ],
        "missing --now value",
    );
}

#[test]
fn remaining_discover_route_calver_and_ls_edges_are_covered() {
    assert_usage(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            ".",
            "--runtime-version",
            "2.0.0",
            "--plugin",
            "missing",
        ],
        "plugin 'missing' not found",
    );
    assert_usage(
        &[
            "discover",
            "--pane",
            "%1|zsh|mawjs:1.0|title|not-a-pid|/tmp|bad-last",
        ],
        "pane pid must be an integer",
    );
    assert_usage(
        &[
            "discover",
            "--pane",
            "%1|zsh|mawjs:1.0|title|123|/tmp|bad-last",
        ],
        "pane last_activity must be an integer",
    );
    assert_text(
        &[
            "discover",
            "--plan-json",
            "--plugin",
            "alpha|1.0.0|ts|core|5|false|/plugins/alpha|run-alpha|-|-|-",
            "--oracle",
            "dup|-|-|-|-|-|-|false|false",
            "--oracle",
            "dup|psi|-|-|-|-|-|true|true",
        ],
        "\"name\":\"dup\"",
    );
    let duplicate = run(&[
        "discover",
        "--plan-json",
        "--oracle",
        "dup|-|-|-|-|-|-|false|false",
        "--oracle",
        "dup|psi|-|-|-|-|-|true|true",
    ]);
    assert_eq!(duplicate.code, 0, "{}", duplicate.stderr);
    assert_eq!(duplicate.stdout.matches("\"name\":\"dup\"").count(), 1);

    assert_text(
        &[
            "route",
            "--query",
            "ghost",
            "--node",
            "local",
            "--session",
            "alpha",
            "--source",
            "local",
        ],
        "route ghost: error not_found 'ghost' not in local sessions or agents map hint=check: maw ls\n",
    );
    assert_usage(
        &["calver", "--now", "2026-5-21-1T10:00"],
        "calver: --now date must use YYYY-M-D",
    );
    assert_usage(&["calver", "--now", "T10:00"], "invalid year in --now");

    assert_text(
        &[
            "ls",
            "--active",
            "2h",
            "--recent",
            "1",
            "--verify",
            "--fix",
            "-a",
            "--now",
            "200000",
            "--pane",
            "%1|node|idle-session:1.0|idle|100|/repo|199900",
        ],
        "idle-session",
    );
    assert_usage(&["ls", "--active=2w"], "ls: invalid --active duration");
    assert_text(
        &[
            "ls",
            "--all",
            "--now",
            "200000",
            "--pane",
            "%1|zsh|stale-no-agent:1.0|shell|100|/repo|100000",
        ],
        "stale-no-agent",
    );
}
