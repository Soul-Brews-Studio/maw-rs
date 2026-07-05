use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn epic55_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn epic55_temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-epic55b-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn epic55_run(args: &[&str], cwd: &Path, maw_home: &Path, ghq_root: &Path) -> std::process::Output {
    Command::new(epic55_bin())
        .args(args)
        .current_dir(cwd)
        .env("MAW_HOME", maw_home)
        .env("GHQ_ROOT", ghq_root)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_SCOPE_FAKE_NOW", "1710000000")
        .output()
        .expect("run maw-rs")
}

#[test]
fn epic55_scope_list_empty_matches_committed_golden_without_ref_checkout() {
    let root = epic55_temp_dir("scope-list");
    let maw_home = root.join("home");
    let ghq = root.join("ghq");
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");

    let output = epic55_run(&["scope", "list"], &cwd, &maw_home, &ghq);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-orchestration/scope-list-empty.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn epic55_find_oracle_matches_committed_golden_without_ref_checkout() {
    let root = epic55_temp_dir("find-oracle");
    let maw_home = root.join("home");
    let ghq = root.join("ghq");
    let repo = ghq.join("github.com/acme/beta");
    fs::create_dir_all(&repo).expect("seed repo");

    let output = epic55_run(&["find", "beta"], &repo, &maw_home, &ghq);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-orchestration/find-oracle.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn epic55_scope_and_find_guard_leading_dash_values_before_io() {
    let root = epic55_temp_dir("guards");
    let maw_home = root.join("home");
    let ghq = root.join("ghq");
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");

    let scope = epic55_run(
        &["scope", "create", "alpha", "--members", "-bad"],
        &cwd,
        &maw_home,
        &ghq,
    );
    assert!(!scope.status.success());
    assert!(String::from_utf8(scope.stderr)
        .expect("stderr")
        .contains("invalid --members value"));

    let find = epic55_run(
        &["find", "--oracle", "-bad", "needle"],
        &cwd,
        &maw_home,
        &ghq,
    );
    assert!(!find.status.success());
    assert!(String::from_utf8(find.stderr)
        .expect("stderr")
        .contains("invalid --oracle value"));
}

#[test]
fn epic55_dispatch_registers_scope_find_without_token_slice() {
    assert_eq!(
        maw_cli::dispatcher_status("scope"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("find"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("token"),
        maw_cli::DispatchKind::Native
    );
}
