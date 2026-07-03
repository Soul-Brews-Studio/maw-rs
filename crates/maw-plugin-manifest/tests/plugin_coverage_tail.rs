use std::collections::BTreeMap;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    import_plugin_symbol, invoke_plugin, load_manifest_from_dir, parse_api, parse_cli, parse_cron,
    reset_discover_cache, InvokeContext, InvokeResult, InvokeSource, LoadedPlugin,
    LoadedPluginKind, MvpWasmInvokeRuntime, PluginManifest, PluginModule,
};
use serde_json::json;

#[test]
fn optional_manifest_error_tails_cover_direct_parsers() {
    assert_eq!(
        parse_cli(&json!({ "cli": {} })).expect_err("missing command"),
        "plugin.json: cli.command must be a non-empty string"
    );
    assert_eq!(parse_api(&json!({})).expect("absent api"), None);
    assert_eq!(
        parse_api(&json!({ "api": [] })).expect_err("api shape"),
        "plugin.json: api must be an object"
    );
    assert_eq!(
        parse_cron(&json!({ "cron": [] })).expect_err("cron shape"),
        "plugin.json: cron must be an object"
    );
}

#[test]
fn relative_manifest_paths_and_symbol_cache_tail_are_exercised() {
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = CWD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reset_discover_cache();
    let original = std::env::current_dir().expect("cwd");
    let root = temp_dir("relative-cache");
    let plugin_dir = root.join("plugins/helper");
    create_dir_all(&plugin_dir).expect("plugin dir");
    write(plugin_dir.join("index.ts"), "export const answer = 42;\n").expect("module");
    write(
        plugin_dir.join("plugin.json"),
        json!({
            "name": "helper",
            "version": "1.0.0",
            "sdk": "*",
            "entry": "index.ts",
            "module": { "path": "index.ts", "exports": ["answer"] }
        })
        .to_string(),
    )
    .expect("manifest");

    std::env::set_current_dir(&root).expect("set cwd");
    let loaded = load_manifest_from_dir(Path::new("plugins/helper"))
        .expect("load relative manifest")
        .expect("plugin present");

    assert!(loaded.entry_path.as_ref().expect("entry").is_absolute());
    let first = import_plugin_symbol("helper", "answer", std::slice::from_ref(&loaded), |_| {
        Ok(BTreeMap::from([("answer".to_owned(), "first".to_owned())]))
    })
    .expect("first import");
    let second = import_plugin_symbol("helper", "answer", std::slice::from_ref(&loaded), |_| {
        Ok(BTreeMap::from([("answer".to_owned(), "second".to_owned())]))
    })
    .expect("cached import");

    assert_eq!((first.as_str(), second.as_str()), ("first", "first"));
    reset_discover_cache();
    std::env::set_current_dir(&original).expect("restore cwd");
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn import_symbol_rejects_disabled_and_unexported_tail_paths() {
    let root = temp_dir("import-errors");
    let plugin_dir = root.join("plugin");
    create_dir_all(&plugin_dir).expect("plugin dir");
    write(plugin_dir.join("mod.ts"), "export const ok = true;\n").expect("module");
    let mut disabled = plugin(&plugin_dir, Some("mod.ts"));
    disabled.disabled = true;
    assert_eq!(
        import_plugin_symbol("helper", "ok", &[disabled], |_| Ok(BTreeMap::new()))
            .expect_err("disabled"),
        "plugin 'helper' is disabled"
    );
    assert_eq!(
        import_plugin_symbol(
            "helper",
            "missing",
            &[plugin(&plugin_dir, Some("mod.ts"))],
            |_| { Ok(BTreeMap::new()) }
        )
        .expect_err("missing export"),
        "plugin 'helper' does not export 'missing'"
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn wasm_tail_segments_cover_imports_signed_offsets_and_length_prefix_output() {
    let root = temp_dir("wasm-tail");
    let import_result = invoke_plugin(
        &write_wasm_plugin(
            &root,
            "imported",
            &wasm_module(
                vec![import_entry("env", "f", 0x00, vec![0x00])],
                1,
                exports(&[("memory", 0x02, 0), ("handle", 0x00, 1)]),
                code(&[body(&[0x00, 0x41, 0x00, 0x0b])]),
                vec![],
            ),
        ),
        &cli(),
        &mut MvpWasmInvokeRuntime,
    );
    assert!(import_result
        .error
        .unwrap_or_default()
        .contains("unresolved imports"));

    let negative_offset = invoke_plugin(
        &write_wasm_plugin(
            &root,
            "negative-offset",
            &valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x41, 0x7f, 0x0b, 0x00]]),
            ),
        ),
        &cli(),
        &mut MvpWasmInvokeRuntime,
    );
    assert!(negative_offset
        .error
        .unwrap_or_default()
        .contains("wasm compile error"));

    assert_eq!(
        invoke_plugin(
            &write_wasm_plugin(
                &root,
                "length-prefix",
                &valid_wasm_with_body(
                    &const_i32_body(65_535),
                    data_section(vec![extend(data_i32_offset_prefix(65_535), &[1, b'o'])]),
                ),
            ),
            &cli(),
            &mut MvpWasmInvokeRuntime,
        ),
        InvokeResult::output("o")
    );
    remove_dir_all(root).expect("cleanup");
}

