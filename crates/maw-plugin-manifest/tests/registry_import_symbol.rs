use std::collections::BTreeMap;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    import_plugin_symbol, reset_discover_cache, LoadedPlugin, LoadedPluginKind, PluginManifest,
    PluginModule,
};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn make_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-plugin-symbol-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn import_plugin_symbol_returns_whitelisted_named_exports() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

    let root = make_temp_dir("happy");
    let dir = root.join("helper");
    create_dir_all(&dir).expect("plugin dir");
    write(dir.join("lib.ts"), b"export const answer = 42;\n").expect("module");
    let plugin = make_plugin(
        &dir,
        Some(PluginModule {
            path: "./lib.ts".to_owned(),
            exports: vec!["answer".to_owned(), "greet".to_owned()],
        }),
        false,
    );

    let answer = import_plugin_symbol("helper", "answer", std::slice::from_ref(&plugin), |_| {
        Ok(BTreeMap::from([
            ("answer".to_owned(), "42".to_owned()),
            ("greet".to_owned(), "hi Nat".to_owned()),
        ]))
    })
    .expect("answer");
    let greet = import_plugin_symbol("helper", "greet", &[plugin], |_| {
        Ok(BTreeMap::from([("greet".to_owned(), "hi Nat".to_owned())]))
    })
    .expect("greet");

    assert_eq!(answer, "42");
    assert_eq!(greet, "hi Nat");
    remove_dir_all(root).expect("cleanup");
    reset_discover_cache();
}

#[test]
fn import_plugin_symbol_rejects_missing_names_before_loading() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

    let no_plugins = Vec::new();
    assert_error(
        import_plugin_symbol("", "thing", &no_plugins, |_| Ok(BTreeMap::new())),
        "pluginName is required",
    );
    assert_error(
        import_plugin_symbol("helper", "", &no_plugins, |_| Ok(BTreeMap::new())),
        "symbolName is required",
    );
}

#[test]
fn import_plugin_symbol_rejects_absent_or_disabled_plugins() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

    let root = make_temp_dir("disabled");
    let dir = root.join("helper");
    create_dir_all(&dir).expect("plugin dir");
    write(dir.join("lib.ts"), b"export const answer = 42;\n").expect("module");
    let disabled = make_plugin(
        &dir,
        Some(PluginModule {
            path: "./lib.ts".to_owned(),
            exports: vec!["answer".to_owned()],
        }),
        true,
    );

    assert_error(
        import_plugin_symbol("missing", "answer", &[], |_| Ok(BTreeMap::new())),
        "plugin 'missing' not found",
    );
    assert_error(
        import_plugin_symbol("helper", "answer", &[disabled], |_| Ok(BTreeMap::new())),
        "plugin 'helper' is disabled",
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn import_plugin_symbol_rejects_missing_module_surface_and_private_symbols() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

    let root = make_temp_dir("surface");
    let dir = root.join("helper");
    create_dir_all(&dir).expect("plugin dir");
    write(dir.join("lib.ts"), b"export const publicThing = true;\n").expect("module");
    let no_module = make_plugin(&dir, None, false);
    let private_symbol = make_plugin(
        &dir,
        Some(PluginModule {
            path: "./lib.ts".to_owned(),
            exports: vec!["publicThing".to_owned()],
        }),
        false,
    );

    assert_error(
        import_plugin_symbol("helper", "publicThing", &[no_module], |_| {
            Ok(BTreeMap::new())
        }),
        "does not declare a module surface",
    );
    assert_error(
        import_plugin_symbol("helper", "privateThing", &[private_symbol], |_| {
            Ok(BTreeMap::new())
        }),
        "does not export 'privateThing'",
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn import_plugin_symbol_rejects_module_paths_that_escape_plugin_dir() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

    let root = make_temp_dir("escape");
    let dir = root.join("helper");
    create_dir_all(&dir).expect("plugin dir");
    write(root.join("outside.ts"), b"export const secret = 7;\n").expect("outside");
    let plugin = make_plugin(
        &dir,
        Some(PluginModule {
            path: "../outside.ts".to_owned(),
            exports: vec!["secret".to_owned()],
        }),
        false,
    );

    assert_error(
        import_plugin_symbol("helper", "secret", &[plugin], |_| Ok(BTreeMap::new())),
        "module.path escapes plugin dir",
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn import_plugin_symbol_rejects_runtime_module_missing_allowlisted_export() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

    let root = make_temp_dir("missing-export");
    let dir = root.join("helper");
    create_dir_all(&dir).expect("plugin dir");
    write(dir.join("lib.ts"), b"export const other = true;\n").expect("module");
    let plugin = make_plugin(
        &dir,
        Some(PluginModule {
            path: "./lib.ts".to_owned(),
            exports: vec!["missing".to_owned()],
        }),
        false,
    );

    assert_error(
        import_plugin_symbol("helper", "missing", &[plugin], |_| {
            Ok(BTreeMap::from([("other".to_owned(), "true".to_owned())]))
        }),
        "module did not provide export 'missing'",
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn import_plugin_symbol_caches_successful_symbol_imports_until_reset() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

    let root = make_temp_dir("cache");
    let dir = root.join("helper");
    create_dir_all(&dir).expect("plugin dir");
    write(dir.join("lib.ts"), b"export const stamp = Math.random();\n").expect("module");
    let plugin = make_plugin(
        &dir,
        Some(PluginModule {
            path: "./lib.ts".to_owned(),
            exports: vec!["stamp".to_owned()],
        }),
        false,
    );
    reset_discover_cache();

    let mut load_calls = 0;
    let first = import_plugin_symbol("helper", "stamp", std::slice::from_ref(&plugin), |_| {
        load_calls += 1;
        Ok(BTreeMap::from([("stamp".to_owned(), "first".to_owned())]))
    })
    .expect("first");
    let second = import_plugin_symbol("helper", "stamp", std::slice::from_ref(&plugin), |_| {
        load_calls += 1;
        Ok(BTreeMap::from([("stamp".to_owned(), "second".to_owned())]))
    })
    .expect("second");

    assert_eq!(first, "first");
    assert_eq!(second, "first");
    assert_eq!(load_calls, 1);

    reset_discover_cache();
    let third = import_plugin_symbol("helper", "stamp", &[plugin], |_| {
        Ok(BTreeMap::from([("stamp".to_owned(), "third".to_owned())]))
    })
    .expect("third");
    assert_eq!(third, "third");

    remove_dir_all(root).expect("cleanup");
    reset_discover_cache();
}

fn make_plugin(dir: &Path, module: Option<PluginModule>, disabled: bool) -> LoadedPlugin {
    LoadedPlugin {
        manifest: PluginManifest {
            name: "helper".to_owned(),
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
            module,
            transport: None,
            engine: None,
            target: None,
            capability_namespaces: None,
            capabilities: None,
            endpoints: None,
            secrets: None,
            capability_warnings: Vec::new(),
            dependencies: None,
            artifact: None,
        },
        dir: dir.to_path_buf(),
        wasm_path: PathBuf::new(),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: LoadedPluginKind::Ts,
        disabled,
    }
}

fn assert_error(result: Result<String, String>, expected: &str) {
    let error = result.expect_err("expected error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}
