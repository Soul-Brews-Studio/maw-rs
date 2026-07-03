use std::collections::BTreeMap;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    import_plugin_symbol, load_manifest_from_dir, parse_manifest, scan_dirs, LoadedPlugin,
    LoadedPluginKind, PluginManifest, PluginModule,
};
use serde_json::json;

#[test]
fn parse_manifest_propagates_nested_section_errors_from_full_parse() {
    let root = temp_dir("nested-errors");
    for (name, extra, expected) in [
        (
            "bad-namespaces",
            json!({"capabilityNamespaces": [""]}),
            "capabilityNamespaces",
        ),
        ("bad-tier", json!({"tier": 99}), "tier"),
        ("bad-cli", json!({"cli": {"command": ""}}), "cli.command"),
        (
            "bad-api",
            json!({"api": {"path": "", "methods": ["GET"]}}),
            "api.path",
        ),
        ("bad-hooks", json!({"hooks": {"gate": "x"}}), "hooks.gate"),
        (
            "bad-cron",
            json!({"cron": {"schedule": ""}}),
            "cron.schedule",
        ),
        (
            "bad-module",
            json!({"module": {"path": "mod.ts", "exports": []}}),
            "module.exports",
        ),
        (
            "bad-transport",
            json!({"transport": {"peer": "yes"}}),
            "transport.peer",
        ),
        (
            "bad-engine",
            json!({"engine": {"serve": {"events": "bad"}}}),
            "engine.serve.events",
        ),
        (
            "bad-deps",
            json!({"dependencies": {"plugins": [""]}}),
            "dependencies.plugins",
        ),
    ] {
        let mut manifest = json!({"name": name, "version": "1.0.0", "sdk": "*"});
        manifest
            .as_object_mut()
            .expect("object")
            .extend(extra.as_object().expect("extra object").clone());
        let error = parse_manifest(&manifest.to_string(), &root).expect_err("nested error");
        assert!(error.contains(expected), "{error:?} missing {expected:?}");
    }
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn load_manifest_reports_plugin_json_read_error() {
    let root = temp_dir("read-error");
    create_dir_all(root.join("plugin.json")).expect("plugin.json directory");

    let error = load_manifest_from_dir(&root).expect_err("read directory as manifest");

    assert!(error.contains("plugin.json: failed to read"), "{error}");
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn import_symbol_propagates_loader_and_canonicalize_errors() {
    let root = temp_dir("import-errors");
    let plugin_dir = root.join("plugin");
    create_dir_all(&plugin_dir).expect("plugin dir");
    write(plugin_dir.join("mod.ts"), "export const ok = 1;\n").expect("module");
    let plugin = loaded_plugin(&plugin_dir, Some("mod.ts"));

    let loader_error = import_plugin_symbol("demo", "ok", std::slice::from_ref(&plugin), |_| {
        Err("loader exploded".to_owned())
    })
    .expect_err("loader error");
    assert_eq!(loader_error, "loader exploded");

    let missing_file = loaded_plugin(&plugin_dir, Some("missing.ts"));
    let canonical_error =
        import_plugin_symbol("demo", "ok", &[missing_file], |_| Ok(BTreeMap::new()))
            .expect_err("canonicalize missing module");
    assert!(!canonical_error.is_empty());

    let missing_root = loaded_plugin(&root.join("missing-plugin"), Some("mod.ts"));
    let root_error = import_plugin_symbol("demo", "ok", &[missing_root], |_| Ok(BTreeMap::new()))
        .expect_err("canonicalize missing root");
    assert!(!root_error.is_empty());

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn scan_dirs_uses_explicit_plugins_dir_env() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let original = std::env::var_os("MAW_PLUGINS_DIR");
    let original_maw_home = std::env::var_os("MAW_HOME");
    let original_home = std::env::var_os("HOME");
    let explicit = temp_dir("explicit-env").join("plugins");

    std::env::set_var("MAW_PLUGINS_DIR", &explicit);
    assert_eq!(scan_dirs(), vec![explicit.clone()]);

    std::env::remove_var("MAW_PLUGINS_DIR");
    std::env::remove_var("MAW_HOME");
    std::env::remove_var("HOME");
    assert_eq!(scan_dirs(), vec![PathBuf::from(".maw/plugins")]);

    restore_env("MAW_PLUGINS_DIR", original);
    restore_env("MAW_HOME", original_maw_home);
    restore_env("HOME", original_home);
    let _ = remove_dir_all(explicit.parent().expect("temp root"));
}

fn loaded_plugin(dir: &Path, module_path: Option<&str>) -> LoadedPlugin {
    LoadedPlugin {
        manifest: PluginManifest {
            name: "demo".to_owned(),
            version: "1.0.0".to_owned(),
            weight: None,
            tier: None,
            wasm: None,
            entry: None,
            entry_export: None,
            sdk: "*".to_owned(),
            cli: None,
            api: None,
            description: None,
            author: None,
            hooks: None,
            cron: None,
            module: module_path.map(|path| PluginModule {
                path: path.to_owned(),
                exports: vec!["ok".to_owned()],
            }),
            transport: None,
            engine: None,
            target: None,
            capability_namespaces: None,
            capabilities: None,
            capability_warnings: Vec::new(),
            dependencies: None,
            artifact: None,
        },
        dir: dir.to_path_buf(),
        wasm_path: PathBuf::new(),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: LoadedPluginKind::Ts,
        disabled: false,
    }
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
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
        "maw-rs-manifest-tail-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}
