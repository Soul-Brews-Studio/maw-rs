use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{parse_manifest, ApiMethod, HookPolicy, PluginTarget};
use serde_json::json;

fn make_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-manifest-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn parse_manifest_happy_path_matches_maw_js_tests() {
    let dir = make_temp_dir("happy");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");
    let manifest = parse_manifest(
        &json!({ "name": "hello-plugin", "version": "1.0.0", "wasm": "plugin.wasm", "sdk": "^1.0.0" }).to_string(),
        &dir,
    )
    .expect("minimal manifest");
    assert_eq!(manifest.name, "hello-plugin");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.wasm, Some("plugin.wasm".to_owned()));
    assert_eq!(manifest.sdk, "^1.0.0");
    assert_eq!(manifest.cli, None);

    let wasm_target = parse_manifest(
        &json!({ "name": "wasm-target", "version": "1.0.0", "wasm": "plugin.wasm", "sdk": "*", "target": "wasm" })
            .to_string(),
        &dir,
    )
    .expect("target wasm manifest");
    assert_eq!(wasm_target.target, Some(PluginTarget::Wasm));
    assert_eq!(wasm_target.wasm, Some("plugin.wasm".to_owned()));

    let wasm_entry_target = parse_manifest(
        &json!({ "name": "wasm-entry-target", "version": "1.0.0", "entry": "plugin.wasm", "sdk": "*", "target": "wasm" })
            .to_string(),
        &dir,
    )
    .expect("target wasm entry manifest");
    assert_eq!(wasm_entry_target.target, Some(PluginTarget::Wasm));
    assert_eq!(wasm_entry_target.entry, Some("plugin.wasm".to_owned()));

    let manifest = parse_manifest(
        &json!({
            "name": "full-plugin",
            "version": "2.3.4",
            "wasm": "plugin.wasm",
            "sdk": "~1.2.0",
            "weight": 25,
            "cli": { "command": "greet", "help": "Say hello" },
            "api": { "path": "/greet", "methods": ["GET", "POST"] },
            "description": "A greeting plugin",
            "author": "Nat"
        })
        .to_string(),
        &dir,
    )
    .expect("full manifest");
    assert_eq!(manifest.weight, Some(25));
    assert_eq!(manifest.cli.expect("cli").command, "greet");
    assert_eq!(
        manifest.api.expect("api").methods,
        vec![ApiMethod::Get, ApiMethod::Post]
    );
    assert_eq!(manifest.description, Some("A greeting plugin".to_owned()));
    assert_eq!(manifest.author, Some("Nat".to_owned()));

    remove_dir_all(dir).expect("cleanup");
}

#[test]
fn parse_manifest_preserves_lifecycle_and_v1_fields() {
    let dir = make_temp_dir("v1");
    write(dir.join("index.ts"), b"export default () => {};\n").expect("entry");
    let manifest = parse_manifest(
        &json!({
            "name": "life-plugin",
            "version": "1.0.0",
            "entry": "index.ts",
            "sdk": "*",
            "target": "js",
            "capabilityNamespaces": ["messages", "messages", "storage"],
            "capabilities": ["sdk:identity", "messages:ledger"],
            "artifact": { "path": "dist/index.js", "sha256": null },
            "hooks": {
                "on": ["MessageSend"],
                "wake": { "script": "setup.ts", "handler": "onWake", "ensures": ["storage:sqlite"], "policy": "best-effort" },
                "sleep": { "handler": "onSleep" },
                "serve": { "script": "serve.ts", "policy": "fail-fast" }
            }
        })
        .to_string(),
        &dir,
    )
    .expect("manifest");
    assert_eq!(manifest.target, Some(PluginTarget::Js));
    assert_eq!(
        manifest.capability_namespaces,
        Some(vec!["messages".to_owned(), "storage".to_owned()])
    );
    assert_eq!(
        manifest.capabilities,
        Some(vec![
            "sdk:identity".to_owned(),
            "messages:ledger".to_owned()
        ])
    );
    assert!(manifest.capability_warnings.is_empty());
    assert_eq!(manifest.artifact.expect("artifact").sha256, None);
    let hooks = manifest.hooks.expect("hooks");
    assert_eq!(hooks.on, Some(vec!["MessageSend".to_owned()]));
    assert_eq!(
        hooks.wake.expect("wake").policy,
        Some(HookPolicy::BestEffort)
    );

    let bare = parse_manifest(
        &json!({ "name": "bare", "version": "1.0.0", "sdk": "*" }).to_string(),
        &dir,
    )
    .expect("bare manifest");
    assert_eq!(bare.wasm, None);
    assert_eq!(bare.entry, None);
    assert_eq!(bare.artifact, None);
    let manifest = parse_manifest(
        &json!({ "name": "endpoint-plugin", "version": "1.0.0", "entry": "index.ts", "sdk": "*", "capabilities": ["net:fetch:discord-rest"], "endpoints": { "discord-rest": { "baseUrl": "https://discord.com/api/v10", "paths": ["/channels/*/messages"] } } }).to_string(),
        &dir,
    )
    .expect("endpoint manifest");
    let endpoint = manifest
        .endpoints
        .expect("endpoints")
        .remove("discord-rest")
        .expect("policy");
    let pattern = &endpoint.paths[0];
    assert!(pattern.matches("/channels/123/messages"));
    for path in [
        "/channels/../messages",
        "https://evil.test/channels/123/messages",
        "//evil.test/channels/123/messages",
        "/channels/https://evil.test/messages",
    ] {
        assert!(!pattern.matches(path), "{path} should not match");
    }
    remove_dir_all(dir).expect("cleanup");
}

