#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::same_item_push
)]
use std::collections::BTreeSet;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    discover_packages, discover_packages_with_profile, import_plugin_symbol, invoke_plugin,
    load_manifest_from_dir, parse_cron, parse_manifest, parse_module, satisfies,
    DiscoverPackagesOptions, InvokeContext, InvokeResult, InvokeSource, LoadedPlugin,
    LoadedPluginKind, MvpWasmInvokeRuntime, PluginManifest,
};
use serde_json::{json, Map, Value};

#[test]
fn optional_manifest_sections_cover_absent_and_missing_member_edges() {
    let cron = parse_cron(&json!({ "cron": { "schedule": "*/5 * * * *" } }))
        .expect("cron parses")
        .expect("cron present");
    assert_eq!(cron.handler, None);

    assert_eq!(
        parse_module(&json!({ "module": { "path": "mod.ts" } })).expect_err("missing exports"),
        "plugin.json: module.exports must be a non-empty array of strings"
    );
}

#[test]
fn relative_manifest_dir_resolves_against_current_dir() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let original = std::env::current_dir().expect("cwd");
    let root = make_temp_dir("relative-dir");
    let plugin_dir = root.join("plugins/relative-plugin");
    create_dir_all(&plugin_dir).expect("plugin dir");
    write(
        plugin_dir.join("index.ts"),
        b"export default async function run() {}\n",
    )
    .expect("entry");
    write(
        plugin_dir.join("plugin.json"),
        json!({
            "name": "relative-plugin",
            "version": "1.0.0",
            "sdk": "*",
            "entry": "index.ts"
        })
        .to_string(),
    )
    .expect("manifest");

    std::env::set_current_dir(&root).expect("set cwd");
    let loaded = load_manifest_from_dir(Path::new("plugins/relative-plugin"))
        .expect("load")
        .expect("plugin");
    std::env::set_current_dir(original).expect("restore cwd");

    let entry_path = loaded.entry_path.expect("entry path");
    assert!(
        entry_path.is_absolute(),
        "entry path should be absolute: {entry_path:?}"
    );
    assert!(
        entry_path.ends_with("plugins/relative-plugin/index.ts"),
        "entry path should resolve relative dir under cwd: {entry_path:?}"
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn import_symbol_reports_missing_module_path_before_loading() {
    let root = make_temp_dir("missing-module-path");
    let plugin = loaded_plugin(&root, "moduleless", minimal_manifest("moduleless"));

    let err = import_plugin_symbol("moduleless", "thing", &[plugin], |_| {
        panic!("loader should not run")
    })
    .expect_err("missing module surface");

    assert_eq!(err, "plugin 'moduleless' does not declare a module surface");
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn registry_edges_cover_hash_read_errors_empty_roots_and_bad_overrides() {
    let root = make_temp_dir("registry-remaining");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");

    let hash_error_dir = plugins_dir.join("hash-error");
    create_dir_all(hash_error_dir.join("dist/index.js")).expect("artifact directory");
    write_manifest(
        &hash_error_dir,
        extend_manifest(
            minimal_object("hash-error"),
            [(
                "artifact",
                json!({ "path": "dist/index.js", "sha256": "sha256:expected" }),
            )],
        ),
    );

    let good_dir = plugins_dir.join("override-survivor");
    create_dir_all(&good_dir).expect("good dir");
    write(
        good_dir.join("index.ts"),
        b"export default async function run() {}\n",
    )
    .expect("entry");
    write_manifest(
        &good_dir,
        extend_manifest(
            minimal_object("override-survivor"),
            [("entry", json!("index.ts"))],
        ),
    );
    write(plugins_dir.join(".overrides.json"), b"not json").expect("bad overrides");

    let no_roots = discover_packages_with_profile(
        &DiscoverPackagesOptions {
            scan_dirs: Vec::new(),
            runtime_version: "1.0.0".to_owned(),
            ..DiscoverPackagesOptions::default()
        },
        |_| Some(BTreeSet::new()),
    );
    assert!(no_roots.plugins.is_empty());

    let report = discover_packages(&DiscoverPackagesOptions {
        scan_dirs: vec![plugins_dir],
        runtime_version: "1.0.0".to_owned(),
        ..DiscoverPackagesOptions::default()
    });

    assert_eq!(plugin_names(&report.plugins), vec!["override-survivor"]);
    assert!(
        report
            .warnings
            .join("\n")
            .contains("plugin 'hash-error' artifact hash failed"),
        "warnings: {:?}",
        report.warnings
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn semver_validation_covers_bad_build_missing_patch_and_caret_lower_bound() {
    assert_eq!(
        parse_manifest(
            &json!({ "name": "bad-build", "version": "1.2.3+", "sdk": "*" }).to_string(),
            Path::new("."),
        )
        .expect_err("bad build metadata"),
        "plugin.json: version must be semver N.N.N (got \"1.2.3+\")"
    );
    assert_eq!(
        parse_manifest(
            &json!({ "name": "bad-sdk", "version": "1.2.3", "sdk": "1.2" }).to_string(),
            Path::new("."),
        )
        .expect_err("missing patch"),
        "plugin.json: sdk must be a semver range (got \"1.2\")"
    );
    assert!(!satisfies("1.2.2", "^1.2.3"));
}

#[test]
fn mvp_wasm_parser_covers_remaining_error_and_import_shapes() {
    let root = make_temp_dir("wasm-remaining");
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("bad-magic", vec![0x00, 0x61, 0x73], "wasm compile error"),
        (
            "truncated-import",
            wasm(&[(2, vec![0x01])]),
            "wasm compile error",
        ),
        (
            "invalid-section-id",
            wasm(&[(13, vec![])]),
            "wasm compile error",
        ),
        (
            "invalid-export-name",
            wasm_module(
                vec![],
                0,
                vec![0x01, 0x01, 0xff, 0x02, 0x00],
                vec![],
                vec![],
            ),
            "wasm compile error",
        ),
        (
            "import-memory-global",
            wasm_module(
                vec![
                    import_entry("env", "mem", 0x02, vec![0x01, 0x01, 0x02]),
                    import_entry("env", "global", 0x03, vec![0x7f, 0x00]),
                ],
                1,
                exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
                code(&[body(&[0x00, 0x41, 0x00, 0x0b])]),
                vec![],
            ),
            "wasm instantiation failed: unresolved imports",
        ),
        (
            "import-table",
            wasm_module(
                vec![import_entry("env", "table", 0x01, vec![0x00, 0x01])],
                1,
                exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
                code(&[body(&[0x00, 0x41, 0x00, 0x0b])]),
                vec![],
            ),
            "wasm instantiation failed: unresolved imports",
        ),
        (
            "invalid-import-kind",
            wasm_module(
                vec![import_entry("env", "bad", 0x7f, vec![])],
                0,
                Vec::new(),
                Vec::new(),
                vec![],
            ),
            "wasm compile error",
        ),
        (
            "handle-is-import",
            wasm_module(
                vec![import_entry("env", "handle", 0x00, vec![0x00])],
                0,
                exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
                vec![0x00],
                vec![],
            ),
            "wasm compile error",
        ),
        (
            "handle-body-out-of-range",
            wasm_module(
                vec![],
                0,
                exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
                vec![0x00],
                vec![],
            ),
            "wasm compile error",
        ),
        (
            "bad-const-opcode",
            valid_wasm_with_body(&[0x00, 0x42, 0x00, 0x0b], vec![]),
            "wasm compile error",
        ),
        (
            "bad-const-end",
            valid_wasm_with_body(&[0x00, 0x41, 0x00, 0x00], vec![]),
            "wasm compile error",
        ),
        (
            "data-bad-flag",
            valid_wasm_with_body(&[0x00, 0x41, 0x00, 0x0b], data_section(vec![vec![0x01]])),
            "wasm compile error",
        ),
        (
            "data-bad-opcode",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x00]]),
            ),
            "wasm compile error",
        ),
        (
            "data-bad-end",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x41, 0x00, 0x00]]),
            ),
            "wasm compile error",
        ),
        (
            "data-negative-offset",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x41, 0x7f, 0x0b, 0x00]]),
            ),
            "wasm compile error",
        ),
        (
            "data-truncated-bytes",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x41, 0x00, 0x0b, 0x03, b'a']]),
            ),
            "wasm compile error",
        ),
        (
            "signed-overlong",
            valid_wasm_with_body(&[0x00, 0x41, 0x80, 0x80, 0x80, 0x80, 0x80], vec![]),
            "wasm compile error",
        ),
    ];

    for (name, bytes, expected) in cases {
        let plugin = write_wasm_plugin(&root, name, &bytes);
        let result = invoke_plugin(&plugin, &cli(), &mut MvpWasmInvokeRuntime);
        assert_error_contains(&result, expected, name);
    }

    let locals = write_wasm_plugin(
        &root,
        "locals",
        &valid_wasm_with_body(&[0x01, 0x01, 0x7f, 0x41, 0x00, 0x0b], vec![]),
    );
    assert_eq!(
        invoke_plugin(&locals, &cli(), &mut MvpWasmInvokeRuntime),
        InvokeResult::ok()
    );

    let negative = write_wasm_plugin(
        &root,
        "negative",
        &valid_wasm_with_body(&[0x00, 0x41, 0x7f, 0x0b], vec![]),
    );
    assert_eq!(
        invoke_plugin(&negative, &cli(), &mut MvpWasmInvokeRuntime),
        InvokeResult::ok()
    );

    let zero_length = write_wasm_plugin(
        &root,
        "zero-length",
        &valid_wasm_with_body(
            &[0x00, 0x41, 0xe4, 0x00, 0x0b],
            data_section(vec![vec![0x00, 0x41, 0xe4, 0x00, 0x0b, 0x04, 0, 0, 0, 0]]),
        ),
    );
    assert_eq!(
        invoke_plugin(&zero_length, &cli(), &mut MvpWasmInvokeRuntime),
        InvokeResult::ok()
    );

    remove_dir_all(root).expect("cleanup");
}

