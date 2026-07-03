use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{load_manifest_from_dir, LoadedPluginKind};
use serde_json::json;

fn make_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-manifest-load-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn load_manifest_from_dir_returns_none_without_plugin_json() {
    let dir = make_temp_dir("missing");
    let loaded = load_manifest_from_dir(&dir).expect("load result");
    assert_eq!(loaded, None);
    remove_dir_all(dir).expect("cleanup");
}

#[test]
fn load_manifest_from_dir_resolves_wasm_plugin_like_maw_js() {
    let dir = make_temp_dir("wasm");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");
    write_manifest(
        &dir,
        &json!({ "name": "test-pkg", "version": "1.0.0", "wasm": "plugin.wasm", "sdk": "*" }),
    );

    let loaded = load_manifest_from_dir(&dir)
        .expect("load result")
        .expect("plugin");
    assert_eq!(loaded.manifest.name, "test-pkg");
    assert_eq!(loaded.dir, dir);
    assert_eq!(loaded.wasm_path, loaded.dir.join("plugin.wasm"));
    assert_eq!(loaded.entry_path, None);
    assert_eq!(loaded.kind, LoadedPluginKind::Wasm);

    remove_dir_all(loaded.dir).expect("cleanup");
}

#[test]
fn load_manifest_from_dir_raw_wasm_field_wins_over_ts_entry_and_js_artifact() {
    let dir = make_temp_dir("entry-precedence");
    write(
        dir.join("index.ts"),
        b"export default () => ({ ok: true });\n",
    )
    .expect("entry");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");
    write_manifest(
        &dir,
        &json!({
            "name": "source-first",
            "version": "1.0.0",
            "entry": "index.ts",
            "wasm": "plugin.wasm",
            "sdk": "*",
            "target": "js",
            "artifact": { "path": "dist/index.js", "sha256": null }
        }),
    );

    let loaded = load_manifest_from_dir(&dir)
        .expect("load result")
        .expect("plugin");
    assert_eq!(loaded.kind, LoadedPluginKind::Wasm);
    assert_eq!(loaded.entry_path, None);
    assert_eq!(loaded.wasm_path, loaded.dir.join("plugin.wasm"));

    remove_dir_all(loaded.dir).expect("cleanup");
}

#[test]
fn load_manifest_from_dir_uses_js_artifact_without_entry_or_wasm() {
    let dir = make_temp_dir("artifact");
    write_manifest(
        &dir,
        &json!({
            "name": "compiled",
            "version": "1.0.0",
            "sdk": "^1.0.0",
            "target": "js",
            "artifact": { "path": "dist/index.js", "sha256": null }
        }),
    );

    let loaded = load_manifest_from_dir(&dir)
        .expect("load result")
        .expect("plugin");
    assert_eq!(loaded.kind, LoadedPluginKind::Ts);
    assert_eq!(loaded.entry_path, Some(loaded.dir.join("dist/index.js")));
    assert_eq!(loaded.wasm_path, PathBuf::new());

    remove_dir_all(loaded.dir).expect("cleanup");
}

#[test]
fn load_manifest_from_dir_bubbles_manifest_validation_errors() {
    let dir = make_temp_dir("invalid");
    write_manifest(
        &dir,
        &json!({ "name": "bad", "version": "not-semver", "sdk": "*" }),
    );

    let error = load_manifest_from_dir(&dir).expect_err("validation error");
    assert!(error.contains("version"), "{error:?}");

    remove_dir_all(dir).expect("cleanup");
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    write(dir.join("plugin.json"), manifest.to_string()).expect("write manifest");
}
