#[test]
#[allow(clippy::too_many_lines)]
fn plugin_manifest_remaining_text_parser_and_runtime_edges_are_covered() {
    assert_usage(
        &["plugin-manifest"],
        "plugin-manifest: expected parse or load",
    );
    assert_usage(
        &["plugin-manifest", "bogus"],
        "plugin-manifest: unknown subcommand bogus",
    );
    assert_usage(
        &["plugin-manifest", "parse", "--bogus"],
        "plugin-manifest parse: unknown argument --bogus",
    );
    assert_usage(
        &["plugin-manifest", "load", "--bogus"],
        "plugin-manifest load: unknown argument --bogus",
    );
    assert_usage(
        &["plugin-manifest", "invoke", "--scan-dir"],
        "plugin-manifest: missing --scan-dir value",
    );
    assert_usage(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            "/tmp",
            "--disabled",
        ],
        "plugin-manifest: missing --disabled value",
    );
    assert_usage(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            "/tmp",
            "--runtime-version",
        ],
        "plugin-manifest: missing --runtime-version value",
    );
    assert_usage(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            "/tmp",
            "--fake-ts-output",
        ],
        "plugin-manifest invoke: unknown argument --fake-ts-output",
    );
    assert_usage(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            "/tmp",
            "--fake-wasm-output",
        ],
        "plugin-manifest invoke: unknown argument --fake-wasm-output",
    );
    assert_usage(
        &["plugin-manifest", "invoke", "--bogus"],
        "plugin-manifest invoke: unknown argument --bogus",
    );
    assert_usage(
        &["plugin-manifest", "invoke"],
        "plugin-manifest invoke: --scan-dir is required",
    );
    assert_usage(
        &[
            "plugin-manifest",
            "import-symbol",
            "--scan-dir",
            "/tmp",
            "--module-symbol",
            "bad",
        ],
        "plugin-manifest import-symbol: --module-symbol must be name=value",
    );
    assert_usage(
        &["plugin-manifest", "discover", "--plugin", "nope"],
        "plugin-manifest discover: unknown argument --plugin",
    );

    let root = temp_dir("plugin-runtime");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");
    write_entry_plugin(&plugins_dir, "ts-plug", serde_json::Map::new());
    write_wasm_plugin(&plugins_dir, "wasm-plug");
    write_entry_plugin(&plugins_dir, "disabled-plug", serde_json::Map::new());

    let discover = run_cli(&[
        "plugin-manifest".to_owned(),
        "discover".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
    ]);
    assert_eq!(discover.code, 0, "{}", discover.stderr);
    assert!(discover.stdout.contains("ts-plug"), "{}", discover.stdout);

    let load_missing = run_cli(&[
        "plugin-manifest".to_owned(),
        "load".to_owned(),
        "--dir".to_owned(),
        root.join("missing").to_string_lossy().into_owned(),
    ]);
    assert_eq!(load_missing.code, 0, "{}", load_missing.stderr);
    assert_eq!(load_missing.stdout, "missing\n");

    let invoke_refused_ts = run_cli(&[
        "plugin-manifest".to_owned(),
        "invoke".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--plugin".to_owned(),
        "ts-plug".to_owned(),
    ]);
    assert_eq!(invoke_refused_ts.code, 2);
    assert!(invoke_refused_ts.stdout.is_empty(), "{}", invoke_refused_ts.stdout);
    assert!(
        invoke_refused_ts
            .stderr
            .contains("TS source plugin 'ts-plug' is not executable"),
        "{}",
        invoke_refused_ts.stderr
    );

    let invoke_missing = run_cli(&[
        "plugin-manifest".to_owned(),
        "invoke".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--plugin".to_owned(),
        "not-here".to_owned(),
    ]);
    assert_eq!(invoke_missing.code, 2);
    assert!(invoke_missing
        .stderr
        .contains("plugin 'not-here' not found"));

    let invoke_disabled = run_cli(&[
        "plugin-manifest".to_owned(),
        "invoke".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--disabled".to_owned(),
        "disabled-plug".to_owned(),
        "--plugin".to_owned(),
        "disabled-plug".to_owned(),
    ]);
    assert_eq!(invoke_disabled.code, 2);
    assert!(invoke_disabled
        .stderr
        .contains("plugin 'disabled-plug' is disabled"));

    let manifest_with_flags = json!({
        "name": "flaggy",
        "version": "1.0.0",
        "sdk": "*",
        "entry": "index.ts",
        "target": "js",
        "cli": { "command": "flaggy", "flags": { "--name": "string" } }
    });
    write(
        root.join("index.ts"),
        b"export default async function flaggy() {}\n",
    )
    .expect("entry");
    let parsed = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "parse".to_owned(),
        "--dir".to_owned(),
        root.to_string_lossy().into_owned(),
        "--json".to_owned(),
        manifest_with_flags.to_string(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(parsed["manifest"]["cli"]["flags"]["--name"], "string");

    remove_dir_all(root).expect("cleanup plugin runtime");
}

fn write_entry_plugin(
    root: &Path,
    name: &str,
    manifest: serde_json::Map<String, serde_json::Value>,
) {
    let dir = root.join(name);
    create_dir_all(&dir).expect("plugin dir");
    write(
        dir.join("index.ts"),
        format!(
            "export default async function {}() {{}}\n",
            name.replace('-', "_")
        ),
    )
    .expect("entry");
    let mut full_manifest = serde_json::Map::from_iter([
        ("name".to_owned(), json!(name)),
        ("version".to_owned(), json!("1.0.0")),
        ("sdk".to_owned(), json!("*")),
        ("target".to_owned(), json!("js")),
        ("entry".to_owned(), json!("index.ts")),
    ]);
    full_manifest.extend(manifest);
    write(
        dir.join("plugin.json"),
        serde_json::to_vec_pretty(&serde_json::Value::Object(full_manifest)).expect("json"),
    )
    .expect("manifest");
}

fn write_wasm_plugin(root: &Path, name: &str) {
    let dir = root.join(name);
    create_dir_all(&dir).expect("plugin dir");
    write(dir.join("plugin.wasm"), b"wasm bytes").expect("wasm");
    let full_manifest = serde_json::Map::from_iter([
        ("name".to_owned(), json!(name)),
        ("version".to_owned(), json!("1.0.0")),
        ("sdk".to_owned(), json!("*")),
        ("wasm".to_owned(), json!("plugin.wasm")),
    ]);
    write(
        dir.join("plugin.json"),
        serde_json::to_vec_pretty(&serde_json::Value::Object(full_manifest)).expect("json"),
    )
    .expect("manifest");
}
