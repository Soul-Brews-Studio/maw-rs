// Ported from maw-js/test/isolated/registry-invoke.test.ts WASM invoke fixtures.
// These tests lock the same malformed/missing-export/output/import-error cases before general WASM runtime work.

use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    invoke_plugin, InvokeContext, InvokeResult, InvokeSource, LoadedPlugin, LoadedPluginKind,
    MvpWasmInvokeRuntime, PluginInvokeRuntime, PluginManifest,
};

#[test]
fn invoke_source_and_mvp_ts_runtime_report_metadata_contracts() {
    assert_eq!(InvokeSource::Cli.as_str(), "cli");
    assert_eq!(InvokeSource::Api.as_str(), "api");
    assert_eq!(InvokeSource::Peer.as_str(), "peer");

    let root = make_temp_dir("ts-runtime-default");
    let plugin = LoadedPlugin {
        kind: LoadedPluginKind::Ts,
        entry_path: Some(root.join("index.ts")),
        wasm_export: "handle".to_owned(),
        ..write_wasm_plugin(&root, "ts-runtime-default", WASM_HANDLE_ZERO)
    };
    let result = MvpWasmInvokeRuntime.invoke_ts(
        &plugin,
        &InvokeContext {
            source: InvokeSource::Api,
            args: vec!["one".to_owned()],
            cwd: None,
            home: None,
        },
    );

    assert_eq!(
        result,
        InvokeResult::error("TS plugin runtime is not available")
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn mvp_wasm_runtime_treats_out_of_bounds_or_empty_outputs_as_ok() {
    let root = make_temp_dir("empty-output");
    let empty = write_wasm_plugin(&root, "empty", WASM_EMPTY_OUTPUT_PTR);
    let out_of_bounds = write_wasm_plugin(&root, "oob", WASM_OUT_OF_BOUNDS_PTR);

    assert_eq!(
        invoke_plugin(&empty, &cli(), &mut MvpWasmInvokeRuntime),
        InvokeResult::ok()
    );
    assert_eq!(
        invoke_plugin(&out_of_bounds, &cli(), &mut MvpWasmInvokeRuntime),
        InvokeResult::ok()
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn mvp_wasm_runtime_rejects_truncated_sections_and_overlong_leb() {
    let root = make_temp_dir("parse-errors");
    let truncated = write_wasm_plugin(&root, "truncated", WASM_TRUNCATED_SECTION);
    let overlong = write_wasm_plugin(&root, "overlong", WASM_OVERLONG_SECTION_LEB);

    assert_error_contains(
        &invoke_plugin(&truncated, &cli(), &mut MvpWasmInvokeRuntime),
        "wasm compile error",
    );
    assert_error_contains(
        &invoke_plugin(&overlong, &cli(), &mut MvpWasmInvokeRuntime),
        "wasm compile error",
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn mvp_wasm_runtime_rejects_malformed_wasm_like_maw_js() {
    let root = make_temp_dir("bad-compile");
    let plugin = write_wasm_plugin(&root, "bad-compile", &WASM_BAD_COMPILE);

    let result = invoke_plugin(&plugin, &cli(), &mut MvpWasmInvokeRuntime);

    assert!(!result.ok);
    assert_error_contains(&result, "wasm compile error");
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn mvp_wasm_runtime_requires_handle_and_memory_exports() {
    let root = make_temp_dir("missing-exports");
    let no_memory = write_wasm_plugin(&root, "no-memory", WASM_NO_MEMORY);
    let no_handle = write_wasm_plugin(&root, "no-handle", WASM_NO_HANDLE);

    let missing_memory = invoke_plugin(&no_memory, &cli(), &mut MvpWasmInvokeRuntime);
    let missing_handle = invoke_plugin(&no_handle, &cli(), &mut MvpWasmInvokeRuntime);

    assert_eq!(
        missing_memory,
        InvokeResult::error("wasm missing required handle+memory exports")
    );
    assert_eq!(
        missing_handle,
        InvokeResult::error("wasm missing required handle+memory exports")
    );
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn mvp_wasm_runtime_returns_ok_when_handle_returns_zero() {
    let root = make_temp_dir("zero");
    let plugin = write_wasm_plugin(&root, "zero", WASM_HANDLE_ZERO);

    let result = invoke_plugin(&plugin, &cli(), &mut MvpWasmInvokeRuntime);

    assert_eq!(result, InvokeResult::ok());
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn mvp_wasm_runtime_decodes_length_prefixed_output() {
    let root = make_temp_dir("length-prefix");
    let plugin = write_wasm_plugin(&root, "lenpre", WASM_LEN_PREFIXED);

    let result = invoke_plugin(&plugin, &cli(), &mut MvpWasmInvokeRuntime);

    assert_eq!(result, InvokeResult::output("HELLO"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn mvp_wasm_runtime_decodes_null_terminated_fallback() {
    let root = make_temp_dir("null-term");
    let plugin = write_wasm_plugin(&root, "nullterm", WASM_NULL_TERM);

    let result = invoke_plugin(&plugin, &cli(), &mut MvpWasmInvokeRuntime);

    assert_eq!(result, InvokeResult::output("HELLO"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn mvp_wasm_runtime_reports_unresolved_imports_like_maw_js() {
    let root = make_temp_dir("bad-instantiate");
    let plugin = write_wasm_plugin(&root, "bad-inst", WASM_BAD_INSTANTIATE);

    let result = invoke_plugin(&plugin, &cli(), &mut MvpWasmInvokeRuntime);

    assert_eq!(
        result,
        InvokeResult::error("wasm instantiation failed: unresolved imports")
    );
    remove_dir_all(root).expect("cleanup");
}

fn make_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-mvp-wasm-invoke-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_wasm_plugin(dir: &Path, name: &str, bytes: &[u8]) -> LoadedPlugin {
    let wasm_path = dir.join(format!("{name}.wasm"));
    write(&wasm_path, bytes).expect("write wasm");
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
            capability_warnings: Vec::new(),
            dependencies: None,
            artifact: None,
        },
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

fn assert_error_contains(result: &InvokeResult, expected: &str) {
    let error = result.error.as_deref().unwrap_or_default();
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

const WASM_HANDLE_ZERO: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65,
    0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x68, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x00, 0x00, 0x0a,
    0x06, 0x01, 0x04, 0x00, 0x41, 0x00, 0x0b,
];

const WASM_LEN_PREFIXED: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65,
    0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x68, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x00, 0x00, 0x0a,
    0x07, 0x01, 0x05, 0x00, 0x41, 0xe4, 0x00, 0x0b, 0x0b, 0x10, 0x01, 0x00, 0x41, 0xe4, 0x00, 0x0b,
    0x09, 0x05, 0x00, 0x00, 0x00, 0x48, 0x45, 0x4c, 0x4c, 0x4f,
];

const WASM_NULL_TERM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65,
    0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x68, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x00, 0x00, 0x0a,
    0x07, 0x01, 0x05, 0x00, 0x41, 0xe4, 0x00, 0x0b, 0x0b, 0x0d, 0x01, 0x00, 0x41, 0xe4, 0x00, 0x0b,
    0x06, 0x48, 0x45, 0x4c, 0x4c, 0x4f, 0x00,
];

const WASM_NO_MEMORY: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0a, 0x01, 0x06, 0x68, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x00,
    0x00, 0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x00, 0x0b,
];

const WASM_NO_HANDLE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x0a, 0x01,
    0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00,
];

const WASM_BAD_INSTANTIATE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0a, 0x02, 0x60, 0x00, 0x00, 0x60, 0x02,
    0x7f, 0x7f, 0x01, 0x7f, 0x02, 0x12, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x0a, 0x6d, 0x69, 0x73, 0x73,
    0x69, 0x6e, 0x67, 0x5f, 0x66, 0x6e, 0x00, 0x00, 0x03, 0x02, 0x01, 0x01, 0x05, 0x03, 0x01, 0x00,
    0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x68, 0x61,
    0x6e, 0x64, 0x6c, 0x65, 0x00, 0x01, 0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x00, 0x0b,
];

const WASM_BAD_COMPILE: [u8; 12] = [
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
];

const WASM_EMPTY_OUTPUT_PTR: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65,
    0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x68, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x00, 0x00, 0x0a,
    0x08, 0x01, 0x06, 0x00, 0x41, 0xe8, 0xfb, 0x03, 0x0b,
];

const WASM_OUT_OF_BOUNDS_PTR: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65,
    0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x68, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x00, 0x00, 0x0a,
    0x08, 0x01, 0x06, 0x00, 0x41, 0x80, 0x80, 0x04, 0x0b,
];

const WASM_TRUNCATED_SECTION: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x10, 0x01,
];

const WASM_OVERLONG_SECTION_LEB: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00,
];
