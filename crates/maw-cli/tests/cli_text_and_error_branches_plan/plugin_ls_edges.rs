fn write_ts_plugin(root: &Path, name: &str, manifest: serde_json::Map<String, serde_json::Value>) {
    let dir = root.join(name);
    create_dir_all(&dir).expect("plugin dir");
    write(
        dir.join("index.ts"),
        b"export default async function plugin() {}\n",
    )
    .expect("entry");
    write(dir.join("lib.ts"), b"export const answer = 42;\n").expect("module");
    let mut full_manifest = serde_json::Map::from_iter([
        ("name".to_owned(), json!(name)),
        ("version".to_owned(), json!("1.0.0")),
        ("sdk".to_owned(), json!("*")),
        ("target".to_owned(), json!("js")),
        ("entry".to_owned(), json!("index.ts")),
        (
            "module".to_owned(),
            json!({ "path": "./lib.ts", "exports": ["answer"] }),
        ),
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
    write(
        dir.join("plugin.json"),
        json!({
            "name": name,
            "version": "1.0.0",
            "sdk": "*",
            "wasm": "plugin.wasm"
        })
        .to_string(),
    )
    .expect("manifest");
}

#[test]
fn plugin_ls_compact_table_filters_and_dedupe_match_maw_js_shape() {
    let root = temp_dir("plugin-ls-parity");
    write_ts_plugin(
        &root,
        "alpha",
        serde_json::Map::from_iter([
            ("weight".to_owned(), json!(0)),
            ("cli".to_owned(), json!({ "command": "alpha" })),
            (
                "api".to_owned(),
                json!({ "path": "/api/alpha", "methods": ["GET"] }),
            ),
        ]),
    );
    write_ts_plugin(
        &root,
        "bravo",
        serde_json::Map::from_iter([
            ("weight".to_owned(), json!(30)),
            ("cli".to_owned(), json!({ "command": "bravo" })),
        ]),
    );
    write_ts_plugin(
        &root,
        "charlie",
        serde_json::Map::from_iter([
            ("tier".to_owned(), json!("extra")),
            ("cli".to_owned(), json!({ "command": "charlie" })),
        ]),
    );
    let duplicate_root = root.join("dupes");
    create_dir_all(&duplicate_root).expect("duplicate root");
    write_ts_plugin(
        &duplicate_root,
        "alpha",
        serde_json::Map::from_iter([
            ("weight".to_owned(), json!(50)),
            ("cli".to_owned(), json!({ "command": "ignored-alpha" })),
        ]),
    );

    let output = run_cli(&[
        "plugin".to_owned(),
        "ls".to_owned(),
        "--scan-dir".to_owned(),
        root.to_string_lossy().into_owned(),
        "--scan-dir".to_owned(),
        duplicate_root.to_string_lossy().into_owned(),
    ]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        "3 plugins (3 active, 0 disabled)\n  core: 1 · standard: 1 · extra: 1\n  cli: 3 · api: 1 · health: ok\n"
    );

    let api = run_cli(&[
        "plugin".to_owned(),
        "ls".to_owned(),
        "--api".to_owned(),
        "--scan-dir".to_owned(),
        root.to_string_lossy().into_owned(),
    ]);
    assert_eq!(api.code, 0, "{}", api.stderr);
    assert_eq!(
        api.stdout,
        "1 plugin (1 active, 0 disabled) matching api\n  core: 1 · standard: 0 · extra: 0\n  cli: 1 · api: 1 · health: ok\n"
    );

    let verbose = run_cli(&[
        "plugin".to_owned(),
        "ls".to_owned(),
        "-v".to_owned(),
        "--core".to_owned(),
        "--scan-dir".to_owned(),
        root.to_string_lossy().into_owned(),
    ]);
    assert_eq!(verbose.code, 0, "{}", verbose.stderr);
    assert!(
        verbose.stdout.starts_with(
            "\n\x1b[1mcore\x1b[0m (1)\nname   version  tier             surfaces                   dir"
        ),
        "{}",
        verbose.stdout
    );
    assert!(
        verbose.stdout.contains(&format!(
            "alpha  1.0.0    \x1b[32m●\x1b[0m core  cli:alpha, api:/api/alpha  {}/alpha",
            root.to_string_lossy()
        )),
        "{}",
        verbose.stdout
    );
    assert!(verbose.stdout.ends_with("\n1 active\n"), "{}", verbose.stdout);

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn plugin_ls_help_matches_current_maw_js_summary() {
    let output = run_cli(&[
        "plugin".to_owned(),
        "ls".to_owned(),
        "--help".to_owned(),
    ]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        "usage: maw plugin <init|build|install|create|ls|info|remove|enable <name...>|disable> [args]\n  ls: compact by default; use -v for full table; filters: --core --standard --extra --api\n"
    );
}
