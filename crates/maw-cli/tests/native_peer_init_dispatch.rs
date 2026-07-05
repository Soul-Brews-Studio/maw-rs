use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-epic55-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(args: &[&str], cwd: &Path, maw_home: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env("MAW_HOME", maw_home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_PEERS_FAKE_NOW", "1000")
        .output()
        .expect("run maw-rs")
}

#[test]
fn epic55_peers_list_empty_matches_committed_golden_without_ref_checkout() {
    let root = temp_dir("peers-empty");
    let output = run(&["peers", "list"], &root, &root.join("home"));
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/epic55/peers-list-empty.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn epic55_peers_add_and_list_match_committed_golden_without_ref_checkout() {
    let root = temp_dir("peers-add");
    let home = root.join("home");
    let add = run(
        &[
            "peers",
            "add",
            "alpha",
            "http://127.0.0.1:3456",
            "--node",
            "node-a",
            "--allow-unreachable",
        ],
        &root,
        &home,
    );
    assert!(
        add.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&add.stderr)
    );
    let list = run(&["peers", "list"], &root, &home);
    assert!(
        list.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&list.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8(add.stdout).expect("add stdout"),
        String::from_utf8(list.stdout).expect("list stdout")
    );
    assert_eq!(
        combined,
        include_str!("fixtures/epic55/peers-add-list.stdout")
    );
    assert_eq!(String::from_utf8(add.stderr).expect("add stderr"), "");
    assert_eq!(String::from_utf8(list.stderr).expect("list stderr"), "");
}

#[test]
fn epic55_init_help_matches_committed_golden_without_ref_checkout() {
    let root = temp_dir("init-help");
    let output = run(&["init", "--help"], &root, &root.join("home"));
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/epic55/init-help.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn epic55_dispatch_registers_assigned_parts_without_token_secret_path() {
    assert_eq!(
        maw_cli::dispatcher_status("peers"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("peer"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("init"),
        maw_cli::DispatchKind::Native
    );
}
