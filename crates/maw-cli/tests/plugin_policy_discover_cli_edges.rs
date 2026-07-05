#![allow(clippy::too_many_lines)]

use maw_cli::{run_cli, CliOutput};
use std::fs::{create_dir_all, write};
use std::time::{SystemTime, UNIX_EPOCH};

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
}

fn assert_error_code(args: &[&str], code: i32, expected: &str) {
    let output = run(args);
    assert_eq!(output.code, code, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
}

fn assert_ok(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stdout.contains(expected),
        "stdout for {args:?} did not contain {expected:?}: {}",
        output.stdout
    );
}

fn temp_path(label: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "maw-rs-cli-slice-c-{label}-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn early_command_error_and_text_edges_are_covered() {
    assert_ok(
        &[
            "auth",
            "verify-legacy-from",
            "--from",
            "mawjs:m5",
            "--signed-at",
            "2023-11-14T22:13:20.000Z",
            "--signature",
            "",
            "--now",
            "1700000000",
            "--cached-pubkey",
            "cached",
            "--plan-json",
        ],
        "refuse-unsigned",
    );
    assert_ok(
        &[
            "auth",
            "verify-v3-from",
            "--from",
            "mawjs:m5",
            "--timestamp",
            "1700000000",
            "--signature-v3",
            "",
            "--now",
            "1700000000",
            "--cached-pubkey",
            "cached",
            "--plan-json",
        ],
        "refuse-unsigned",
    );

    let blocking_file = temp_path("hub-file");
    write(&blocking_file, "not a dir").expect("write blocking file");
    assert_error_code(
        &[
            "hub",
            "load-workspaces",
            "--config-dir",
            blocking_file.to_str().unwrap(),
        ],
        1,
        "hub load-workspaces",
    );

    assert_ok(&["xdg", "core-paths"], "/");

    let blocking_home = temp_path("xdg-home");
    write(&blocking_home, "not a dir").expect("write blocking home");
    assert_error_code(
        &[
            "xdg",
            "core-paths",
            "--env",
            &format!("MAW_HOME={}", blocking_home.display()),
        ],
        1,
        "xdg core-paths",
    );
}

#[test]
fn plugin_feed_fuzzy_identity_policy_discover_error_edges_are_covered() {
    let bad_manifest_root = temp_path("bad-manifest-root");
    create_dir_all(&bad_manifest_root).expect("create bad manifest root");
    write(bad_manifest_root.join("plugin.json"), "{not json").expect("write bad manifest");
    assert_usage(
        &[
            "plugin-manifest",
            "load",
            "--dir",
            bad_manifest_root.to_str().unwrap(),
        ],
        "plugin-manifest",
    );

    assert_usage(
        &["feed", "active", "--window", "bad"],
        "--window must be an integer",
    );
    assert_usage(
        &["fuzzy", "--max-results", "1"],
        "fuzzy: expected distance or match",
    );
    assert_error_code(
        &["identity", "session-name", "neo", "--slot", "100"],
        1,
        "identity:",
    );
    assert_usage(
        &["policy", "--default-active", "nope", "--includes", "plugin"],
        "unknown --default-active key",
    );

    for (args, expected) in [
        (&["discover", "--peer"][..], "missing --peer value"),
        (
            &["discover", "--named-peer"][..],
            "missing --named-peer value",
        ),
        (
            &["discover", "--discovered"][..],
            "missing --discovered value",
        ),
        (&["discover", "--pane"][..], "missing --pane value"),
        (&["discover", "--plugin"][..], "missing --plugin value"),
        (&["discover", "--ghq"][..], "missing --ghq value"),
        (&["discover", "--agent"][..], "missing --agent value"),
        (&["discover", "--fleet"][..], "missing --fleet value"),
        (&["discover", "--oracle"][..], "missing --oracle value"),
        (&["discover", "--pane", "s|w|p|bad"][..], "--pane must use"),
        (
            &[
                "discover",
                "--oracle",
                "name|source|node|session|window|repo|path|true|bad",
            ][..],
            "has_fleet_config must be true or false",
        ),
    ] {
        assert_usage(args, expected);
    }
}
