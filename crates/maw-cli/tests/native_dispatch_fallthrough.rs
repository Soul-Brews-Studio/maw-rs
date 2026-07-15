use std::{
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use maw_cli::run_cli;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-dispatch-fallthrough-{label}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

struct EnvRestore {
    key: &'static str,
    value: Option<std::ffi::OsString>,
}

impl EnvRestore {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let restore = Self {
            key,
            value: std::env::var_os(key),
        };
        std::env::set_var(key, value);
        restore
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn write_plugin(plugins_dir: &Path) {
    let package_dir = plugins_dir.join("ctq-legacy");
    std::fs::create_dir_all(&package_dir).expect("plugin dir");
    std::fs::write(
        package_dir.join("index.ts"),
        "export default async function plugin() {}\n",
    )
    .expect("entry");
    std::fs::write(
        package_dir.join("plugin.json"),
        r#"{"name":"ctq-legacy","version":"1.0.0","sdk":"*","runtime":"bun-dev","target":"js","entry":"index.ts","cli":{"command":"cross-team-queue","help":"maw cross-team-queue"}}"#,
    )
    .expect("manifest");
}

#[test]
fn native_unknown_args_fall_through_to_same_name_plugin_fast() {
    let _guard = env_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = temp_dir("ctq");
    let empty_path = root.join("empty-path");
    let plugins_dir = root.join("plugins");
    std::fs::create_dir_all(&empty_path).expect("empty path dir");
    std::fs::create_dir_all(&plugins_dir).expect("plugins dir");
    write_plugin(&plugins_dir);

    let _path = EnvRestore::set("PATH", &empty_path);
    let _home = EnvRestore::set("HOME", &root);
    let _xdg_config = EnvRestore::set("XDG_CONFIG_HOME", root.join("config"));
    let _xdg_state = EnvRestore::set("XDG_STATE_HOME", root.join("state"));
    let _maw_home = EnvRestore::set("MAW_HOME", root.join("maw-home"));
    let _plugins = EnvRestore::set("MAW_PLUGINS_DIR", &plugins_dir);

    let started = Instant::now();
    let output = run_cli(&[
        "cross-team-queue".to_owned(),
        "--recipient".to_owned(),
        "athena".to_owned(),
    ]);
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(1),
        "fall-through dispatch took {elapsed:?}"
    );
    assert_eq!(output.code, 2, "stderr={}", output.stderr);
    assert!(output.stdout.is_empty(), "stdout={}", output.stdout);
    assert!(
        output.stderr.contains("ctq-legacy needs bun"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("unknown flag --recipient"),
        "{}",
        output.stderr
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
