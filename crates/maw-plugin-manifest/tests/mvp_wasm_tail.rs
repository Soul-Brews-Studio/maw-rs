use std::collections::BTreeMap;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    invoke_plugin, ApiMethod, CliFlagKind, InvokeContext, InvokeResult, InvokeSource, LoadedPlugin,
    LoadedPluginKind, MvpWasmInvokeRuntime, PluginApi, PluginCli, PluginHooks, PluginManifest,
};

#[test]
fn help_rendering_covers_empty_aliases_and_sparse_hooks() {
    let root = temp_dir("help-sparse");
    let entry = root.join("index.ts");
    write(&entry, "export default () => null;\n").expect("entry");
    let mut plugin = plugin(&root, "sparse");
    plugin.entry_path = Some(entry);
    plugin.manifest.cli = Some(PluginCli {
        command: "sparse".to_owned(),
        help: None,
        aliases: Some(Vec::new()),
        flags: Some(BTreeMap::from([(
            "--verbose".to_owned(),
            CliFlagKind::Boolean,
        )])),
    });
    plugin.manifest.api = Some(PluginApi {
        path: "/api/sparse".to_owned(),
        methods: vec![ApiMethod::Post],
    });
    plugin.manifest.hooks = Some(PluginHooks {
        gate: None,
        filter: Some(vec!["message".to_owned()]),
        on: None,
        late: None,
        wake: None,
        sleep: None,
        serve: None,
    });

    let output = invoke_plugin(&plugin, &cli(&["--help"]), &mut MvpWasmInvokeRuntime)
        .output
        .expect("help output");

    assert!(output.contains("usage: maw sparse"));
    assert!(!output.contains("aliases:"));
    assert!(output.contains("--verbose"));
    assert!(output.contains("boolean"));
    assert!(output.contains("api: POST /api/sparse"));
    assert!(output.contains("hooks: filter"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn wasm_parser_rejects_additional_truncated_sections() {
    let root = temp_dir("wasm-truncated");
    let cases = [
        ("zero-section-id", wasm(&[(0, vec![])])),
        ("empty-import-section", wasm(&[(2, vec![])])),
        ("empty-function-section", wasm(&[(3, vec![])])),
        ("empty-export-section", wasm(&[(7, vec![])])),
        ("import-missing-module-name", wasm(&[(2, vec![0x01])])),
        ("import-missing-field-name", wasm(&[(2, vec![0x01, 0x00])])),
        ("import-missing-kind", wasm(&[(2, vec![0x01, 0x00, 0x00])])),
        (
            "import-func-missing-type",
            wasm(&[(2, vector(vec![import_entry("env", "f", 0x00, vec![])]))]),
        ),
        (
            "import-table-missing-limits",
            wasm(&[(2, vector(vec![import_entry("env", "table", 0x01, vec![])]))]),
        ),
        (
            "import-global-missing-mutability",
            wasm(&[(2, vector(vec![import_entry("env", "g", 0x03, vec![0x7f])]))]),
        ),
        (
            "import-global-missing-content-type",
            wasm(&[(2, vector(vec![import_entry("env", "g", 0x03, vec![])]))]),
        ),
        (
            "import-table-missing-min",
            wasm(&[(
                2,
                vector(vec![import_entry("env", "table", 0x01, vec![0x00])]),
            )]),
        ),
        (
            "import-table-missing-max",
            wasm(&[(
                2,
                vector(vec![import_entry("env", "table", 0x01, vec![0x01, 0x01])]),
            )]),
        ),
        (
            "import-name-bytes-truncated",
            wasm(&[(2, vec![0x01, 0x03])]),
        ),
    ];

    for (name, bytes) in cases {
        let result = invoke_plugin(
            &write_wasm_plugin(&root, name, &bytes),
            &cli(&[]),
            &mut MvpWasmInvokeRuntime,
        );
        assert_error_contains(&result, "wasm compile error", name);
    }
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn wasm_parser_rejects_truncated_exports_and_code() {
    let root = temp_dir("wasm-truncated-more");
    let cases = [
        (
            "export-invalid-utf8",
            wasm_module(
                vec![],
                1,
                vector(vec![extend(vec![0x01, 0xff], &[0x02, 0x00])]),
                code(&[body(&[0x00, 0x41, 0x00, 0x0b])]),
                vec![],
            ),
        ),
        (
            "export-missing-kind",
            wasm(&[(7, vector(vec![wasm_name("memory")]))]),
        ),
        (
            "export-missing-index",
            wasm(&[(7, vector(vec![extend(wasm_name("memory"), &[0x02])]))]),
        ),
        (
            "export-name-bytes-truncated",
            wasm(&[(7, vec![0x01, 0x05])]),
        ),
        (
            "code-missing-count",
            wasm_module(
                vec![],
                1,
                exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
                vec![],
                vec![],
            ),
        ),
        (
            "code-missing-body-len",
            wasm_module(
                vec![],
                1,
                exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
                vec![0x01],
                vec![],
            ),
        ),
        (
            "code-missing-body-bytes",
            wasm_module(
                vec![],
                1,
                exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
                vec![0x01, 0x04, 0x00],
                vec![],
            ),
        ),
        (
            "const-missing-local-count",
            valid_wasm_with_body(&[], vec![]),
        ),
        (
            "const-local-missing-type",
            valid_wasm_with_body(&[0x01, 0x01], vec![]),
        ),
        (
            "const-local-missing-count",
            valid_wasm_with_body(&[0x01], vec![]),
        ),
        (
            "const-opcode-missing",
            valid_wasm_with_body(&[0x00], vec![]),
        ),
        (
            "const-missing-i32-op",
            valid_wasm_with_body(&[0x00, 0x42, 0x00, 0x0b], vec![]),
        ),
        (
            "const-end-missing",
            valid_wasm_with_body(&[0x00, 0x41, 0x00], vec![]),
        ),
        (
            "const-missing-end",
            valid_wasm_with_body(&[0x00, 0x41, 0x00, 0x00], vec![]),
        ),
        (
            "const-missing-signed-value",
            valid_wasm_with_body(&[0x00, 0x41], vec![]),
        ),
    ];

    for (name, bytes) in cases {
        let result = invoke_plugin(
            &write_wasm_plugin(&root, name, &bytes),
            &cli(&[]),
            &mut MvpWasmInvokeRuntime,
        );
        assert_error_contains(&result, "wasm compile error", name);
    }
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn wasm_parser_rejects_truncated_data_sections() {
    let root = temp_dir("wasm-truncated-data");
    let cases = [
        ("data-section-empty", wasm_with_raw_data_section(vec![])),
        (
            "data-missing-flag",
            valid_wasm_with_body(&[0x00, 0x41, 0x00, 0x0b], data_section(vec![vec![]])),
        ),
        (
            "data-non-i32-offset",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x42]]),
            ),
        ),
        (
            "data-negative-offset",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x41, 0x7f, 0x0b, 0x00]]),
            ),
        ),
        (
            "data-missing-end",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x41, 0x00, 0x00]]),
            ),
        ),
        (
            "data-end-byte-missing",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x41, 0x00]]),
            ),
        ),
        (
            "data-missing-offset",
            valid_wasm_with_body(&[0x00, 0x41, 0x00, 0x0b], data_section(vec![vec![0x00]])),
        ),
        (
            "data-missing-len",
            valid_wasm_with_body(
                &[0x00, 0x41, 0x00, 0x0b],
                data_section(vec![vec![0x00, 0x41, 0x00, 0x0b]]),
            ),
        ),
    ];

    for (name, bytes) in cases {
        let result = invoke_plugin(
            &write_wasm_plugin(&root, name, &bytes),
            &cli(&[]),
            &mut MvpWasmInvokeRuntime,
        );
        assert_error_contains(&result, "wasm compile error", name);
    }
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn wasm_parser_accepts_import_descriptor_edges_before_runtime_rejects_imports() {
    let root = temp_dir("wasm-import-descriptors");
    let cases = [
        (
            "import-table-with-max",
            import_entry("env", "table", 0x01, vec![0x01, 0x01, 0x02]),
        ),
        (
            "import-memory-with-max",
            import_entry("env", "memory", 0x02, vec![0x01, 0x01, 0x02]),
        ),
        (
            "import-global",
            import_entry("env", "global", 0x03, vec![0x7f, 0x00]),
        ),
    ];

    for (name, import) in cases {
        let bytes = wasm_module(
            vec![import],
            1,
            exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)]),
            code(&[body(&[0x00, 0x41, 0x00, 0x0b])]),
            vec![],
        );
        let result = invoke_plugin(
            &write_wasm_plugin(&root, name, &bytes),
            &cli(&[]),
            &mut MvpWasmInvokeRuntime,
        );
        assert_error_contains(&result, "unresolved imports", name);
    }
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn wasm_runtime_covers_large_data_offset_and_short_length_prefix() {
    let root = temp_dir("wasm-memory-edges");
    let high_data = valid_wasm_with_body(
        &[0x00, 0x41, 0x00, 0x0b],
        data_section(vec![vec![0x00, 0x41, 0x80, 0x80, 0x04, 0x0b, 0x01, b'x']]),
    );
    assert_eq!(
        invoke_plugin(
            &write_wasm_plugin(&root, "high-data", &high_data),
            &cli(&[]),
            &mut MvpWasmInvokeRuntime,
        ),
        InvokeResult::ok()
    );

    let short_prefix = valid_wasm_with_body(
        &[0x00, 0x41, 0xff, 0xff, 0x03, 0x0b],
        data_section(vec![vec![
            0x00, 0x41, 0xff, 0xff, 0x03, 0x0b, 0x02, b'a', b'b',
        ]]),
    );
    assert_eq!(
        invoke_plugin(
            &write_wasm_plugin(&root, "short-prefix", &short_prefix),
            &cli(&[]),
            &mut MvpWasmInvokeRuntime,
        ),
        InvokeResult::output("a")
    );

    let payload_past_end = valid_wasm_with_body(
        &const_i32_body(65_530),
        data_section(vec![extend(
            data_i32_offset_prefix(65_530),
            &[6, 10, 0, 0, 0, b'a', b'b'],
        )]),
    );
    assert_eq!(
        invoke_plugin(
            &write_wasm_plugin(&root, "payload-past-end", &payload_past_end),
            &cli(&[]),
            &mut MvpWasmInvokeRuntime,
        ),
        InvokeResult::output("\n")
    );
    remove_dir_all(root).expect("cleanup");
}

