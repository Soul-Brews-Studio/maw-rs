use std::fs::{create_dir_all, read_to_string, write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    invoke_plugin, InvokeContext, InvokeResult, InvokeSource, LoadedPlugin, LoadedPluginKind,
    MvpWasmInvokeRuntime, PluginManifest, PluginTarget,
};
use serde_json::Value;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn ts_invoke_refuses_without_delegating_to_bun() {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = temp_dir("no-bun-runtime");
    let fake_bin = root.join("bin");
    create_dir_all(&fake_bin).expect("fake bin dir");
    let marker = root.join("delegated-bun.txt");
    write_fake_bun(&fake_bin.join("bun"), &marker);

    let old_path = std::env::var_os("PATH");
    let fake_path = prepend_path(&fake_bin, old_path.as_deref());
    std::env::set_var("PATH", &fake_path);

    let plugin_dir = root.join("plugin");
    create_dir_all(&plugin_dir).expect("plugin dir");
    let entry_path = plugin_dir.join("index.ts");
    write(
        &entry_path,
        "export default { async handle() { return { ok: true } } };\n",
    )
    .expect("entry");
    let plugin = ts_plugin(&plugin_dir, &entry_path);
    let result = invoke_plugin(
        &plugin,
        &InvokeContext {
            source: InvokeSource::Cli,
            args: vec!["--proof".to_owned()],
            cwd: None,
            home: None,
        },
        &mut MvpWasmInvokeRuntime,
    );

    restore_env("PATH", old_path);

    assert_eq!(capture(&result), read_golden("ts-refused.json"));
    assert!(
        !marker.exists(),
        "fake bun marker proves unexpected delegation"
    );
    assert_ne!(
        result.error.as_deref(),
        Some("DELEGATED-BUN"),
        "runtime error must not leak fake bun output"
    );
    let _ = std::fs::remove_dir_all(&root);
}

fn ts_plugin(dir: &Path, entry_path: &Path) -> LoadedPlugin {
    LoadedPlugin {
        manifest: PluginManifest {
            name: "no-bun-ts".to_owned(),
            version: "1.0.0".to_owned(),
            weight: None,
            tier: None,
            wasm: None,
            entry: Some("index.ts".to_owned()),
            entry_export: Some("handle".to_owned()),
            sdk: "*".to_owned(),
            cli: None,
            api: None,
            description: None,
            author: None,
            hooks: None,
            cron: None,
            module: None,
            transport: None,
            engine: None,
            target: Some(PluginTarget::Js),
            capability_namespaces: None,
            capabilities: Some(Vec::new()),
            capability_warnings: Vec::new(),
            dependencies: None,
            artifact: None,
        },
        dir: dir.to_path_buf(),
        wasm_path: dir.join("plugin.wasm"),
        entry_path: Some(entry_path.to_path_buf()),
        wasm_export: "handle".to_owned(),
        kind: LoadedPluginKind::Ts,
        disabled: false,
    }
}

fn capture(result: &InvokeResult) -> Value {
    serde_json::json!({
        "ok": result.ok,
        "output": result.output,
        "error": result.error,
    })
}

fn read_golden(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/no-bun-runtime")
        .join(name);
    serde_json::from_str(&read_to_string(&path).expect("golden file")).expect("golden json")
}

fn write_fake_bun(path: &Path, marker: &Path) {
    let script = format!(
        "#!/bin/sh\nprintf 'DELEGATED-BUN' > {}\necho DELEGATED-BUN >&2\nexit 42\n",
        shell_quote(marker)
    );
    write(path, script).expect("fake bun");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .expect("fake bun metadata")
            .permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(path, perms).expect("fake bun permissions");
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn prepend_path(fake_bin: &Path, old_path: Option<&std::ffi::OsStr>) -> std::ffi::OsString {
    let mut value = std::ffi::OsString::from(fake_bin.as_os_str());
    if let Some(old_path) = old_path {
        value.push(":");
        value.push(old_path);
    }
    value
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn temp_dir(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-{prefix}-{}-{stamp}", std::process::id()));
    create_dir_all(&path).expect("temp dir");
    path
}
