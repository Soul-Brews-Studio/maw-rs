#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, run_cli, DispatchKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-native-ls-flags-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn chmod_exec(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn write_fake_maw(bin_dir: &Path) {
    let maw = bin_dir.join("maw");
    fs::write(
        &maw,
        r#"#!/bin/sh
printf 'DELEGATED-MAW %s\n' "$*"
exit 42
"#,
    )
    .expect("write fake maw");
    chmod_exec(&maw);
}

fn write_fake_curl(bin_dir: &Path) {
    let curl = bin_dir.join("curl");
    // GET /api/sessions returns a TOP-LEVEL array (measured against a real serve), not a
    // {"sessions":[…]} envelope. The stub mirrors that measured shape so both federation
    // tests exercise the `as_array` branch of ls_sessions_from_payload — deleting that
    // branch then turns these RED instead of silently returning empty for every real peer
    // (#676; the old stub encoded the imagined shape of the never-existent /api/ls).
    fs::write(
        &curl,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_LS_CURL_LOG"
printf '[{"name":"blue-oracle","windows":[{"name":"main","index":0,"active":true}]}]'
"#,
    )
    .expect("write fake curl");
    chmod_exec(&curl);
}

fn write_fake_git(bin_dir: &Path, log: &Path) {
    let git = bin_dir.join("git");
    fs::write(
        &git,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
exit 0
"#,
            log.display()
        ),
    )
    .expect("write fake git");
    chmod_exec(&git);
}

fn run_binary(root: &Path, args: &[&str]) -> std::process::Output {
    let bin_dir = root.join("bin");
    let home = root.join("home");
    let xdg_state = root.join("xdg-state");
    let xdg_config = root.join("xdg-config");
    fs::create_dir_all(&bin_dir).expect("bin");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(xdg_state.join("maw")).expect("state");
    fs::create_dir_all(xdg_config.join("maw")).expect("config");
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("HOME", &home)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("PEERS_FILE", xdg_state.join("maw/peers.json"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_LS_CURL_LOG", root.join("curl.log"))
        .output()
        .expect("run maw-rs")
}

#[test]
fn ls_flags_parse_and_render_federation_golden() {
    assert_eq!(dispatcher_status("ls"), DispatchKind::Native);
    let root = temp_dir("golden");
    let state = root.join("xdg-state/maw");
    fs::create_dir_all(&state).expect("state");
    fs::write(
        state.join("peers.json"),
        r#"{"version":1,"peers":{"blue":{"url":"http://127.0.0.1:9999","node":"blue-node","addedAt":"2026-06-27T00:00:00Z"}}}"#,
    )
    .expect("peers");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin");
    write_fake_curl(&bin_dir);

    let output = run_binary(
        &root,
        &[
            "ls",
            "--federation",
            "--json",
            "--pane",
            "%1|claude|50-mawjs:1.0|mawjs|100|/repo|1700000000",
            "--now",
            "1700000600",
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-ls-flags/federation-json.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(root.join("curl.log")).expect("curl log"),
        "-fsS --max-time 2 -- http://127.0.0.1:9999/api/sessions\n"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn ls_federation_peer_drilldown_fetches_peer_sessions() {
    assert_eq!(dispatcher_status("ls"), DispatchKind::Native);
    let root = temp_dir("peer");
    let state = root.join("xdg-state/maw");
    fs::create_dir_all(&state).expect("state");
    fs::write(
        state.join("peers.json"),
        r#"{"version":1,"peers":{"blue":{"url":"http://127.0.0.1:9999","node":"blue-node","addedAt":"2026-06-27T00:00:00Z"}}}"#,
    )
    .expect("peers");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin");
    write_fake_curl(&bin_dir);

    let output = run_binary(&root, &["ls", "--federation", "blue", "--json"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "{\"peer\":\"blue\",\"url\":\"http://127.0.0.1:9999\",\"sessions\":[{\"name\":\"blue-oracle\",\"windows\":[{\"active\":true,\"index\":0,\"name\":\"main\"}]}]}\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(root.join("curl.log")).expect("curl log"),
        "-fsS --max-time 2 -- http://127.0.0.1:9999/api/sessions\n"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn ls_node_requires_safe_value_and_fleet_only_filters_orphans() {
    let missing = run_cli(&args(&["ls", "--node"]));
    assert_eq!(missing.code, 2);
    assert!(missing.stderr.contains("--node requires a value"));

    let dash = run_cli(&args(&["ls", "--node", "-bad"]));
    assert_eq!(dash.code, 2);
    assert!(dash.stderr.contains("must not start"));

    let output = run_cli(&args(&[
        "ls",
        "--plan-json",
        "--fleet-only",
        "--pane",
        "%1|claude|50-mawjs:1.0|mawjs|100|/repo|1700000000",
        "--pane",
        "%2|claude|scratch:1.0|scratch|100|/repo|1700000000",
    ]));
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stdout.contains("50-mawjs"));
    assert!(!output.stdout.contains("scratch"));
}

#[test]
fn ls_verify_and_fix_validate_before_git_prune_and_use_argv_only() {
    let root = temp_dir("fix");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin");
    let git_log = root.join("git.log");
    write_fake_git(&bin_dir, &git_log);
    write_fake_maw(&bin_dir);

    let output = run_binary(&root, &["ls", "--fix"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("worktree root validated"), "{stdout}");
    assert!(stdout.contains("pruned via git worktree prune"), "{stdout}");
    assert!(!stdout.contains("DELEGATED-MAW"), "{stdout}");
    assert_eq!(
        fs::read_to_string(git_log).expect("git log"),
        format!(
            "-C {} worktree prune\n",
            root.canonicalize().expect("canonical").display()
        )
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn ls_native_binary_no_delegation_with_fake_maw_and_missing_js_ref() {
    let root = temp_dir("no-delegate");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin");
    write_fake_maw(&bin_dir);

    let output = run_binary(&root, &["ls", "--help"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("maw ls --federation"), "{stdout}");
    assert!(!stdout.contains("DELEGATED-MAW"), "{stdout}");
    fs::remove_dir_all(root).expect("cleanup");
}
