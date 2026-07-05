use maw_cli::{run_cli, CliOutput};
use serde_json::Value;

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

fn assert_ok(args: &[&str]) -> CliOutput {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
    output
}

fn assert_json(args: &[&str]) -> Value {
    let output = assert_ok(args);
    serde_json::from_str(&output.stdout).unwrap_or_else(|err| {
        panic!("invalid json for {args:?}: {err}\n{}", output.stdout);
    })
}

#[test]
fn auth_tail_b_parser_required_value_and_success_edges_are_stable() {
    assert_usage(
        &["auth", "sign-v1", "--token", "peer"],
        "auth sign-v1: --now is required",
    );
    assert_usage(
        &["auth", "sign-v1", "--token", "peer", "--body-hash"],
        "auth: missing --body-hash value",
    );
    assert_usage(
        &[
            "auth",
            "sign-headers",
            "--token",
            "peer",
            "--now",
            "1700000000",
            "--body",
        ],
        "auth: missing --body value",
    );
    assert_usage(
        &["auth", "verify-v1", "--token", "peer", "--signature", "sig"],
        "auth verify-v1: --signed-at is required",
    );
    assert_usage(
        &[
            "auth",
            "verify-legacy-from",
            "--from",
            "mawjs:m5",
            "--signed-at",
            "1700000000",
            "--signature",
            "sig",
        ],
        "auth verify-legacy-from: --now is required",
    );
    assert_usage(
        &[
            "auth",
            "verify-v3-from",
            "--from",
            "mawjs:m5",
            "--timestamp",
            "1700000000",
            "--signature-v3",
            "sig",
        ],
        "auth verify-v3-from: --now is required",
    );

    let hmac = assert_json(&[
        "auth",
        "hmac-sign",
        "--secret",
        "peer-secret",
        "--payload",
        "hello",
        "--plan-json",
    ]);
    assert_eq!(hmac["kind"], "hmac-sign");
    assert_eq!(hmac["signature"].as_str().expect("signature").len(), 64);
}

#[test]
fn discover_tail_b_parser_and_nested_worktree_edges_are_stable() {
    assert_usage(
        &["discover", "--oracle", "neo|-|-|-|-|-|-|maybe|false"],
        "discover: oracle has_psi must be true or false",
    );
    assert_usage(
        &["discover", "--oracle", "neo|-|-|-|-|-|-|true|maybe"],
        "discover: oracle has_fleet_config must be true or false",
    );
    assert_usage(
        &[
            "discover",
            "--plugin",
            "buddy|1.0.0|ts|standard|heavy|false|/plugins/buddy|buddy|-|-|-",
        ],
        "discover: plugin weight must be an integer",
    );

    let output = assert_json(&[
        "discover",
        "--oracle",
        "nested|manifest+fleet|white|101-mawjs|mawjs|Soul/nested|/opt/Code/github.com/Soul/nested/agents/3-feature|true|false",
        "--ghq",
        "/opt/Code/github.com/Soul/nested/agents/3-feature",
        "--json",
        "--tree",
        "--plan-json",
    ]);
    assert_eq!(output["ok"], true);
    assert_eq!(output["oracles"]["records"][0]["name"], "nested");
    assert_eq!(
        output["oracles"]["records"][0]["sources"],
        serde_json::json!(["manifest", "fleet"])
    );
    assert_eq!(output["oracles"]["records"][0]["worktree"], true);
    assert_eq!(output["ghq"]["repos"][0]["worktree"], true);
}

