#[test]
fn plugin_manifest_invoke_uses_real_extism_wasm_and_refuses_unbuilt_ts() {
    let root = make_temp_dir("invoke-runtime");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");
    write_invoke_ts_plugin(&plugins_dir, "ts-plug", serde_json::Map::new());
    copy_fixture_plugin("triggers", &plugins_dir);

    let ts = run_cli(&[
        "plugin-manifest".to_owned(),
        "invoke".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--disabled".to_owned(),
        "other-plug".to_owned(),
        "--runtime-version".to_owned(),
        "1.2.3".to_owned(),
        "--use-cache".to_owned(),
        "--plugin".to_owned(),
        "ts-plug".to_owned(),
        "--source".to_owned(),
        "cli".to_owned(),
        "--arg".to_owned(),
        "a".to_owned(),
        "--arg".to_owned(),
        "b".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(ts.code, 2);
    assert!(ts.stdout.is_empty(), "{}", ts.stdout);
    assert!(
        ts.stderr.contains("TS source plugin 'ts-plug' is not executable"),
        "{}",
        ts.stderr
    );
    assert!(ts.stderr.contains("Build this plugin to WASM"), "{}", ts.stderr);
    assert!(
        ts.stderr.contains("No Bun/JS subprocess fallback is available"),
        "{}",
        ts.stderr
    );

    let wasm = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "invoke".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--plugin".to_owned(),
        "triggers-parity".to_owned(),
        "--plan-json".to_owned(),
    ]));
    let golden = serde_json::from_str::<serde_json::Value>(include_str!(
        "../fixtures/native-plugin-manifest/invoke-triggers-plan-json.stdout"
    ))
    .expect("golden json");
    assert_eq!(wasm, golden);
    assert_eq!(wasm["runtime"]["mode"], "extism-wasm");
    assert_eq!(wasm["runtime"]["noBunFallback"], true);

    remove_dir_all(root).expect("cleanup invoke runtime");
}

fn write_invoke_ts_plugin(
    root: &Path,
    name: &str,
    manifest: serde_json::Map<String, serde_json::Value>,
) {
    write_entry_plugin(root, name, manifest);
}

fn copy_fixture_plugin(name: &str, plugins_dir: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates dir")
        .join("maw-plugin-manifest")
        .join("tests")
        .join("fixtures")
        .join("wasm-parity")
        .join(name);
    let target = plugins_dir.join(name);
    copy_dir(&fixture, &target);
}

fn copy_dir(source: &Path, target: &Path) {
    create_dir_all(target).expect("copy target");
    for entry in std::fs::read_dir(source).expect("read fixture dir") {
        let entry = entry.expect("fixture entry");
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir(&source_path, &target_path);
        } else {
            std::fs::copy(&source_path, &target_path).expect("copy fixture file");
        }
    }
}