fn plugin(dir: &Path, module_path: Option<&str>) -> LoadedPlugin {
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
        wasm_path: dir.join("helper.wasm"),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: LoadedPluginKind::Ts,
        disabled: false,
    }
}

fn write_wasm_plugin(dir: &Path, name: &str, bytes: &[u8]) -> LoadedPlugin {
    let plugin = LoadedPlugin {
        manifest: PluginManifest {
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            weight: None,
            tier: None,
            wasm: Some(format!("{name}.wasm")),
            entry: None,
            entry_export: None,
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
            target: None,
            capability_namespaces: None,
            capabilities: None,
            capability_warnings: Vec::new(),
            dependencies: None,
            artifact: None,
        },
        dir: dir.to_path_buf(),
        wasm_path: dir.join(format!("{name}.wasm")),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: LoadedPluginKind::Wasm,
        disabled: false,
    };
    write(&plugin.wasm_path, bytes).expect("wasm");
    plugin
}

fn cli() -> InvokeContext {
    InvokeContext {
        source: InvokeSource::Cli,
        args: Vec::new(),
        cwd: None,
        home: None,
    }
}

fn valid_wasm_with_body(body_bytes: &[u8], data: Vec<u8>) -> Vec<u8> {
    wasm_module(
        vec![],
        1,
        exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
        code(&[body(body_bytes)]),
        data,
    )
}

fn wasm_module(
    imports: Vec<Vec<u8>>,
    func_count: u32,
    exports: Vec<u8>,
    code: Vec<u8>,
    data: Vec<u8>,
) -> Vec<u8> {
    let mut sections = vec![
        (1, vec![0x01, 0x60, 0x00, 0x01, 0x7f]),
        (5, vec![0x01, 0x00, 0x01]),
    ];
    if !imports.is_empty() {
        sections.push((2, vector(imports)));
    }
    sections.push((3, section_indices(func_count)));
    sections.push((7, exports));
    sections.push((10, code));
    if !data.is_empty() {
        sections.push((11, data));
    }
    wasm(&sections)
}

fn wasm(sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    for (id, section) in sections {
        bytes.push(*id);
        bytes.extend(leb(len_u32(section.len())));
        bytes.extend(section);
    }
    bytes
}

fn section_indices(count: u32) -> Vec<u8> {
    let mut bytes = leb(count);
    bytes.extend(std::iter::repeat_n(0x00, count as usize));
    bytes
}

fn exports(entries: &[(&str, u8, u32)]) -> Vec<u8> {
    vector(
        entries
            .iter()
            .map(|(name, kind, index)| {
                let mut bytes = wasm_name(name);
                bytes.push(*kind);
                bytes.extend(leb(*index));
                bytes
            })
            .collect(),
    )
}

fn import_entry(module: &str, name: &str, kind: u8, descriptor: Vec<u8>) -> Vec<u8> {
    let mut bytes = wasm_name(module);
    bytes.extend(wasm_name(name));
    bytes.push(kind);
    bytes.extend(descriptor);
    bytes
}

fn code(bodies: &[Vec<u8>]) -> Vec<u8> {
    vector(bodies.iter().map(|body| length_prefixed(body)).collect())
}
fn body(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}
fn data_section(segments: Vec<Vec<u8>>) -> Vec<u8> {
    vector(segments)
}
fn data_i32_offset_prefix(offset: u32) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x41];
    bytes.extend(leb(offset));
    bytes.push(0x0b);
    bytes
}
fn const_i32_body(value: u32) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x41];
    bytes.extend(leb(value));
    bytes.push(0x0b);
    bytes
}
fn vector(items: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = leb(len_u32(items.len()));
    for item in items {
        bytes.extend(item);
    }
    bytes
}
fn extend(mut bytes: Vec<u8>, tail: &[u8]) -> Vec<u8> {
    bytes.extend(tail);
    bytes
}
fn length_prefixed(bytes: &[u8]) -> Vec<u8> {
    let mut out = leb(len_u32(bytes.len()));
    out.extend(bytes);
    out
}
fn wasm_name(name: &str) -> Vec<u8> {
    length_prefixed(name.as_bytes())
}

fn len_u32(len: usize) -> u32 {
    u32::try_from(len).expect("test fixture length fits in u32")
}

fn leb(mut value: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            break;
        }
    }
    bytes
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-plugin-tail-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp dir");
    dir
}