#[test]
fn worktree_calver_normalize_resolve_tail_b_edges_are_stable() {
    assert_usage(
        &[
            "worktree-window",
            "--main-repo-name",
            "mawjs-oracle",
            "--wt-name",
            "1-feature",
            "--session",
            "mawjs",
            "--window",
            "x:mawjs:false",
        ],
        "worktree-window: invalid window index",
    );
    assert_usage(
        &[
            "worktree-window",
            "--main-repo-name",
            "mawjs-oracle",
            "--wt-name",
            "1-feature",
            "--session",
            "mawjs",
            "--window",
            "1:mawjs:maybe",
        ],
        "worktree-window: window active must be true or false",
    );
    let bound = assert_json(&[
        "worktree-window",
        "--main-repo-name",
        "mawjs-oracle",
        "--wt-name",
        "1-feature",
        "--session",
        "mawjs-oracle",
        "--window",
        "1:mawjs-feature:true",
        "--plan-json",
    ]);
    assert_eq!(bound["kind"], "bound");
    assert_eq!(bound["window"], "mawjs-feature");

    for (args, expected) in [
        (
            &["calver", "--now", "x-5-21T10:00"][..],
            "calver: invalid year in --now",
        ),
        (
            &["calver", "--now", "2026-x-21T10:00"][..],
            "calver: invalid month in --now",
        ),
        (
            &["calver", "--now", "2026-5-xT10:00"][..],
            "calver: invalid day in --now",
        ),
        (
            &["calver", "--now", "2026-5-21Tx:00"][..],
            "calver: invalid hour in --now",
        ),
        (
            &["calver", "--now", "2026-5-21T10:x"][..],
            "calver: invalid minute in --now",
        ),
        (
            &["calver", "--now", "2026-5-21-1T10:00"][..],
            "calver: --now date must use YYYY-M-D",
        ),
        (
            &["calver", "--now", "2026-13-21T10:00"][..],
            "calver: --now contains out-of-range date/time parts",
        ),
        (
            &["calver", "constants", "--bad"][..],
            "calver constants: unknown argument --bad",
        ),
        (
            &["normalize", "constants", "--bad"][..],
            "normalize constants: unknown argument --bad",
        ),
        (
            &["resolve", "constants", "--bad"][..],
            "resolve constants: unknown argument --bad",
        ),
        (&["resolve", "--mode"][..], "resolve: missing --mode value"),
        (
            &["resolve", "--mode", "unknown", "neo", "neo"][..],
            "resolve: unknown --mode",
        ),
    ] {
        assert_usage(args, expected);
    }

    let normalized = assert_ok(&["normalize", "  /repo/example.git///  "]);
    assert_eq!(normalized.stdout, "/repo/example.git\n");
    let none = assert_ok(&["resolve", "--mode", "by-name", "", "alpha"]);
    assert_eq!(none.stdout, "resolve by-name : none\n");
}

#[test]
fn ls_and_bring_tail_b_render_and_filter_edges_are_stable() {
    let default_compact = assert_ok(&[
        "ls",
        "--now",
        "1700000000",
        "--pane",
        "%1|bash|neo-oracle:1.0|oracle|100|/repo|1699999999",
        "--pane",
        "%2|bash|general-discord:1.0|channel|101|/repo|1699999999",
        "--pane",
        "%3|bash|discord-admin:1.0|admin|102|/repo|1699999999",
    ]);
    assert!(
        default_compact.stdout.contains("neo-oracle"),
        "{}",
        default_compact.stdout
    );
    assert!(
        !default_compact.stdout.contains("general-discord"),
        "{}",
        default_compact.stdout
    );
    assert!(
        default_compact.stdout.contains("discord-admin"),
        "{}",
        default_compact.stdout
    );

    let all_without_channels = assert_json(&[
        "ls",
        "--all",
        "--json",
        "--now",
        "1700000000",
        "--pane",
        "%2|bash|general-discord:1.0|channel|101|/repo|1699999999",
        "--pane",
        "%3|bash|discord-admin:1.0|admin|102|/repo|1699999999",
    ]);
    assert_eq!(
        all_without_channels["sessions"][0]["session"],
        "discord-admin"
    );

    let channels = assert_json(&[
        "ls",
        "--all",
        "--channels",
        "--json",
        "--now",
        "1700000000",
        "--pane",
        "%2|bash|general-discord:1.0|channel|101|/repo|1699999999",
    ]);
    assert_eq!(channels["sessions"][0]["session"], "general-discord");

    let verbose = assert_ok(&[
        "ls",
        "--all",
        "--verbose",
        "--now",
        "200000",
        "--pane",
        "%4|bash|53-mawjs:1.0|days|103|/repo|100000",
    ]);
    assert!(verbose.stdout.contains("1d"), "{}", verbose.stdout);

    assert_usage(&["bring", "--tab"], "bring: missing oracle name");
    let minimal = assert_ok(&["bring", "neo"]);
    assert_eq!(minimal.stdout, "wake neo --split\n");
}

