use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_cli::{run_cli, CliOutput};

fn maw_rs_bin() -> PathBuf {
    let cargo_bin = PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"));
    if cargo_bin.exists() {
        return cargo_bin;
    }
    let mut current = std::env::current_exe().expect("current test exe");
    current.pop();
    if current.file_name().is_some_and(|name| name == "deps") {
        current.pop();
    }
    current.join("maw-rs")
}

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
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-cli-remaining-coverage-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn plugin_manifest_invoke_text_reports_wasm_read_errors() {
    let root = temp_dir("wasm-read-error");
    let plugin_dir = root.join("broken-wasm");
    create_dir_all(&plugin_dir).expect("plugin dir");
    create_dir_all(plugin_dir.join("missing.wasm")).expect("wasm dir");
    write(
        plugin_dir.join("plugin.json"),
        r#"{"name":"broken-wasm","version":"1.0.0","sdk":"*","wasm":"missing.wasm"}"#,
    )
    .expect("manifest");

    let output = run(&[
        "plugin-manifest",
        "invoke",
        "--scan-dir",
        root.to_string_lossy().as_ref(),
        "--plugin",
        "broken-wasm",
    ]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stdout.contains("failed to read wasm:"),
        "{}",
        output.stdout
    );
    assert!(output.stderr.is_empty());

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn route_text_error_without_hint_and_pair_api_shape_edges_are_stable() {
    assert_ok_contains(
        &[
            "route",
            "--query",
            "agent",
            "--agent",
            "agent=ghost-node",
        ],
        "route agent: error no_peer_url 'agent' mapped to node 'ghost-node' but no URL found hint=add ghost-node to maw.config.json namedPeers\n",
    );

    assert_ok_contains(
        &[
            "pair-api",
            "status",
            "--code",
            "ABC-DEF",
            "--now",
            "2000",
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
        ],
        "pair-api status status=404 ok=false\n",
    );
    assert_usage_contains(
        &["pair-api", "wat", "--code", "ABCDEF", "--now", "1"],
        "expected generate, probe, accept, or status",
    );
}

#[test]
fn ls_parser_optional_duration_and_recent_positionals_are_stable() {
    let active_minutes = assert_ok_contains(
        &[
            "ls",
            "--active",
            "2",
            "--json",
            "--now",
            "1000",
            "--pane",
            "%1|node|50-mawjs:1.0|agent|100|/repo|950",
        ],
        "\"activeThresholdSec\":120",
    );
    assert!(active_minutes.stdout.contains("\"status\":\"idle\""));

    let active_with_filter = assert_ok_contains(
        &[
            "ls",
            "--active",
            "not-a-duration",
            "--json",
            "--now",
            "1000",
            "--pane",
            "%1|node|50-mawjs:1.0|agent|100|/repo|999",
        ],
        "\"sessions\":[]",
    );
    assert!(active_with_filter
        .stdout
        .contains("\"activeThresholdSec\":1800"));

    let recent_with_filter = assert_ok_contains(
        &[
            "ls",
            "--recent",
            "not-a-limit",
            "--json",
            "--now",
            "1000",
            "--session-created",
            "50-mawjs=1",
            "--pane",
            "%1|zsh|50-mawjs:1.0|shell|100|/repo|995",
        ],
        "\"sessions\":[]",
    );
    assert!(!recent_with_filter.stdout.contains("recentLimit"));

    assert_ok_contains(
        &[
            "ls",
            "--active",
            "--json",
            "--now",
            "1000",
            "--pane",
            "%1|node|50-mawjs:1.0|agent|100|/repo|999",
        ],
        "\"activeThresholdSec\":1800",
    );

    let active_without_following_value = run(&["ls", "--active"]);
    assert_eq!(active_without_following_value.code, 0);
    assert!(active_without_following_value.stderr.is_empty());

    assert_ok_contains(
        &[
            "ls",
            "--recent",
            "--json",
            "--now",
            "1000",
            "--pane",
            "%1|node|50-mawjs:1.0|agent|100|/repo|999",
        ],
        "\"sessions\":[",
    );

    let recent_without_following_value = run(&["ls", "--recent"]);
    assert_eq!(recent_without_following_value.code, 0);
    assert!(recent_without_following_value.stderr.is_empty());
}

#[test]
fn ls_empty_local_text_and_live_tmux_fallback_are_stable() {
    assert_ok_contains(
        &[
            "ls",
            "--pane",
            "%1|zsh|plain-discord:1.0|shell|100|/repo|900",
        ],
        "No active sessions.\n  → maw bud <name>     create new oracle\n  → maw wake <name>    attach existing\n",
    );

    let live = run(&["ls", "--json"]);
    assert_eq!(live.code, 0, "{}", live.stderr);
    assert!(
        live.stdout.starts_with("{\"command\":\"ls\""),
        "{}",
        live.stdout
    );
    assert!(live.stderr.is_empty());
}

#[test]
fn ls_child_process_emits_color_when_no_color_is_absent() {
    let bin = maw_rs_bin();
    let output = Command::new(bin)
        .env_remove("NO_COLOR")
        .args([
            "ls",
            "--all",
            "--now",
            "1000",
            "--pane",
            "%1|node|50-mawjs:1.0|agent|100|/repo|999",
        ])
        .output()
        .expect("run maw-rs");
    assert!(output.status.success(), "status: {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("\u{1b}["), "{stdout:?}");
    assert!(String::from_utf8(output.stderr)
        .expect("utf8 stderr")
        .is_empty());
}
