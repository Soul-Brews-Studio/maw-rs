#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, run_cli, DispatchKind};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    values: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn capture(keys: &[&'static str]) -> Self {
        Self {
            values: keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.values.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-triggers-{label}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    root
}

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, body).expect("write");
}

fn write_fake_maw(bin: &Path) {
    write_file(
        &bin.join("maw"),
        "#!/bin/sh\nprintf 'DELEGATED-MAW\\n'\nprintf 'args=%s\\n' \"$*\"\nexit 37\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(bin.join("maw"))
            .expect("shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(bin.join("maw"), permissions).expect("chmod shim");
    }
}

fn setup_env(root: &Path) -> EnvRestore {
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
    let restore = EnvRestore::capture(&keys);
    write_fake_maw(&root.join("bin"));
    std::env::set_var("HOME", root.join("home"));
    std::env::remove_var("MAW_HOME");
    std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
    std::env::set_var("MAW_STATE_DIR", root.join("state"));
    std::env::set_var("MAW_PLUGINS_DIR", root.join("plugins"));
    std::env::set_var("PATH", root.join("bin"));
    std::env::remove_var("MAW_FROM_RS");
    std::fs::create_dir_all(root.join("plugins")).expect("plugins");
    restore
}

fn binary_command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_maw-rs"));
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_CONFIG_DIR", root.join("config"))
        .env("MAW_STATE_DIR", root.join("state"))
        .env("MAW_PLUGINS_DIR", root.join("plugins"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("PATH", root.join("bin"));
    command
}

fn seed_config(root: &Path) {
    write_file(
        &root.join("config/maw.config.json"),
        r#"{
  "node": "test-node",
  "triggers": [
    { "on": "issue-close", "repo": "Soul-Brews-Studio/maw-js", "action": "maw hey pulse-oracle issue closed", "once": true },
    { "on": "pr-merge", "repo": "Soul-Brews-Studio/maw-js", "action": "maw done neo-mawjs" },
    { "on": "agent-idle", "timeout": 30, "action": "maw sleep {agent}" },
    { "on": "agent-wake", "action": "maw hey awakened-oracle hi" },
    { "on": "agent-crash", "action": "maw hey ops-oracle crash happened" },
    { "on": "custom-event", "action": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx" },
    { "on": "-bad", "action": "should not render" },
    { "on": "bad", "action": "SECRET_TOKEN_SHOULD_NOT_RENDER\nline" }
  ]
}"#,
    );
}

#[test]
fn triggers_empty_is_native_and_never_invokes_path_maw() {
    let _guard = env_lock().lock().expect("env lock");
    let root = temp_dir("empty");
    let _restore = setup_env(&root);

    let output = run_cli(&args(&["triggers"]));

    assert_eq!(dispatcher_status("triggers"), DispatchKind::Native);
    assert_eq!(dispatcher_status("trigger"), DispatchKind::Native);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert_eq!(
        output.stdout,
        include_str!("fixtures/zero-bun/triggers-empty.stdout")
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn triggers_reads_configured_triggers_hermetically_without_secret_leak() {
    let _guard = env_lock().lock().expect("env lock");
    let root = temp_dir("configured");
    let _restore = setup_env(&root);
    seed_config(&root);

    let output = run_cli(&args(&["trigger"]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        include_str!("fixtures/zero-bun/triggers-configured.stdout")
    );
    assert!(
        !output.stdout.contains("SECRET_TOKEN_SHOULD_NOT_RENDER"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stderr.contains("SECRET_TOKEN_SHOULD_NOT_RENDER"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn triggers_runtime_fake_maw_proof() {
    let root = temp_dir("runtime");
    write_fake_maw(&root.join("bin"));
    std::fs::create_dir_all(root.join("plugins")).expect("plugins");
    seed_config(&root);

    let output = binary_command(&root)
        .arg("triggers")
        .output()
        .expect("run maw-rs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stdout.contains("Workflow Triggers"), "{stdout}");
    assert!(!stdout.contains("DELEGATED-MAW"), "{stdout}");
    assert!(!stderr.contains("DELEGATED-MAW"), "{stderr}");
    assert!(!stderr.contains("failed to run maw fallback"), "{stderr}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn triggers_rejects_args_before_fallback() {
    let _guard = env_lock().lock().expect("env lock");
    let root = temp_dir("args");
    let _restore = setup_env(&root);

    let output = run_cli(&args(&["triggers", "--bad"]));

    assert_ne!(output.code, 0);
    assert!(output.stdout.is_empty());
    assert!(
        output.stderr.contains("triggers: unknown argument --bad"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("DELEGATED-MAW"),
        "{}",
        output.stderr
    );
    let _ = std::fs::remove_dir_all(root);
}
