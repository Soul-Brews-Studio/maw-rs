// Offline integration surface for `maw update` / `maw upgrade`.
//
// Every test here must stay network-free: only argv parsing, --help, and
// error surfaces are exercised (never a code path that reaches the GitHub
// releases API). The single allowed live-network test is the #[ignore]d
// `update_live_releases_api_lists_channel_tags` unit test in update.rs,
// run manually. The fake `maw`/`bun` markers on PATH prove the handler is
// native — it must never delegate to an external maw or bun.

use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn update_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-update-{label}-{}-{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir_all(root.join("config/maw")).expect("config");
    fs::create_dir_all(root.join("state")).expect("state");
    root
}

fn update_chmod_exec(path: &Path) {
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms).expect("chmod");
}

fn update_write_fake_marker(bin_dir: &Path, name: &str, marker: &str) {
    let path = bin_dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho '{marker} $*'\nexit 0\n")).expect("marker");
    update_chmod_exec(&path);
}

fn update_run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(args)
        .env_clear()
        .env("PATH", root.join("bin"))
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("MAW_CONFIG_DIR", root.join("config/maw"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run update")
}

fn update_assert_no_delegation(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("DELEGATED-MAW"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-MAW"), "stderr={stderr}");
    assert!(!stdout.contains("DELEGATED-BUN"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-BUN"), "stderr={stderr}");
}

fn update_fake_maw_bun_root(label: &str) -> PathBuf {
    let root = update_temp_dir(label);
    let bin_dir = root.join("bin");
    update_write_fake_marker(&bin_dir, "maw", "DELEGATED-MAW");
    update_write_fake_marker(&bin_dir, "bun", "DELEGATED-BUN");
    root
}

#[test]
fn update_upgrade_help_is_native_offline_and_never_delegates() {
    assert_eq!(dispatcher_status("update"), DispatchKind::Native);
    assert_eq!(dispatcher_status("upgrade"), DispatchKind::Native);
    let root = update_fake_maw_bun_root("help");

    for command in ["update", "upgrade"] {
        let output = update_run(&root, &[command, "--help"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        update_assert_no_delegation(&output);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("usage: maw update"), "stdout={stdout}");
        assert!(stdout.contains("sha256"), "stdout={stdout}");
        assert!(
            output.stderr.is_empty(),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn update_rejects_unknown_channel_with_parse_error_before_any_network() {
    let root = update_fake_maw_bun_root("bad-channel");

    let output = update_run(&root, &["update", "--channel", "beta"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    update_assert_no_delegation(&output);
    assert!(
        output.stdout.is_empty(),
        "stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown channel \"beta\""),
        "stderr={stderr}"
    );
    assert!(
        stderr.contains("expected stable or alpha"),
        "stderr={stderr}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn update_upgrade_reject_bogus_flags_with_command_specific_usage_hint() {
    let root = update_fake_maw_bun_root("bogus-flag");

    for command in ["update", "upgrade"] {
        let output = update_run(&root, &[command, "--bogus-flag"]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        update_assert_no_delegation(&output);
        assert!(
            output.stdout.is_empty(),
            "stdout={}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unknown flag \"--bogus-flag\""),
            "stderr={stderr}"
        );
        assert!(
            stderr.contains(&format!("maw {command} --help")),
            "stderr={stderr}"
        );
    }
    let _ = fs::remove_dir_all(root);
}
