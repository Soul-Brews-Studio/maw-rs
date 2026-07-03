#[test]
fn rust_manifest_matches_scaffolded_plugin_json_contract() {
    let data = manifest("my-rust-plugin", PluginLanguage::Rust);

    assert_eq!(data["name"], "my-rust-plugin");
    assert_eq!(data["version"], "0.1.0");
    assert_eq!(data["sdk"], "^1.0.0");
    assert_eq!(
        data["wasm"],
        "./target/wasm32-unknown-unknown/release/my_rust_plugin.wasm"
    );
    assert_eq!(data["cli"]["command"], "my-rust-plugin");
    assert_eq!(data["api"]["path"], "/api/plugins/my-rust-plugin");
}

#[test]
fn assemblyscript_manifest_matches_scaffolded_plugin_json_contract() {
    let data = manifest("my-as-plugin", PluginLanguage::AssemblyScript);

    assert_eq!(data["name"], "my-as-plugin");
    assert_eq!(data["version"], "0.1.0");
    assert_eq!(data["sdk"], "^1.0.0");
    assert_eq!(data["wasm"], "./build/release.wasm");
    assert_eq!(data["cli"]["command"], "my-as-plugin");
    assert_eq!(data["api"]["path"], "/api/plugins/my-as-plugin");
}

#[test]
fn build_manifest_json_normalizes_underscores_to_hyphens_in_slug_fields() {
    let data = manifest("my_plugin", PluginLanguage::Rust);

    assert_eq!(data["name"], "my-plugin");
    assert!(data["wasm"]
        .as_str()
        .expect("wasm string")
        .contains("my_plugin.wasm"));
    assert_eq!(data["cli"]["command"], "my-plugin");
    assert_eq!(data["api"]["path"], "/api/plugins/my-plugin");
    assert_eq!(data["api"]["methods"], serde_json::json!(["GET", "POST"]));
}

#[test]
fn build_manifest_json_ends_with_newline() {
    assert!(build_manifest_json("my-plugin", PluginLanguage::Rust).ends_with('\n'));
}

fn manifest(name: &str, lang: PluginLanguage) -> Value {
    serde_json::from_str(&build_manifest_json(name, lang)).expect("valid manifest json")
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "maw-rs-plugin-{label}-{}-{stamp}",
        std::process::id()
    ))
}

fn make_rust_template(dir: &Path, sdk_rel_path: &str) {
    fs::create_dir_all(dir.join("src")).expect("create rust template src");
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"hello-rust\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nmaw-plugin-sdk = {{ path = \"{sdk_rel_path}\" }}\n"
        ),
    )
    .expect("write rust template cargo");
    fs::write(
        dir.join("src").join("lib.rs"),
        "use maw_plugin_sdk as maw;\n\n#[no_mangle]\npub extern \"C\" fn handle(ptr: *const u8, len: usize) -> i32 { 0 }\n",
    )
    .expect("write rust template lib");
}

fn make_as_template(dir: &Path) {
    fs::create_dir_all(dir.join("assembly")).expect("create as template assembly");
    fs::write(
        dir.join("package.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": "hello-as",
            "version": "0.1.0",
            "scripts": { "build": "asc assembly/index.ts -o build/hello-as.wasm" }
        }))
        .expect("serialize package json")
            + "\n",
    )
    .expect("write as template package");
    fs::write(
        dir.join("assembly").join("index.ts"),
        "// AssemblyScript stub\nexport function handle(ptr: i32, len: i32): i32 { return 0; }\nexport const memory = new Memory();\n",
    )
    .expect("write as template index");
}
