#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    load_manifest_from_dir, parse_cli, parse_manifest, CliFlagKind, LoadedPluginKind, PluginTier,
};
use serde_json::json;

#[test]
fn sparse_cli_capability_manifest_and_wasm_only_load_cover_line_edges() {
    let root = temp_dir("sparse-success");

    let cli = parse_cli(&json!({ "cli": { "command": "demo" } }))
        .expect("valid sparse cli")
        .expect("cli present");
    assert_eq!(cli.flags, None);

    let flagged = parse_manifest(
        &json!({
            "name": "typed-cli",
            "version": "1.0.0",
            "sdk": "*",
            "tier": "extra",
            "cli": { "command": "typed", "flags": { "--count": "number" } }
        })
        .to_string(),
        &root,
    )
    .expect("number flag manifest parses");
    assert_eq!(flagged.tier, Some(PluginTier::Extra));
    assert_eq!(
        flagged.cli.expect("cli").flags.expect("flags")["--count"],
        CliFlagKind::Number
    );

    let parsed = parse_manifest(
        &json!({
            "name": "cap-demo",
            "version": "1.0.0",
            "sdk": "*",
            "capabilityNamespaces": ["custom"],
            "capabilities": ["custom:thing"]
        })
        .to_string(),
        &root,
    )
    .expect("capability manifest parses");
    assert_eq!(
        parsed.capability_namespaces,
        Some(vec!["custom".to_owned()])
    );
    assert_eq!(parsed.capabilities, Some(vec!["custom:thing".to_owned()]));
    assert!(parsed.capability_warnings.is_empty());

    let wasm_dir = root.join("wasm-only");
    create_dir_all(&wasm_dir).expect("create wasm-only plugin dir");
    write(
        wasm_dir.join("plugin.json"),
        r#"{"name":"wasm-only","version":"1.0.0","sdk":"*"}"#,
    )
    .expect("write manifest");
    let loaded = load_manifest_from_dir(&wasm_dir)
        .expect("load wasm-only manifest")
        .expect("plugin present");
    assert_eq!(loaded.kind, LoadedPluginKind::Wasm);
    assert_eq!(loaded.entry_path, None);

    remove_dir_all(root).expect("cleanup");
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-manifest-line-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}
