#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use maw_cli::{dispatcher_status, DispatchKind};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-profile-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed_config(root: &Path, active: &str) -> PathBuf {
    let config = root.join("config");
    let profiles = config.join("profiles");
    fs::create_dir_all(&profiles).expect("profiles dir");
    fs::write(config.join("profile-active"), format!("{active}\n")).expect("active profile");
    fs::write(
        profiles.join("all.json"),
        "{\n  \"name\": \"all\",\n  \"description\": \"All plugins (Phase 1 default — equivalent to no profile filter).\"\n}\n",
    )
    .expect("all profile");
    fs::write(
        profiles.join("lean.json"),
        "{\n  \"description\": \"Lean profile\",\n  \"name\": \"lean\",\n  \"plugins\": [\n    \"peek\",\n    \"profile\"\n  ],\n  \"tiers\": [\n    \"core\",\n    \"standard\"\n  ]\n}\n",
    )
    .expect("lean profile");
    config
}

fn fake_maw_path(root: &Path) -> PathBuf {
    let bin_dir = root.join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("fake bin");
    let maw = bin_dir.join("maw");
    fs::write(&maw, "#!/bin/sh\necho DELEGATED-MAW \"$@\"\nexit 99\n").expect("fake maw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&maw, fs::Permissions::from_mode(0o755)).expect("chmod fake maw");
    }
    bin_dir
}

fn run_profile(root: &Path, args: &[&str]) -> Output {
    let config = root.join("config");
    let fake_bin = fake_maw_path(root);
    Command::new(bin())
        .args(args)
        .env_clear()
        .env("PATH", fake_bin)
        .env("HOME", root.join("home"))
        .env("MAW_CONFIG_DIR", config)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn assert_no_delegation(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("DELEGATED-MAW"),
        "stdout delegated: {stdout}"
    );
    assert!(
        !stderr.contains("DELEGATED-MAW"),
        "stderr delegated: {stderr}"
    );
}

fn assert_success_golden(root: &Path, args: &[&str], expected: &str) {
    let output = run_profile(root, args);
    assert_no_delegation(&output);
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn profile_dispatch_is_native() {
    assert_eq!(dispatcher_status("profile"), DispatchKind::Native);
}

#[test]
fn profile_committed_goldens_use_native_config_without_maw_delegation() {
    let root = temp_dir("golden");
    seed_config(&root, "lean");

    assert_success_golden(
        &root,
        &["profile", "current"],
        include_str!("fixtures/native-profile/current.stdout"),
    );
    assert_success_golden(
        &root,
        &["profile", "list"],
        include_str!("fixtures/native-profile/list.stdout"),
    );
    assert_success_golden(
        &root,
        &["profile", "show", "lean"],
        include_str!("fixtures/native-profile/show-lean.stdout"),
    );
}

#[test]
fn profile_use_updates_pointer_atomically_without_maw_delegation() {
    let root = temp_dir("use");
    let config = seed_config(&root, "all");
    assert_success_golden(
        &root,
        &["profile", "use", "lean"],
        include_str!("fixtures/native-profile/use-lean.stdout"),
    );
    assert_eq!(
        fs::read_to_string(config.join("profile-active")).expect("active pointer"),
        "lean\n"
    );
    assert!(!config.join("profile-active.tmp").exists());
}

#[test]
fn profile_rejects_injected_profile_names_before_any_delegation() {
    let root = temp_dir("reject");
    seed_config(&root, "all");
    let output = run_profile(&root, &["profile", "use", "--bad"]);
    assert_no_delegation(&output);
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("invalid profile name"));
}