#[test]
fn parse_manifest_validation_failures_match_maw_js_tests() {
    let dir = make_temp_dir("failures");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");
    write(dir.join("index.ts"), b"export default () => {};\n").expect("entry");

    expect_manifest_error("not json!", &dir, "plugin.json: invalid JSON");
    expect_manifest_error("null", &dir, "plugin.json: must be a JSON object");
    expect_manifest_error("[]", &dir, "plugin.json: must be a JSON object");
    expect_manifest_error(
        &json!({ "name": "Hello_Plugin!", "version": "1.0.0", "wasm": "plugin.wasm", "sdk": "*" })
            .to_string(),
        &dir,
        "plugin.json: name must match",
    );
    expect_manifest_error(
        &json!({ "name": "my-plugin", "version": "not-semver", "wasm": "plugin.wasm", "sdk": "*" })
            .to_string(),
        &dir,
        "plugin.json: version must be semver",
    );
    expect_manifest_error(
        &json!({ "name": "bad-weight", "version": "1.0.0", "wasm": "plugin.wasm", "sdk": "*", "weight": 100 }).to_string(),
        &dir,
        "plugin.json: weight must be a number 0-99",
    );
    expect_manifest_error(
        &json!({ "name": "my-plugin", "version": "1.0.0", "wasm": "missing.wasm", "sdk": "*" })
            .to_string(),
        &dir,
        "plugin.json: wasm file not found",
    );
    expect_manifest_error(
        &json!({ "name": "my-plugin", "version": "1.0.0", "entry": "missing.ts", "sdk": "*" })
            .to_string(),
        &dir,
        "plugin.json: entry file not found",
    );
    expect_manifest_error(
        &json!({ "name": "my-plugin", "version": "1.0.0", "wasm": "plugin.wasm", "sdk": "not-a-range" }).to_string(),
        &dir,
        "plugin.json: sdk must be a semver range",
    );
    expect_manifest_error(
        &json!({ "name": "too-early", "version": "1.0.0", "entry": "index.ts", "sdk": "*", "target": "wasm" }).to_string(),
        &dir,
        "Phase C",
    );
    expect_manifest_error(
        &json!({ "name": "bad-caps", "version": "1.0.0", "entry": "index.ts", "sdk": "*", "capabilities": "sdk:identity" }).to_string(),
        &dir,
        "plugin.json: capabilities must be an array of strings",
    );
    expect_manifest_error(
        &json!({ "name": "missing-endpoint", "version": "1.0.0", "entry": "index.ts", "sdk": "*", "capabilities": ["net:fetch:discord-rest"], "endpoints": { "hermes-api": { "baseUrl": "http://127.0.0.1:8642", "paths": ["/health"] } } }).to_string(),
        &dir,
        "references missing endpoint",
    );
    expect_manifest_error(
        &json!({ "name": "bad-endpoint-path", "version": "1.0.0", "entry": "index.ts", "sdk": "*", "endpoints": { "bad-api": { "baseUrl": "https://example.test", "paths": ["/channels/**"] } } }).to_string(),
        &dir,
        "only full-segment * wildcards",
    );
    expect_manifest_error(
        &json!({ "name": "bad-art", "version": "1.0.0", "entry": "index.ts", "sdk": "*", "artifact": "dist/index.js" }).to_string(),
        &dir,
        "plugin.json: artifact must be an object",
    );

    remove_dir_all(dir).expect("cleanup");
}

fn expect_manifest_error(json_text: &str, dir: &std::path::Path, expected: &str) {
    let error = parse_manifest(json_text, dir).expect_err("expected parse_manifest error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

#[test]
fn parse_manifest_accepts_wasm_entry_object_export() {
    let root = make_temp_dir("entry-object-export");
    write(root.join("plugin.wasm"), b"wasm").expect("wasm");
    let manifest = parse_manifest(
        r#"{"name":"entry-export","version":"1.0.0","sdk":"*","entry":{"kind":"wasm","path":"plugin.wasm","export":"run"}}"#,
        &root,
    )
    .expect("manifest");
    assert_eq!(manifest.entry.as_deref(), Some("plugin.wasm"));
    assert_eq!(manifest.entry_export.as_deref(), Some("run"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn parse_manifest_rejects_empty_wasm_entry_export() {
    let root = make_temp_dir("entry-object-bad-export");
    write(root.join("plugin.wasm"), b"wasm").expect("wasm");
    let err = parse_manifest(
        r#"{"name":"entry-export","version":"1.0.0","sdk":"*","entry":{"kind":"wasm","path":"plugin.wasm","export":""}}"#,
        &root,
    )
    .unwrap_err();
    assert_eq!(err, "plugin.json: entry.export must be a non-empty string");
    remove_dir_all(root).expect("cleanup");
}
