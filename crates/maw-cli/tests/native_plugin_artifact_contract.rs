#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, run_cli, DispatchKind};
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
    maw_js_ref_dir: Option<OsString>,
    path: Option<OsString>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            maw_js_ref_dir: std::env::var_os("MAW_JS_REF_DIR"),
            path: std::env::var_os("PATH"),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env("MAW_JS_REF_DIR", self.maw_js_ref_dir.take());
        restore_env("PATH", self.path.take());
    }
}

fn restore_env(key: &str, value: Option<OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "maw-plugin-artifact-contract-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&root).expect("temp");
    root
}

fn write_tool_shim(dir: &Path, name: &str) {
    let shim = dir.join(name);
    write(
        &shim,
        format!(
            "#!/bin/sh\nprintf 'DELEGATED-{}\\n'\nexit 55\n",
            name.to_uppercase()
        ),
    )
    .expect("write shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).expect("chmod");
    }
}

fn write_js_plugin(root: &Path) -> PathBuf {
    let dir = root.join("legacy-js");
    create_dir_all(&dir).expect("plugin");
    write(
        dir.join("index.ts"),
        "export default function handle() {}\n",
    )
    .expect("entry");
    write(
        dir.join("plugin.json"),
        r#"{"name":"legacy-js","version":"1.0.0","sdk":"*","target":"js","entry":"index.ts","capabilities":["sdk:identity"]}"#,
    )
    .expect("manifest");
    dir
}

#[test]
fn plugin_artifact_contract_is_native_and_never_delegates_to_maw_or_bun() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("no-delegate");
    let bin_dir = root.join("bin");
    create_dir_all(&bin_dir).expect("bin");
    write_tool_shim(&bin_dir, "maw");
    write_tool_shim(&bin_dir, "bun");
    let plugin_dir = write_js_plugin(&root);
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");

    let output = run_cli(&args(&[
        "plugin-artifact",
        "plan",
        plugin_dir.to_str().expect("dir"),
    ]));

    assert_eq!(dispatcher_status("plugin-artifact"), DispatchKind::Native);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        include_str!("fixtures/native-plugin-artifact/plan-js-refused.stdout")
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
        !output.stdout.contains("DELEGATED-BUN"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stderr.contains("DELEGATED-BUN"),
        "{}",
        output.stderr
    );

    remove_dir_all(root).expect("cleanup");
}
