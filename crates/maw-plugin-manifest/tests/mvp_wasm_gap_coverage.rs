use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    invoke_plugin, InvokeContext, InvokeSource, LoadedPlugin, LoadedPluginKind,
    MvpWasmInvokeRuntime, PluginManifest,
};

#[test]
fn wasm_parser_rejects_overlong_export_name_length() {
    let root = temp_dir("overlong-export-name");
    let bytes = wasm(&[(7, vec![0x01, 0x80, 0x80, 0x80, 0x80, 0x80])]);
    let plugin = write_wasm_plugin(&root, "bad-export-name", &bytes);

    let result = invoke_plugin(&plugin, &cli(), &mut MvpWasmInvokeRuntime);

    assert!(
        result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("wasm compile error"),
        "{result:?}"
    );
    let _ = remove_dir_all(root);
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

fn wasm(sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    for (id, section) in sections {
        bytes.push(*id);
        bytes.push(u8::try_from(section.len()).expect("section length fits one byte"));
        bytes.extend(section);
    }
    bytes
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-mvp-gap-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp dir");
    dir
}