fn make_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-manifest-remaining-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

fn minimal_object(name: &str) -> Map<String, Value> {
    Map::from_iter([
        ("name".to_owned(), json!(name)),
        ("version".to_owned(), json!("1.0.0")),
        ("sdk".to_owned(), json!("*")),
    ])
}

fn extend_manifest(
    mut manifest: Map<String, Value>,
    entries: impl IntoIterator<Item = (&'static str, Value)>,
) -> Map<String, Value> {
    for (key, value) in entries {
        manifest.insert(key.to_owned(), value);
    }
    manifest
}

fn write_manifest(dir: &Path, manifest: Map<String, Value>) {
    create_dir_all(dir).expect("plugin dir");
    write(dir.join("plugin.json"), Value::Object(manifest).to_string()).expect("manifest");
}

fn minimal_manifest(name: &str) -> PluginManifest {
    PluginManifest {
        name: name.to_owned(),
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
        module: None,
        transport: None,
        engine: None,
        target: None,
        capability_namespaces: None,
        capabilities: None,
        endpoints: None,
        capability_warnings: Vec::new(),
        dependencies: None,
        artifact: None,
    }
}

fn loaded_plugin(root: &Path, name: &str, manifest: PluginManifest) -> LoadedPlugin {
    LoadedPlugin {
        manifest,
        dir: root.join(name),
        wasm_path: PathBuf::new(),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: LoadedPluginKind::Ts,
        disabled: false,
    }
}

fn write_wasm_plugin(dir: &Path, name: &str, bytes: &[u8]) -> LoadedPlugin {
    let wasm_path = dir.join(format!("{name}.wasm"));
    write(&wasm_path, bytes).expect("write wasm");
    let mut manifest = minimal_manifest(name);
    manifest.wasm = Some(format!("{name}.wasm"));
    LoadedPlugin {
        manifest,
        dir: dir.to_path_buf(),
        wasm_path,
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: LoadedPluginKind::Wasm,
        disabled: false,
    }
}

fn cli() -> InvokeContext {
    InvokeContext {
        source: InvokeSource::Cli,
        args: Vec::new(),
        cwd: None,
        home: None,
    }
}

fn plugin_names(plugins: &[LoadedPlugin]) -> Vec<&str> {
    plugins
        .iter()
        .map(|plugin| plugin.manifest.name.as_str())
        .collect()
}

fn assert_error_contains(result: &InvokeResult, expected: &str, name: &str) {
    let error = result.error.as_deref().unwrap_or_default();
    assert!(
        error.contains(expected),
        "{name}: {error:?} did not contain {expected:?}; result={result:?}"
    );
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
    export_body: Vec<u8>,
    code_body: Vec<u8>,
    data_body: Vec<u8>,
) -> Vec<u8> {
    let mut sections = vec![
        (1, vec![0x01, 0x60, 0x00, 0x01, 0x7f]),
        (5, vec![0x01, 0x00, 0x01]),
    ];
    if !imports.is_empty() {
        sections.push((2, vector(imports)));
    }
    sections.push((3, section_indices(func_count)));
    sections.push((7, export_body));
    if !code_body.is_empty() {
        sections.push((10, code_body));
    }
    if !data_body.is_empty() {
        sections.push((11, data_body));
    }
    wasm(&sections)
}

fn wasm(sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    for (id, section) in sections {
        bytes.push(*id);
        bytes.extend(leb(section.len() as u32));
        bytes.extend(section);
    }
    bytes
}

fn section_indices(count: u32) -> Vec<u8> {
    let mut bytes = leb(count);
    for _ in 0..count {
        bytes.push(0x00);
    }
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

fn vector(items: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = leb(items.len() as u32);
    for item in items {
        bytes.extend(item);
    }
    bytes
}

fn length_prefixed(bytes: &[u8]) -> Vec<u8> {
    let mut out = leb(bytes.len() as u32);
    out.extend(bytes);
    out
}

fn wasm_name(name: &str) -> Vec<u8> {
    length_prefixed(name.as_bytes())
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