#[test]
fn auth_tail_b_success_region_edges_are_stable() {
    let sign_v1 = assert_json(&[
        "auth",
        "sign-v1",
        "--token",
        "peer-token",
        "--method",
        "POST",
        "--path",
        "/api/oracle",
        "--now",
        "1700000000",
        "--body-hash",
        "abc123",
        "--plan-json",
    ]);
    assert_eq!(sign_v1["kind"], "sign-v1");
    assert_eq!(sign_v1["method"], "POST");
    assert_eq!(sign_v1["path"], "/api/oracle");
    assert_eq!(sign_v1["bodyHash"], "abc123");

    let headers = assert_json(&[
        "auth",
        "sign-headers",
        "--token",
        "peer-token",
        "--method",
        "PATCH",
        "--path",
        "/api/patch",
        "--now",
        "1700000001",
        "--body",
        "payload-body",
        "--plan-json",
    ]);
    assert_eq!(headers["kind"], "sign-headers");
    assert_eq!(headers["method"], "PATCH");
    assert_eq!(headers["path"], "/api/patch");

    let verify_v1 = assert_json(&[
        "auth",
        "verify-v1",
        "--token",
        "peer-token",
        "--method",
        "POST",
        "--path",
        "/api/oracle",
        "--signature",
        "bad-signature",
        "--signed-at",
        "1700000000",
        "--now",
        "1700000001",
        "--plan-json",
    ]);
    assert_eq!(verify_v1["kind"], "verify-v1");
    assert_eq!(verify_v1["valid"], false);

    let payload = assert_json(&[
        "auth",
        "from-sign-payload",
        "--from",
        "mawjs:m5",
        "--legacy",
        "--signed-at",
        "1700000000",
        "--method",
        "PUT",
        "--path",
        "/legacy",
        "--body-hash",
        "bodyhash",
        "--plan-json",
    ]);
    assert_eq!(payload["kind"], "from-sign-payload");
    assert_eq!(payload["version"], "legacy");
}

#[test]
fn discover_ls_bring_tail_b_remaining_region_edges_are_stable() {
    assert_usage(
        &["discover", "--oracle", "bad"],
        "discover: --oracle must use",
    );
    let discover = assert_json(&[
        "discover",
        "--named-peer",
        "window-node=http://window-node:3456",
        "--agent",
        "agent-window=window-node",
        "--fleet",
        "fleet.toml|slot|workspace|101-mawjs|agent-window|Soul/workspace",
        "--oracle",
        "workspace|-|-|-|agent-window|Soul/workspace|-|false|true",
        "--tree",
        "--json",
        "--plan-json",
    ]);
    assert_eq!(
        discover["fleet"]["records"][0]["endpoint"],
        "http://window-node:3456"
    );
    assert_eq!(discover["oracles"]["records"][0]["fleetMatched"], true);

    let active_minutes = assert_json(&[
        "ls",
        "--active",
        "2m",
        "--json",
        "--now",
        "200",
        "--pane",
        "%1|bash|50-mawjs:1.0|minute-old|100|/repo|100",
    ]);
    assert_eq!(active_minutes["activeThresholdSec"], 120);
    assert_eq!(active_minutes["sessions"][0]["status"], "idle");

    let empty_days = assert_ok(&[
        "ls",
        "--active=1d",
        "--now",
        "200000",
        "--pane",
        "%9|bash|50-mawjs:1.0|too-old|100|/repo|1",
    ]);
    assert_eq!(empty_days.stdout, "No sessions active in the last 1d.\n");
    assert_usage(
        &["bring", "--engine", "codex"],
        "bring: missing oracle name",
    );
}
