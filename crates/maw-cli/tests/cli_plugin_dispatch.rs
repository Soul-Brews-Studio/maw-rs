use maw_cli::run_cli;
use maw_plugin_manifest::hash_file;
use serde_json::json;
use std::ffi::OsString;
use std::fs::{create_dir_all, read_to_string, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    home: Option<OsString>,
    maw_home: Option<OsString>,
    maw_plugins_dir: Option<OsString>,
    path: Option<OsString>,
    maw_shim_marker: Option<OsString>,
    bun_shim_args: Option<OsString>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            home: std::env::var_os("HOME"),
            maw_home: std::env::var_os("MAW_HOME"),
            maw_plugins_dir: std::env::var_os("MAW_PLUGINS_DIR"),
            path: std::env::var_os("PATH"),
            maw_shim_marker: std::env::var_os("MAW_SHIM_MARKER"),
            bun_shim_args: std::env::var_os("BUN_SHIM_ARGS"),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env("HOME", self.home.take());
        restore_env("MAW_HOME", self.maw_home.take());
        restore_env("MAW_PLUGINS_DIR", self.maw_plugins_dir.take());
        restore_env("PATH", self.path.take());
        restore_env("MAW_SHIM_MARKER", self.maw_shim_marker.take());
        restore_env("BUN_SHIM_ARGS", self.bun_shim_args.take());
    }
}