fn plugin(dir: &Path, name: &str) -> LoadedPlugin {
    LoadedPlugin {
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
            endpoints: None,
            secrets: None,
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
    }
}

fn write_wasm_plugin(dir: &Path, name: &str, bytes: &[u8]) -> LoadedPlugin {
    let plugin = plugin(dir, name);
    write(&plugin.wasm_path, bytes).expect("wasm");
    plugin
}

fn cli(args: &[&str]) -> InvokeContext {
    InvokeContext {
        source: InvokeSource::Cli,
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: None,
        home: None,
    }
}

fn assert_error_contains(result: &InvokeResult, expected: &str, name: &str) {
    let error = result.error.as_deref().unwrap_or_default();
    assert!(error.contains(expected), "{name}: {error:?}");
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

fn wasm_with_raw_data_section(data: Vec<u8>) -> Vec<u8> {
    wasm(&[
        (1, vec![0x01, 0x60, 0x00, 0x01, 0x7f]),
        (5, vec![0x01, 0x00, 0x01]),
        (3, section_indices(1)),
        (7, exports(&[("memory", 0x02, 0), ("handle", 0x00, 0)])),
        (10, code(&[body(&[0x00, 0x41, 0x00, 0x0b])])),
        (11, data),
    ])
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
        bytes.extend(leb(u32_len(section.len())));
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
    let mut bytes = leb(u32_len(items.len()));
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
    let mut out = leb(u32_len(bytes.len()));
    out.extend(bytes);
    out
}

fn wasm_name(name: &str) -> Vec<u8> {
    length_prefixed(name.as_bytes())
}

fn u32_len(len: usize) -> u32 {
    u32::try_from(len).expect("test wasm vector length fits in u32")
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
        "maw-rs-mvp-tail-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp dir");
    dir
}
