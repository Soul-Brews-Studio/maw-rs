use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_cli::{run_cli, CliOutput};
use serde_json::json;

const PEER_KEY: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";
const FROM: &str = "mawjs:m5";
const SIGNED_AT: &str = "2023-11-14T22:13:20.000Z";
const NOW: &str = "1700000000";
const LEGACY_SIG: &str = "102cca45924d32428c10ff346d99cb13b5892b8c9b6a83da94607e379984ed5d";

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn assert_ok(args: &[&str], expected: &str) {
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

fn assert_usage(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
}

fn json_output(output: &CliOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
        panic!("invalid json: {error}\n{}", output.stdout);
    })
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-cli-coverage-slice-a-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn auto_wake_remaining_parser_and_site_rendering_edges_are_covered() {
    assert_usage(
        &["auto-wake", "neo", "--site=bogus"],
        "auto-wake: invalid --site value",
    );
    assert_usage(
        &["auto-wake", "neo", "--manifest-live"],
        "auto-wake: missing --manifest-live value",
    );
    assert_usage(
        &["auto-wake", "neo", "--manifest-live=maybe"],
        "auto-wake: --manifest-live must be true or false",
    );
    assert_usage(
        &["auto-wake", "neo", "--definitely-unknown"],
        "auto-wake: unknown argument --definitely-unknown",
    );

    for site in ["api-send", "api-wake", "peek", "bud", "wake-cmd"] {
        let output = run(&[
            "auto-wake",
            "neo",
            "--site",
            site,
            "--manifest-source",
            "fleet",
            "--manifest-live",
            "true",
            "--plan-json",
        ]);
        assert_eq!(output.code, 0, "{site}: {}", output.stderr);
        let value = json_output(&output);
        assert_eq!(value["site"], site);
        assert_eq!(value["manifest"]["isLive"], true);
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn auth_text_and_parser_edges_are_covered() {
    assert_ok(
        &[
            "auth",
            "verify-request",
            "--method",
            "POST",
            "--path",
            "/api/send",
            "--now",
            NOW,
        ],
        "accept-legacy",
    );
    assert_ok(
        &[
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
        ],
        "accept-tofu-record",
    );
    assert_ok(
        &[
            "auth",
            "verify-v3-from",
            "--from",
            FROM,
            "--timestamp",
            NOW,
            "--signature-v3",
            &"0".repeat(64),
            "--method",
            "POST",
            "--path",
            "/api/send",
            "--cached-pubkey",
            PEER_KEY,
            "--now",
            NOW,
        ],
        "refuse-mismatch",
    );
    assert_ok(
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
            "/api/send",
            "--now",
            NOW,
            "--body",
            "body",
        ],
        "",
    );

    assert_usage(
        &["auth", "sign-v1", "--weird"],
        "auth sign-v1: unknown argument --weird",
    );
    assert_usage(
        &["auth", "verify-v1", "--body-hash", "abc", "--odd"],
        "auth verify-v1: unknown argument --odd",
    );
    assert_usage(
        &["auth", "verify-legacy-from", "--odd"],
        "auth verify-legacy-from: unknown argument --odd",
    );
    assert_usage(
        &["auth", "verify-v3-from", "--odd"],
        "auth verify-v3-from: unknown argument --odd",
    );
    assert_usage(
        &["auth", "from-sign-payload", "--odd"],
        "auth from-sign-payload: unknown argument --odd",
    );
    assert_usage(
        &["auth", "hmac-verify", "--odd"],
        "auth hmac-verify: unknown argument --odd",
    );
    assert_usage(
        &["auth", "hmac-sign", "--odd"],
        "auth hmac-sign: unknown argument --odd",
    );
    assert_usage(
        &["auth", "sign-v3", "--odd"],
        "auth sign-v3: unknown argument --odd",
    );
    assert_usage(
        &["auth", "loopback", "--odd"],
        "auth loopback: unknown argument --odd",
    );
    assert_usage(
        &["auth", "from-address", "--odd"],
        "auth from-address: unknown argument --odd",
    );
    assert_usage(
        &["auth", "verify-request", "--odd"],
        "auth verify-request: unknown argument --odd",
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn hub_xdg_scaffold_bind_feed_fuzzy_identity_text_and_errors_are_covered() {
    assert_usage(
        &["hub"],
        "hub: expected validate-workspace or load-workspaces",
    );
    assert_usage(&["hub", "bogus"], "hub: unknown subcommand bogus");
    assert_usage(
        &["hub", "validate-workspace", "--bogus"],
        "hub validate-workspace: unknown argument --bogus",
    );
    assert_usage(
        &["hub", "load-workspaces", "--bogus"],
        "hub load-workspaces: unknown argument --bogus",
    );
    assert_ok(&["hub", "constants"], "hub constants heartbeat-ms=");

    assert_usage(
        &["xdg"],
        "xdg: expected paths, core-paths, or validate-instance",
    );
    assert_usage(&["xdg", "bogus"], "xdg: unknown subcommand bogus");
    assert_usage(
        &["xdg", "paths", "--env", "NO_EQUALS"],
        "xdg: --env must be KEY=VALUE",
    );
    assert_usage(
        &["xdg", "paths", "--bogus"],
        "xdg: unknown argument --bogus",
    );
    assert_usage(
        &["xdg", "validate-instance", "--bogus"],
        "xdg validate-instance: unknown argument --bogus",
    );
    assert_ok(
        &["xdg", "paths", "--home", "/tmp/maw-home"],
        "/tmp/maw-home/.maw",
    );
    assert_ok(&["xdg", "validate-instance", "--name", "maw_1"], "true");
    assert_ok(
        &["xdg", "constants"],
        "xdg constants modes=legacy,xdg,MAW_HOME",
    );

    assert_usage(
        &["plugin-scaffold"],
        "plugin-scaffold: expected validate-name or manifest",
    );
    assert_usage(
        &["plugin-scaffold", "bogus"],
        "plugin-scaffold: unknown subcommand bogus",
    );
    assert_usage(
        &["plugin-scaffold", "validate-name", "--bogus"],
        "plugin-scaffold validate-name: unknown argument --bogus",
    );
    assert_usage(
        &["plugin-scaffold", "manifest", "--bogus"],
        "plugin-scaffold manifest: unknown argument --bogus",
    );
    assert_ok(
        &["plugin-scaffold", "validate-name", "--name", "bad name"],
        "is invalid",
    );
    assert_ok(
        &["plugin-scaffold", "manifest", "--name", "slice-a", "--as"],
        "build/release.wasm",
    );
    assert_ok(
        &["plugin-scaffold", "constants"],
        "plugin-scaffold constants actions=validate-name,manifest",
    );

    assert_usage(
        &["bind-host", "--config-peers-len"],
        "bind-host: missing --config-peers-len value",
    );
    assert_usage(
        &["bind-host", "--config-named-peers-len"],
        "bind-host: missing --config-named-peers-len value",
    );
    assert_usage(
        &["bind-host", "--maw-host"],
        "bind-host: missing --maw-host value",
    );
    assert_usage(
        &["bind-host", "--peers-store-len"],
        "bind-host: missing --peers-store-len value",
    );
    assert_usage(
        &["bind-host", "--peers-store-error"],
        "bind-host: missing --peers-store-error value",
    );
    assert_usage(
        &["bind-host", "--bogus"],
        "bind-host: unknown argument --bogus",
    );
    assert_ok(&["bind-host", "--maw-host", "0.0.0.0"], "0.0.0.0");
    assert_ok(
        &["bind-host", "constants"],
        "bind-host constants hosts=127.0.0.1,0.0.0.0",
    );

    assert_usage(&["feed"], "feed: expected parse-line, describe, or active");
    assert_usage(&["feed", "parse-line"], "feed: missing parse-line value");
    assert_usage(&["feed", "describe"], "feed: missing describe event value");
    assert_usage(
        &["feed", "describe", "UserPromptSubmit", "--message"],
        "feed: missing --message value",
    );
    assert_usage(
        &["feed", "parse-line", "x", "--message", "oops"],
        "feed: --message requires describe",
    );
    assert_usage(
        &["feed", "parse-line", "x", "--now", "1"],
        "feed: --now requires active",
    );
    assert_usage(
        &["feed", "active", "--window"],
        "feed: missing --window value",
    );
    assert_usage(
        &["feed", "active", "--event"],
        "feed: missing --event value",
    );
    assert_usage(&["feed", "--bogus"], "feed: unknown argument --bogus");
    assert_ok(
        &[
            "feed",
            "active",
            "--now",
            "100",
            "--window",
            "50",
            "--event",
            "neo:90:hello",
        ],
        "neo",
    );
    assert_ok(
        &["feed", "constants"],
        "feed constants actions=parse-line,describe,active",
    );

    assert_usage(&["fuzzy"], "fuzzy: expected distance or match");
    assert_usage(&["fuzzy", "distance"], "fuzzy: missing distance left value");
    assert_usage(
        &["fuzzy", "distance", "left"],
        "fuzzy: missing distance right value",
    );
    assert_usage(&["fuzzy", "match"], "fuzzy: missing match input");
    assert_usage(
        &["fuzzy", "match", "neo", "--candidate"],
        "fuzzy: missing --candidate value",
    );
    assert_usage(
        &["fuzzy", "match", "neo", "--max-results"],
        "fuzzy: missing --max-results value",
    );
    assert_usage(
        &["fuzzy", "match", "neo", "--max-distance"],
        "fuzzy: missing --max-distance value",
    );
    assert_usage(&["fuzzy", "--bogus"], "fuzzy: unknown argument --bogus");
    assert_ok(
        &[
            "fuzzy",
            "match",
            "neo",
            "--candidate",
            "neo",
            "--max-results",
            "1",
        ],
        "neo",
    );
    assert_ok(
        &["fuzzy", "constants"],
        "fuzzy constants algorithm=levenshtein",
    );

    assert_usage(
        &["identity"],
        "identity: expected session-name or node-identity",
    );
    assert_usage(
        &["identity", "session-name"],
        "identity: missing session-name oracle",
    );
    assert_usage(&["identity", "node"], "identity: missing node host");
    assert_usage(
        &["identity", "session-name", "neo", "--slot"],
        "identity: missing --slot value",
    );
    assert_usage(
        &["identity", "session-name", "neo", "--slot", "bad"],
        "identity: --slot must be an integer",
    );
    assert_usage(
        &["identity", "node", "host", "--slot", "1"],
        "identity: --slot requires session-name",
    );
    assert_usage(
        &["identity", "node", "host", "--user"],
        "identity: missing --user value",
    );
    assert_usage(
        &["identity", "session-name", "neo", "--user", "nat"],
        "identity: --user requires node-identity",
    );
    assert_usage(
        &["identity", "--bogus"],
        "identity: unknown argument --bogus",
    );
    assert_ok(&["identity", "session-name", "neo", "--slot", "7"], "neo");
    assert_ok(&["identity", "node", "host", "--user", "nat"], "nat@host");
    assert_ok(
        &["identity", "constants"],
        "identity constants actions=session-name,node-identity",
    );
}

