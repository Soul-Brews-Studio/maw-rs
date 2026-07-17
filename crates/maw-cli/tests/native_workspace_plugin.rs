#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, run_cli, DispatchKind};
use serde_json::json;
use std::ffi::OsString;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    vars: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn capture() -> Self {
        let keys = [
            "HOME",
            "MAW_HOME",
            "MAW_CONFIG_DIR",
            "MAW_DATA_DIR",
            "MAW_STATE_DIR",
            "MAW_CACHE_DIR",
            "MAW_XDG",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
            "MAW_FROM_RS",
            "MAW_PLUGINS_DIR",
            "PATH",
        ];
        Self {
            vars: keys
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.vars.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-workspace-native-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn write_executable(path: &Path, body: &str) {
    write(path, body).expect("write executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn setup_env(root: &Path) -> (PathBuf, PathBuf) {
    let maw_home = root.join("maw-home");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    create_dir_all(&maw_home).expect("maw home");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_executable(
        &bin_dir.join("maw"),
        "#!/bin/sh\nprintf 'DELEGATED-MAW\\n'\nprintf 'args=%s\\n' \"$*\"\nexit 37\n",
    );
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("MAW_HOME", &maw_home);
    std::env::remove_var("MAW_CONFIG_DIR");
    std::env::remove_var("MAW_DATA_DIR");
    std::env::remove_var("MAW_STATE_DIR");
    std::env::remove_var("MAW_CACHE_DIR");
    std::env::remove_var("MAW_XDG");
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("XDG_STATE_HOME");
    std::env::remove_var("XDG_CACHE_HOME");
    std::env::remove_var("MAW_FROM_RS");
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::set_var("PATH", &bin_dir);
    (
        maw_home.join("workspaces"),
        maw_home.join("config").join("workspaces"),
    )
}

fn write_ws(dir: &Path, filename: &str, value: &serde_json::Value) {
    create_dir_all(dir).expect("workspace dir");
    write(
        dir.join(filename),
        serde_json::to_string_pretty(&value).expect("json") + "\n",
    )
    .expect("write workspace");
}

#[test]
fn workspace_ls_is_native_and_never_invokes_path_maw() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("empty");
    setup_env(&root);

    let output = run_cli(&args(&["workspace", "ls"]));

    assert_eq!(dispatcher_status("workspace"), DispatchKind::Native);
    assert_eq!(dispatcher_status("ws"), DispatchKind::Native);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert_eq!(
        output.stdout,
        include_str!("fixtures/zero-bun/workspace-ls-empty.stdout")
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stderr.contains("DELEGATED-MAW"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("failed to run maw fallback"),
        "{}",
        output.stderr
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn workspace_list_reads_primary_and_legacy_workspace_files_hermetically() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("populated");
    let (primary, legacy) = setup_env(&root);
    create_dir_all(&legacy).expect("legacy dir");
    write(legacy.join("corrupt.json"), "{ nope").expect("corrupt");
    write_ws(
        &primary,
        "01-one.json",
        &json!({
            "id": "ws-l1",
            "name": "one",
            "hubUrl": "https://a.example",
            "sharedAgents": ["alice", 7, "bob"],
            "joinedAt": "2026-04-01",
            "lastStatus": "connected"
        }),
    );
    write_ws(
        &primary,
        "02-two.json",
        &json!({
            "id": "ws-l2",
            "name": "two",
            "hubUrl": "https://b.example",
            "sharedAgents": [],
            "joinedAt": "2026-04-02",
            "lastStatus": "disconnected"
        }),
    );
    write(primary.join("skip.txt"), "not json").expect("skip");

    let output = run_cli(&args(&["ws", "list"]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert_eq!(
        output.stdout,
        include_str!("fixtures/zero-bun/workspace-ls-populated.stdout")
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );

    remove_dir_all(root).expect("cleanup");
}
