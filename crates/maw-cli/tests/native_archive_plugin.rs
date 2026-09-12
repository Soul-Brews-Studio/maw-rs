#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
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
    let path = std::env::temp_dir().join(format!("maw-rs-archive-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(
    args: &[&str],
    cwd: &Path,
    maw_home: &Path,
    home: &Path,
    ghq_root: &Path,
) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env("MAW_HOME", maw_home)
        .env("HOME", home)
        .env("GHQ_ROOT", ghq_root)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn seed_fleet(maw_home: &Path) {
    fs::create_dir_all(maw_home.join("config/fleet")).expect("fleet dir");
    fs::write(
        maw_home.join("config/fleet/03-neo.json"),
        r#"{
  "name": "03-neo",
  "windows": [
    {
      "name": "neo-oracle",
      "repo": "Soul-Brews-Studio/neo-oracle"
    }
  ],
  "sync_peers": ["trinity", "morpheus"],
  "project_repos": []
}
"#,
    )
    .expect("fleet file");
}

#[test]
fn native_archive_dry_run_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("dry-run");
    let cwd = root.join("repo");
    let maw_home = root.join("maw-home");
    let home = root.join("home");
    let ghq = root.join("ghq");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(&home).expect("home");
    seed_fleet(&maw_home);

    let output = run(
        &["archive", "neo", "--dry-run"],
        &cwd,
        &maw_home,
        &home,
        &ghq,
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-archive/archive-dry-run.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert!(
        maw_home.join("config/fleet/03-neo.json").exists(),
        "dry-run must not disable fleet config"
    );
}

#[test]
fn native_archive_help_registered_and_temp_home_offline() {
    assert_eq!(
        maw_cli::dispatcher_status("archive"),
        maw_cli::DispatchKind::Native
    );
    assert!(maw_cli::native_dispatch_commands().contains(&"archive"));

    let root = temp_dir("help");
    let cwd = root.join("repo");
    let maw_home = root.join("maw-home");
    let home = root.join("home");
    let ghq = root.join("ghq");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(&home).expect("home");

    let output = run(&["archive", "--help"], &cwd, &maw_home, &home, &ghq);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-archive/archive-help.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert!(
        !maw_home.join("config/fleet").exists(),
        "help must not touch temp MAW_HOME"
    );
}

#[test]
fn native_archive_refuses_option_injection_target_before_side_effects() {
    let root = temp_dir("guard");
    let cwd = root.join("repo");
    let maw_home = root.join("maw-home");
    let home = root.join("home");
    let ghq = root.join("ghq");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(&home).expect("home");
    seed_fleet(&maw_home);

    let output = run(
        &["archive", "-oProxyCommand=touch-pwned", "--dry-run"],
        &cwd,
        &maw_home,
        &home,
        &ghq,
    );

    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("archive: unknown argument -oProxyCommand"));
    assert!(maw_home.join("config/fleet/03-neo.json").exists());
}
