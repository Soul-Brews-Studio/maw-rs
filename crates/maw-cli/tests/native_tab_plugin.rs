#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    path: Option<OsString>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            path: std::env::var_os("PATH"),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-native-tab-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_fake_tmux(bin_dir: &Path) {
    let tmux = bin_dir.join("tmux");
    fs::write(
        &tmux,
        r#"#!/bin/sh
case "$1" in
  display-message)
    printf '50-mawjs\n'
    ;;
  list-windows)
    printf '0:shell:0\n1:oracle:1\n'
    ;;
  capture-pane)
    printf 'captured oracle\n'
    ;;
  list-panes)
    printf 'claude\n'
    ;;
  send-keys)
    printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
    ;;
  *)
    printf 'unexpected %s\n' "$1" >&2
    exit 9
    ;;
esac
"#,
    )
    .expect("write fake tmux");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&tmux).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tmux, permissions).expect("chmod");
    }
}

fn run(args: &[&str], root: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("MAW_HOME", root.join("home"))
        .env("GHQ_ROOT", root.join("ghq"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .output()
        .expect("run maw-rs")
}

#[test]
fn native_tab_list_matches_committed_maw_js_golden_without_ref_checkout() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("list");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_tmux(&bin_dir);
    std::env::set_var("PATH", &bin_dir);

    let output = run(&["tab"], &root);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-tab/list.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_tab_is_registered_and_peek_is_offline_temp_home() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("peek");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_tmux(&bin_dir);
    std::env::set_var("PATH", &bin_dir);

    assert_eq!(dispatcher_status("tab"), DispatchKind::Native);
    let output = run(&["tab", "1"], &root);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "\u{1b}[36m--- oracle ---\u{1b}[0m\ncaptured oracle\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    fs::remove_dir_all(root).expect("cleanup");
}
