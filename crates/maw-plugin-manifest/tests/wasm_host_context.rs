use std::fs::{create_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{parse_manifest, LoadedPlugin, LoadedPluginKind, MawWasmHost};
use serde_json::{json, Value};

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-wasm-ctx-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp dir");
    dir
}

fn host(dir: &Path, caps: &[&str]) -> MawWasmHost {
    write(dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("wasm");
    let manifest = parse_manifest(
        &json!({
            "name": "ctx-plugin",
            "version": "1.0.0",
            "sdk": "*",
            "entry": { "kind": "wasm", "path": "plugin.wasm", "export": "handle" },
            "capabilities": caps,
        })
        .to_string(),
        dir,
    )
    .expect("manifest");
    let loaded = LoadedPlugin {
        manifest,
        dir: dir.to_path_buf(),
        wasm_path: dir.join("plugin.wasm"),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: LoadedPluginKind::Wasm,
        disabled: false,
    };
    MawWasmHost::new(&loaded).with_fs_root("sandbox", dir)
}

fn call(host: &MawWasmHost, name: &str, args: &Value) -> Value {
    serde_json::from_str(&host.handle_json(name, &args.to_string())).expect("host result json")
}

#[test]
fn paths_get_returns_allowlisted_names() {
    let dir = temp("paths-allow");
    let host = host(&dir, &[]).with_paths(
        Some("/work/here".to_owned()),
        Some("/home/tester".to_owned()),
    );

    let home = call(&host, "maw.paths.get", &json!({ "name": "home" }));
    assert_eq!(home["ok"], true, "{home}");
    assert_eq!(home["value"]["path"], "/home/tester");

    let cwd = call(&host, "maw.paths.get", &json!({ "name": "cwd" }));
    assert_eq!(cwd["ok"], true, "{cwd}");
    assert_eq!(cwd["value"]["path"], "/work/here");

    let teams = call(&host, "maw.paths.get", &json!({ "name": "teams" }));
    assert_eq!(teams["ok"], true, "{teams}");
    assert_eq!(teams["value"]["path"], "/home/tester/.claude/teams");
}

#[test]
fn paths_get_denies_unknown_names() {
    let dir = temp("paths-deny");
    let host = host(&dir, &[]).with_paths(
        Some("/work/here".to_owned()),
        Some("/home/tester".to_owned()),
    );

    for name in ["secrets", "env", "path", "..", "HOME", ""] {
        let denied = call(&host, "maw.paths.get", &json!({ "name": name }));
        assert_eq!(
            denied["ok"], false,
            "name {name:?} must be denied: {denied}"
        );
        assert_eq!(denied["code"], "invalid_args", "{denied}");
    }
}

#[test]
fn paths_get_reports_not_found_when_context_missing() {
    let dir = temp("paths-missing");
    // No with_paths: cwd/home are unset in this context.
    let host = host(&dir, &[]);

    let home = call(&host, "maw.paths.get", &json!({ "name": "home" }));
    assert_eq!(home["ok"], false, "{home}");
    assert_eq!(home["code"], "not_found", "{home}");
}

#[test]
fn exec_injects_home_only_with_exec_home_capability() {
    let dir = temp("exec-home-granted");
    let host = host(&dir, &["proc:exec:env", "fs:read:sandbox", "exec:home"])
        .with_paths(None, Some("/home/injected".to_owned()));

    let out = call(
        &host,
        "maw.exec.run",
        &json!({ "cmd": "env", "cwd": dir, "allowNonZero": true }),
    );
    assert_eq!(out["ok"], true, "{out}");
    let stdout = out["value"]["stdout"].as_str().unwrap_or_default();
    assert!(
        stdout.contains("HOME=/home/injected"),
        "HOME must be injected with exec:home cap: {stdout}"
    );
}

#[test]
fn exec_omits_home_without_exec_home_capability() {
    let dir = temp("exec-home-denied");
    let host = host(&dir, &["proc:exec:env", "fs:read:sandbox"])
        .with_paths(None, Some("/home/injected".to_owned()));

    let out = call(
        &host,
        "maw.exec.run",
        &json!({ "cmd": "env", "cwd": dir, "allowNonZero": true }),
    );
    assert_eq!(out["ok"], true, "{out}");
    let stdout = out["value"]["stdout"].as_str().unwrap_or_default();
    assert!(
        !stdout.contains("HOME="),
        "HOME must be absent without exec:home cap: {stdout}"
    );
}
