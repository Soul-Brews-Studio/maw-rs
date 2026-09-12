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
    let path = std::env::temp_dir().join(format!("maw-rs-shellenv-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_fake_maw(bin_dir: &Path) {
    fs::create_dir_all(bin_dir).expect("fake bin");
    let maw = bin_dir.join("maw");
    fs::write(
        &maw,
        "#!/bin/sh\necho FAKE-MAW-SHOULD-NOT-RUN \"$@\"\nexit 99\n",
    )
    .expect("fake maw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = fs::metadata(&maw).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&maw, perms).expect("chmod");
    }
}

fn run(args: &[&str], root: &Path) -> std::process::Output {
    let fake_bin = root.join("fake-bin");
    write_fake_maw(&fake_bin);
    Command::new(bin())
        .args(args)
        .env_clear()
        .env("PATH", &fake_bin)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

#[test]
fn shellenv_native_goldens_run_with_fake_maw_only() {
    let root = temp_dir("goldens");
    for (name, args, expected) in [
        (
            "bash",
            ["shellenv", "bash"].as_slice(),
            include_str!("fixtures/native-shellenv/bash.stdout"),
        ),
        (
            "zsh",
            ["shellenv", "zsh"].as_slice(),
            include_str!("fixtures/native-shellenv/zsh.stdout"),
        ),
        (
            "help",
            ["shellenv", "--help"].as_slice(),
            include_str!("fixtures/native-shellenv/help.stdout"),
        ),
    ] {
        let output = run(args, &root);
        assert!(
            output.status.success(),
            "{name}: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout");
        assert_eq!(stdout, expected, "{name}");
        assert!(
            !stdout.contains("FAKE-MAW-SHOULD-NOT-RUN"),
            "native shellenv must not call maw fallback"
        );
        assert_eq!(
            String::from_utf8(output.stderr).expect("stderr"),
            "",
            "{name}"
        );
    }
}

#[test]
fn shellenv_native_rejects_fish_without_fallback() {
    let root = temp_dir("fish");
    let output = run(&["shellenv", "fish"], &root);
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(stderr, include_str!("fixtures/native-shellenv/fish.stderr"));
    assert!(!stderr.contains("FAKE-MAW-SHOULD-NOT-RUN"));
    assert!(!stderr.contains("failed to run maw fallback"));
}
