#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn zenoh_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn zenoh_temp(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-native-zenoh-scout-{name}-{nonce}"));
    std::fs::create_dir_all(root.join("home/.maw")).expect("home");
    root
}

fn zenoh_command(root: &std::path::Path) -> Command {
    let mut command = Command::new(zenoh_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn zenoh_scout_native_status_is_opt_in_and_nodep() {
    let root = zenoh_temp("status");
    let output = zenoh_command(&root)
        .args(["scout", "--status"])
        .output()
        .expect("run scout status");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("zenoh-scout disabled"), "{stdout}");
    assert!(
        stdout.contains("opt-in") || stdout.contains("set zenoh.scout.enabled=true"),
        "{stdout}"
    );
    assert_eq!(dispatcher_status("scout"), DispatchKind::Native);
    assert_eq!(dispatcher_status("discover"), DispatchKind::Native);
    assert_eq!(dispatcher_status("zenoh-scout"), DispatchKind::Native);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn zenoh_scout_native_force_fails_closed_without_zenoh_crate_or_bun() {
    let root = zenoh_temp("force");
    let output = zenoh_command(&root)
        .args([
            "zenoh-scout",
            "--force",
            "--locator",
            "ws://127.0.0.1:10000",
        ])
        .output()
        .expect("run zenoh force");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("zenoh-scout unavailable"), "{stdout}");
    assert!(
        stdout.contains("native zenoh backend is not linked"),
        "{stdout}"
    );
    assert!(!stdout.to_lowercase().contains("bun"), "{stdout}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn zenoh_scout_native_guards_flags_before_io() {
    let root = zenoh_temp("guard");
    let bad_locator = zenoh_command(&root)
        .args(["scout", "--locator", "-bad"])
        .output()
        .expect("run bad locator");
    assert!(!bad_locator.status.success());
    assert!(String::from_utf8(bad_locator.stderr)
        .expect("stderr")
        .contains("locator"));

    let bad_transport = zenoh_command(&root)
        .args(["scout", "--transport", "bad"])
        .output()
        .expect("run bad transport");
    assert!(!bad_transport.status.success());
    assert!(String::from_utf8(bad_transport.stderr)
        .expect("stderr")
        .contains("zenoh|scout|both"));
    let _ = std::fs::remove_dir_all(root);
}
