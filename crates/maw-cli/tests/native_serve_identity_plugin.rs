#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn serveidentity_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn serveidentity_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn serveidentity_temp(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-native-serveidentity-{name}-{nonce}"));
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    root
}

fn serveidentity_command(root: &Path) -> Command {
    let mut command = Command::new(serveidentity_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TMUX", "/tmp/tmux-89,1,0")
        .env("TMUX_PANE", "%1")
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn serveidentity_native_reports_mounted_route_without_touching_identity_material() {
    let root = serveidentity_temp("mounted");
    serveidentity_write(
        &root.join("xdg-config/maw/maw.config.json"),
        r#"{"node":"white","oracle":"gm-bo","port":4567,"agents":{"nova":"local"}}"#,
    );
    let output = serveidentity_command(&root)
        .args(["serve-identity"])
        .output()
        .expect("run serve-identity");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stdout.contains("registers GET /api/identity"), "{stdout}");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(!stdout.contains("stub"), "{stdout}");
    assert!(!stdout.contains("TODO(#89)"), "{stdout}");
    assert_eq!(dispatcher_status("serve-identity"), DispatchKind::Native);
    assert!(!root.join("xdg-state/maw/peer-key").exists());
    assert!(!root.join("home/.maw/state/peer-key").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn serveidentity_native_guard_rejects_separator_before_io() {
    let root = serveidentity_temp("guard");
    let output = serveidentity_command(&root)
        .args(["serve-identity", "--"])
        .output()
        .expect("run serve-identity guard");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("-- separator is not supported"), "{stderr}");
    assert!(!root.join("home/.maw/state/peer-key").exists());
    let _ = std::fs::remove_dir_all(root);
}