fn restore_env(key: &str, value: Option<OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-cli-plugin-dispatch-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn write_maw_shim(dir: &Path) {
    let shim = dir.join("maw");
    write(
        &shim,
        "#!/bin/sh\nif [ -n \"$MAW_SHIM_MARKER\" ]; then printf 'invoked\\n' > \"$MAW_SHIM_MARKER\"; fi\nprintf 'MAW_FROM_RS=%s\\n' \"$MAW_FROM_RS\"\nprintf 'args=%s\\n' \"$*\"\n",
    )
    .expect("write maw shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim)
            .expect("shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).expect("chmod maw shim");
    }
}

fn write_bun_shim(dir: &Path) {
    let shim = dir.join("bun");
    write(
        &shim,
        "#!/bin/sh\nentry=$1\nshift\nif [ -n \"$BUN_SHIM_ARGS\" ]; then\n  {\n    printf 'entry=%s\\n' \"$entry\"\n    i=0\n    for arg in \"$@\"; do\n      printf 'arg%s=%s\\n' \"$i\" \"$arg\"\n      i=$((i + 1))\n    done\n  } > \"$BUN_SHIM_ARGS\"\nfi\nprintf 'bun stdout\\n'\nprintf 'bun stderr\\n' >&2\n",
    )
    .expect("write bun shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim)
            .expect("shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).expect("chmod bun shim");
    }
}

fn write_ts_plugin(plugins_dir: &Path, dir_name: &str, command: &str) {
    write_ts_plugin_with_runtime(plugins_dir, dir_name, command, None);
}

fn write_bun_dev_ts_plugin(plugins_dir: &Path, dir_name: &str, command: &str) {
    write_ts_plugin_with_runtime(plugins_dir, dir_name, command, Some("bun-dev"));
}

fn write_ts_plugin_with_runtime(
    plugins_dir: &Path,
    dir_name: &str,
    command: &str,
    runtime: Option<&str>,
) {
    let package_dir = plugins_dir.join(dir_name);
    create_dir_all(&package_dir).expect("plugin dir");
    write(
        package_dir.join("index.ts"),
        b"export default async function plugin() {}\n",
    )
    .expect("entry");
    let mut manifest = json!({
        "name": dir_name,
        "version": "1.0.0",
        "sdk": "*",
        "target": "js",
        "entry": "index.ts",
        "cli": {
            "command": command,
            "help": format!("maw {command}")
        }
    });
    if let Some(runtime) = runtime {
        manifest["runtime"] = json!(runtime);
    }
    write(package_dir.join("plugin.json"), manifest.to_string()).expect("manifest");
}

fn write_bun_dev_wasm_plugin(plugins_dir: &Path, dir_name: &str, command: &str) {
    let package_dir = plugins_dir.join(dir_name);
    create_dir_all(&package_dir).expect("plugin dir");
    let wasm_path = package_dir.join("plugin.wasm");
    write(&wasm_path, WASM_HANDLE_ZERO).expect("wasm");
    let sha256 = hash_file(&wasm_path).expect("wasm hash");
    write(
        package_dir.join("plugin.json"),
        json!({
            "name": dir_name,
            "version": "1.0.0",
            "sdk": "*",
            "runtime": "bun-dev",
            "target": "wasm",
            "entry": {
                "kind": "wasm",
                "path": "plugin.wasm",
                "export": "handle"
            },
            "artifact": {
                "path": "plugin.wasm",
                "sha256": sha256
            },
            "cli": {
                "command": command,
                "help": format!("maw {command}")
            }
        })
        .to_string(),
    )
    .expect("manifest");
}

fn write_bun_dev_raw_wasm_with_ts_entry_plugin(plugins_dir: &Path, dir_name: &str, command: &str) {
    let package_dir = plugins_dir.join(dir_name);
    create_dir_all(&package_dir).expect("plugin dir");
    write(
        package_dir.join("index.ts"),
        b"export default async function plugin() {}\n",
    )
    .expect("entry");
    let wasm_path = package_dir.join("plugin.wasm");
    write(&wasm_path, WASM_HANDLE_ZERO).expect("wasm");
    let sha256 = hash_file(&wasm_path).expect("wasm hash");
    write(
        package_dir.join("plugin.json"),
        json!({
            "name": dir_name,
            "version": "1.0.0",
            "sdk": "*",
            "runtime": "bun-dev",
            "target": "js",
            "entry": "index.ts",
            "wasm": "plugin.wasm",
            "artifact": {
                "path": "plugin.wasm",
                "sha256": sha256
            },
            "cli": {
                "command": command,
                "help": format!("maw {command}")
            }
        })
        .to_string(),
    )
    .expect("manifest");
}

#[test]
fn dispatch_cli_plugin_finds_matching_ts_plugin_and_refuses_maw_bridge() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("prefix");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    let marker = root.join("maw-shim-invoked");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_maw_shim(&bin_dir);
    write_bun_shim(&bin_dir);
    write_ts_plugin(&plugins_dir, "weather-demo", "weather report");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::set_var("MAW_SHIM_MARKER", &marker);
    let bun_args = root.join("bun-args");
    std::env::set_var("BUN_SHIM_ARGS", &bun_args);

    let dispatched = run_cli(&args(&["weather", "report", "--city", "Bangkok"]));

    assert_eq!(dispatched.code, 2, "{}", dispatched.stdout);
    assert!(dispatched.stdout.is_empty(), "{}", dispatched.stdout);
    assert_eq!(
        dispatched.stderr,
        "TS/JS plugin requires prebuilt WASM artifact; no maw-js/Bun fallback\n"
    );
    assert!(
        !marker.exists(),
        "fake PATH maw was invoked, but TS/JS plugin dispatch must fail closed"
    );
    assert!(
        !bun_args.exists(),
        "fake PATH bun was invoked without runtime=bun-dev opt-in"
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn dispatch_cli_plugin_runs_explicit_bun_dev_runtime_with_argv() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("bun-dev");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    let bun_args = root.join("bun-args");
    let injected_marker = root.join("shell-injection");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_bun_shim(&bin_dir);
    write_bun_dev_ts_plugin(&plugins_dir, "weather-demo", "weather report");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::set_var("BUN_SHIM_ARGS", &bun_args);
    let injected_arg = format!("value;touch {}", injected_marker.display());

    let dispatched = run_cli(&[
        "weather".to_owned(),
        "report".to_owned(),
        "--city".to_owned(),
        "Bangkok".to_owned(),
        injected_arg.clone(),
    ]);

    assert_eq!(dispatched.code, 0, "{}", dispatched.stderr);
    assert_eq!(dispatched.stdout, "bun stdout\n");
    assert_eq!(
        dispatched.stderr,
        "⚠ [dev-tier: bun] weather-demo — TS runs unsandboxed; ship tier = WASM (maw plugin build)\nbun stderr\n"
    );
    let captured = read_to_string(&bun_args).expect("bun args");
    assert!(
        captured.contains(&format!(
            "entry={}\n",
            plugins_dir.join("weather-demo").join("index.ts").display()
        )),
        "{captured}"
    );
    assert!(captured.contains("arg0=--city\n"), "{captured}");
    assert!(captured.contains("arg1=Bangkok\n"), "{captured}");
    assert!(
        captured.contains(&format!("arg2={injected_arg}\n")),
        "{captured}"
    );
    assert!(
        !injected_marker.exists(),
        "plugin args were interpreted by a shell instead of passed as argv"
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn dispatch_cli_plugin_reports_missing_bun_for_bun_dev_runtime() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("bun-missing");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_bun_dev_ts_plugin(&plugins_dir, "weather-demo", "weather report");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);

    let dispatched = run_cli(&args(&["weather", "report"]));

    assert_eq!(dispatched.code, 2, "{}", dispatched.stdout);
    assert!(dispatched.stdout.is_empty(), "{}", dispatched.stdout);
    assert_eq!(
        dispatched.stderr,
        "⚠ [dev-tier: bun] weather-demo — TS runs unsandboxed; ship tier = WASM (maw plugin build)\ndev-tier plugin weather-demo needs bun; install bun or build wasm\n"
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn dispatch_cli_plugin_prefers_wasm_artifact_over_bun_dev_runtime() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("bun-dev-wasm");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    let bun_args = root.join("bun-args");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_bun_shim(&bin_dir);
    write_bun_dev_wasm_plugin(&plugins_dir, "weather-demo", "weather report");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::set_var("BUN_SHIM_ARGS", &bun_args);

    let dispatched = run_cli(&args(&["weather", "report"]));

    assert_eq!(dispatched.code, 0, "{}", dispatched.stderr);
    assert!(dispatched.stdout.is_empty(), "{}", dispatched.stdout);
    assert!(dispatched.stderr.is_empty(), "{}", dispatched.stderr);
    assert!(
        !bun_args.exists(),
        "fake PATH bun was invoked even though a WASM artifact was dispatchable"
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn dispatch_cli_plugin_raw_wasm_field_beats_bun_dev_string_ts_entry() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("bun-dev-raw-wasm");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    let bun_args = root.join("bun-args");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_bun_shim(&bin_dir);
    write_bun_dev_raw_wasm_with_ts_entry_plugin(&plugins_dir, "weather-demo", "weather report");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::set_var("BUN_SHIM_ARGS", &bun_args);

    let dispatched = run_cli(&args(&["weather", "report"]));

    assert_eq!(dispatched.code, 0, "{}", dispatched.stderr);
    assert!(dispatched.stdout.is_empty(), "{}", dispatched.stdout);
    assert!(dispatched.stderr.is_empty(), "{}", dispatched.stderr);
    assert!(
        !bun_args.exists(),
        "fake PATH bun was invoked even though raw wasm field must win"
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn unknown_plugin_command_falls_through_to_cli_error() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("unknown");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_maw_shim(&bin_dir);
    write_ts_plugin(&plugins_dir, "weather-demo", "weather report");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);

    let partial = run_cli(&args(&["weather", "--help"]));

    assert_eq!(partial.code, 2, "{}", partial.stdout);
    assert!(partial.stdout.is_empty(), "{}", partial.stdout);
    assert!(
        partial.stderr.contains("maw-rs: unknown command 'weather'"),
        "{}",
        partial.stderr
    );

    remove_dir_all(root).expect("cleanup");
}

const WASM_HANDLE_ZERO: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65,
    0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x68, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x00, 0x00, 0x0a,
    0x06, 0x01, 0x04, 0x00, 0x41, 0x00, 0x0b,
];

#[test]
fn plugin_ls_scans_home_maw_plugins_by_default() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("home-scan");
    let home = root.join("home");
    let plugins_dir = home.join(".maw").join("plugins");
    create_dir_all(&plugins_dir).expect("home plugins dir");
    write_ts_plugin(&plugins_dir, "home-weather", "home weather");
    std::env::set_var("HOME", &home);
    std::env::remove_var("MAW_HOME");
    std::env::remove_var("MAW_PLUGINS_DIR");

    let output = run_cli(&args(&["plugin", "ls"]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert_eq!(
        output.stdout,
        "1 plugin (1 active, 0 disabled)\n  core: 0 · standard: 0 · extra: 1\n  cli: 1 · api: 0 · health: ok\n"
    );

    remove_dir_all(root).expect("cleanup");
}
