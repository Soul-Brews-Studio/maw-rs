use maw_cli::{run_cli, CliOutput};

const PEER_KEY: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";
const FROM: &str = "mawjs:m5";
const NOW: &str = "1700000000";

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn assert_ok_contains(args: &[&str], expected: &str) -> CliOutput {
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
    output
}

fn assert_usage_contains(args: &[&str], expected: &str) {
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

#[test]
#[allow(clippy::too_many_lines)]
fn auth_parser_trailing_value_and_required_edges_are_stable() {
    for (args, expected) in [
        (
            &["auth", "sign-v1", "--path"][..],
            "auth: missing --path value",
        ),
        (&["auth", "sign-v1", "--now"], "auth: missing --now value"),
        (
            &["auth", "sign-v1", "--body-hash"],
            "auth: missing --body-hash value",
        ),
        (
            &["auth", "sign-headers", "--method"],
            "auth: missing --method value",
        ),
        (
            &["auth", "sign-headers", "--path"],
            "auth: missing --path value",
        ),
        (
            &["auth", "sign-headers", "--now"],
            "auth: missing --now value",
        ),
        (
            &["auth", "sign-headers", "--body"],
            "auth: missing --body value",
        ),
        (
            &["auth", "verify-v1", "--token"],
            "auth: missing --token value",
        ),
        (
            &["auth", "verify-v1", "--method"],
            "auth: missing --method value",
        ),
        (
            &["auth", "verify-v1", "--path"],
            "auth: missing --path value",
        ),
        (
            &["auth", "verify-v1", "--signed-at"],
            "auth: missing --signed-at value",
        ),
        (&["auth", "verify-v1", "--now"], "auth: missing --now value"),
        (
            &["auth", "verify-v1", "--signature"],
            "auth: missing --signature value",
        ),
        (
            &["auth", "verify-v1", "--body-hash"],
            "auth: missing --body-hash value",
        ),
        (
            &["auth", "verify-legacy-from", "--cached-pubkey"],
            "auth: missing --cached-pubkey value",
        ),
        (
            &["auth", "verify-legacy-from", "--from"],
            "auth: missing --from value",
        ),
        (
            &["auth", "verify-legacy-from", "--signed-at"],
            "auth: missing --signed-at value",
        ),
        (
            &["auth", "verify-legacy-from", "--signature"],
            "auth: missing --signature value",
        ),
        (
            &["auth", "verify-legacy-from", "--method"],
            "auth: missing --method value",
        ),
        (
            &["auth", "verify-legacy-from", "--path"],
            "auth: missing --path value",
        ),
        (
            &["auth", "verify-legacy-from", "--now"],
            "auth: missing --now value",
        ),
        (
            &["auth", "verify-legacy-from", "--body"],
            "auth: missing --body value",
        ),
        (
            &["auth", "verify-v3-from", "--cached-pubkey"],
            "auth: missing --cached-pubkey value",
        ),
        (
            &["auth", "verify-v3-from", "--from"],
            "auth: missing --from value",
        ),
        (
            &["auth", "verify-v3-from", "--timestamp"],
            "auth: missing --timestamp value",
        ),
        (
            &["auth", "verify-v3-from", "--signature-v3"],
            "auth: missing --signature-v3 value",
        ),
        (
            &["auth", "verify-v3-from", "--method"],
            "auth: missing --method value",
        ),
        (
            &["auth", "verify-v3-from", "--path"],
            "auth: missing --path value",
        ),
        (
            &["auth", "verify-v3-from", "--now"],
            "auth: missing --now value",
        ),
        (
            &["auth", "verify-v3-from", "--body"],
            "auth: missing --body value",
        ),
        (
            &["auth", "from-sign-payload", "--from"],
            "auth: missing --from value",
        ),
        (
            &["auth", "from-sign-payload", "--timestamp"],
            "auth: missing --timestamp value",
        ),
        (
            &["auth", "from-sign-payload", "--signed-at"],
            "auth: missing --signed-at value",
        ),
        (
            &["auth", "from-sign-payload", "--method"],
            "auth: missing --method value",
        ),
        (
            &["auth", "from-sign-payload", "--path"],
            "auth: missing --path value",
        ),
        (
            &["auth", "from-sign-payload", "--body-hash"],
            "auth: missing --body-hash value",
        ),
        (
            &["auth", "hmac-verify", "--secret"],
            "auth: missing --secret value",
        ),
        (
            &["auth", "hmac-verify", "--payload"],
            "auth: missing --payload value",
        ),
        (
            &["auth", "hmac-verify", "--signature"],
            "auth: missing --signature value",
        ),
        (
            &["auth", "hmac-sign", "--secret"],
            "auth: missing --secret value",
        ),
        (
            &["auth", "hmac-sign", "--payload"],
            "auth: missing --payload value",
        ),
        (
            &["auth", "sign-v3", "--peer-key"],
            "auth: missing --peer-key value",
        ),
        (
            &["auth", "sign-v3", "--method"],
            "auth: missing --method value",
        ),
        (&["auth", "sign-v3", "--path"], "auth: missing --path value"),
        (&["auth", "sign-v3", "--now"], "auth: missing --now value"),
        (&["auth", "sign-v3", "--body"], "auth: missing --body value"),
        (
            &["auth", "loopback", "--address"],
            "auth: missing --address value",
        ),
        (
            &["auth", "from-address", "--oracle"],
            "auth: missing --oracle value",
        ),
        (
            &["auth", "from-address", "--node"],
            "auth: missing --node value",
        ),
        (
            &["auth", "hash-body", "--body"],
            "auth: missing --body value",
        ),
        (
            &["auth", "verify-request", "--cached-pubkey"],
            "auth: missing --cached-pubkey value",
        ),
    ] {
        assert_usage_contains(args, expected);
    }

    assert_usage_contains(
        &["auth", "sign-headers", "--token", "tok", "--plan-json"],
        "auth sign-headers: --now is required",
    );
    assert_usage_contains(
        &["auth", "sign-v3", "--peer-key", PEER_KEY],
        "auth sign-v3: --from is required",
    );
}

#[test]
fn auth_parser_optional_values_render_in_plan_json() {
    assert_ok_contains(
        &[
            "auth",
            "sign-v1",
            "--token",
            "tok",
            "--method",
            "POST",
            "--path",
            "/x",
            "--now",
            NOW,
            "--body-hash",
            "abc",
            "--plan-json",
        ],
        "\"path\":\"/x\"",
    );
    assert_ok_contains(
        &[
            "auth",
            "verify-v1",
            "--token",
            "tok",
            "--method",
            "POST",
            "--path",
            "/x",
            "--signed-at",
            "1699999999",
            "--now",
            NOW,
            "--signature",
            "sig",
            "--body-hash",
            "abc",
            "--plan-json",
        ],
        "\"kind\":\"verify-v1\"",
    );
    assert_ok_contains(
        &[
            "auth",
            "verify-request",
            "--method",
            "POST",
            "--path",
            "/x",
            "--now",
            NOW,
            "--body",
            "body",
            "--cached-pubkey",
            PEER_KEY,
            "--header",
            "X-Maw-From=mawjs:m5",
            "--plan-json",
        ],
        "\"kind\":\"verify-request\"",
    );
    assert_ok_contains(
        &[
            "auth",
            "sign-v3",
            "--peer-key",
            PEER_KEY,
            "--from",
            FROM,
            "--method",
            "POST",
            "--path",
            "/x",
            "--now",
            NOW,
            "--body",
            "body",
            "--plan-json",
        ],
        "\"bodyHash\"",
    );
}

#[test]
fn calver_normalize_pair_api_and_ls_edge_parsers_are_stable() {
    for (now, expected) in [
        ("2026-4-30", "calver: --now must use YYYY-M-DTHH:MM"),
        ("bad-4-30T9:37", "calver: invalid year in --now"),
        ("2026-4T9:37", "calver: missing day in --now"),
        ("2026-4-30T9", "calver: missing minute in --now"),
        ("2026-4-30T9:37:1", "calver: --now time must use HH:MM"),
        (
            "2026-13-30T9:37",
            "calver: --now contains out-of-range date/time parts",
        ),
    ] {
        assert_eq!(run(&["calver", "--now", now]).code, 2, "{now}");
        assert_usage_contains(&["calver", "--now", now], expected);
    }

    assert_ok_contains(
        &["normalize", "constants", "--plan-json"],
        "\"kind\":\"constants\"",
    );
    assert_usage_contains(
        &[
            "pair-api",
            "probe",
            "--code",
            "ABCDEF",
            "--now",
            "1",
            "--node",
            "m5",
            "--oracle",
            "mawjs",
            "--port",
            "8787",
            "--base-url",
            "http://127.0.0.1:8787",
            "--federation-token",
            "token",
            "--pubkey",
            "pub",
            "--seed-code",
            "ABCDEF:10:nope",
        ],
        "pair-api: --seed-code created_at_ms must be a non-negative integer",
    );

    assert_usage_contains(&["ls", "--active=0"], "ls: invalid --active duration");
    assert_usage_contains(&["ls", "--pane"], "ls: missing --pane value");
    assert_usage_contains(&["ls", "--now", "soon"], "ls: --now must be an integer");
    assert_usage_contains(
        &["ls", "--session-created", "mawjs"],
        "ls: --session-created must use <session=epoch_seconds>",
    );
    assert_usage_contains(
        &["ls", "--session-created", "mawjs=soon"],
        "ls: session-created epoch must be an integer",
    );
    assert_ok_contains(
        &[
            "ls",
            "remote-peer",
            "--json",
            "--now",
            "1000",
            "--pane",
            "%1|codex|remote-peer:1.0|agent|100|/repo|990",
            "--pane",
            "%2|zsh|other:1.0|other|101|/repo|990",
        ],
        "\"session\":\"remote-peer\"",
    );
    assert_ok_contains(
        &[
            "ls",
            "--compact",
            "--channels",
            "--json",
            "--now",
            "1000",
            "--pane",
            "%1|node|50-discord:1.0|chat|100|/repo|990",
        ],
        "\"session\":\"50-discord\"",
    );
    assert_ok_contains(
        &[
            "ls",
            "-c",
            "--all",
            "--json",
            "--now",
            "1000",
            "--pane",
            "%1|zsh|plain-session:1.0|shell|100|/repo|900",
        ],
        "\"mode\":\"compact\"",
    );
}

#[test]
fn discover_inventory_text_and_join_edges_are_stable() {
    let output = assert_ok_contains(
        &[
            "discover", "--peers", "config", "--named-peer", "oracle-node=http://node:3456",
            "--named-peer", "window-peer=http://window:3456", "--agent", "oracle-window=agent-node",
            "--fleet", "fleet.json|7|registered-name|sess|oracle-window|owner/repo",
            "--ghq", "/workspace/owner/repo", "--oracle",
            "registered-name|fleet+manifest|oracle-node|sess|oracle-window|owner/repo|/workspace/owner/repo|false|true",
        ],
        "registered oracles",
    );
    assert!(
        output.stdout.contains("registered-name offline"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("fleet config"), "{}", output.stdout);
    assert!(
        output.stdout.contains("repo /workspace/owner/repo"),
        "{}",
        output.stdout
    );
}
