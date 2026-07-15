// The wasm-host runtime-edge test (real-Extism triggers invoke + unbuilt-TS
// refusal) was removed in the repo split — the wasm-parity/triggers fixture it
// copied cross-crate now lives in Soul-Brews-Studio/maw-fixtures @aecf20b6;
// rework/relocate tracked in #546. Its plan-json golden
// (`fixtures/native-plugin-manifest/invoke-triggers-plan-json.stdout`) stays
// committed, per policy: never delete goldens.

fn write_invoke_ts_plugin(
    root: &Path,
    name: &str,
    manifest: serde_json::Map<String, serde_json::Value>,
) {
    write_entry_plugin(root, name, manifest);
}
