use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_cli::run_cli;
use serde_json::json;

fn json_output(output: &maw_cli::CliOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
        panic!("invalid json: {error}\n{}", output.stdout);
    })
}

fn make_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-cli-plugin-manifest-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn plugin_manifest_parse_plan_cli_matches_maw_js_manifest_fixture() {
    let dir = make_temp_dir("parse-happy");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");

    let manifest = json!({
        "name": "full-plugin",
        "version": "2.3.4",
        "wasm": "plugin.wasm",
        "sdk": "~1.2.0",
        "weight": 25,
        "cli": { "command": "greet", "help": "Say hello" },
        "api": { "path": "/greet", "methods": ["GET", "POST"] },
        "description": "A greeting plugin",
        "author": "Nat"
    });
    let output = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "parse".to_owned(),
        "--dir".to_owned(),
        dir.to_string_lossy().into_owned(),
        "--json".to_owned(),
        manifest.to_string(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(output["command"], "plugin-manifest");
    assert_eq!(output["kind"], "parse");
    assert_eq!(output["manifest"]["name"], "full-plugin");
    assert_eq!(output["manifest"]["version"], "2.3.4");
    assert_eq!(output["manifest"]["wasm"], "plugin.wasm");
    assert_eq!(output["manifest"]["sdk"], "~1.2.0");
    assert_eq!(output["manifest"]["weight"], 25);
    assert_eq!(output["manifest"]["cli"]["command"], "greet");
    assert_eq!(output["manifest"]["cli"]["help"], "Say hello");
    assert_eq!(output["manifest"]["api"]["path"], "/greet");
    assert_eq!(output["manifest"]["api"]["methods"], json!(["GET", "POST"]));
    assert_eq!(output["manifest"]["description"], "A greeting plugin");
    assert_eq!(output["manifest"]["author"], "Nat");

    remove_dir_all(dir).expect("cleanup");
}

#[test]
fn plugin_manifest_parse_accepts_target_wasm_and_matches_golden() {
    let dir = make_temp_dir("parse-target-wasm");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");

    let manifest = json!({
        "name": "target-wasm",
        "version": "1.0.0",
        "target": "wasm",
        "wasm": "plugin.wasm",
        "sdk": "*",
        "cli": { "command": "target-wasm" }
    });
    let output = run_cli(&[
        "plugin-manifest".to_owned(),
        "parse".to_owned(),
        "--dir".to_owned(),
        dir.to_string_lossy().into_owned(),
        "--json".to_owned(),
        manifest.to_string(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let mut stable = serde_json::from_str::<serde_json::Value>(&output.stdout)
        .expect("plugin-manifest parse json");
    stable
        .as_object_mut()
        .expect("parse object")
        .remove("dir");
    let golden = serde_json::from_str::<serde_json::Value>(include_str!(
        "../fixtures/native-plugin-manifest/target-wasm-parse.stdout"
    ))
    .expect("golden json");
    assert_eq!(stable, golden);

    remove_dir_all(dir).expect("cleanup");
}

#[test]
fn plugin_manifest_load_plan_cli_matches_maw_js_loader_contract() {
    let missing = make_temp_dir("missing");
    let output = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "load".to_owned(),
        "--dir".to_owned(),
        missing.to_string_lossy().into_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(output["command"], "plugin-manifest");
    assert_eq!(output["kind"], "load");
    assert_eq!(output["present"], false);
    assert!(output["plugin"].is_null());
    remove_dir_all(missing).expect("cleanup missing");

    let dir = make_temp_dir("load-wasm");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");
    write_manifest(
        &dir,
        &json!({ "name": "test-pkg", "version": "1.0.0", "wasm": "plugin.wasm", "sdk": "*" }),
    );

    let output = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "load".to_owned(),
        "--dir".to_owned(),
        dir.to_string_lossy().into_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(output["present"], true);
    assert_eq!(output["plugin"]["kind"], "wasm");
    assert_eq!(output["plugin"]["disabled"], false);
    assert_eq!(output["plugin"]["manifest"]["name"], "test-pkg");
    assert_eq!(output["plugin"]["manifest"]["wasm"], "plugin.wasm");
    assert!(output["plugin"]["manifest"]["target"].is_null());
    assert_eq!(
        output["plugin"]["wasmPath"],
        dir.join("plugin.wasm").to_string_lossy().as_ref()
    );
    assert!(output["plugin"]["entryPath"].is_null());

    remove_dir_all(dir).expect("cleanup load");

    let target_dir = make_temp_dir("load-target-wasm");
    write(target_dir.join("plugin.wasm"), b"wasm").expect("wasm");
    write_manifest(
        &target_dir,
        &json!({
            "name": "target-wasm-load",
            "version": "1.0.0",
            "target": "wasm",
            "wasm": "plugin.wasm",
            "sdk": "*"
        }),
    );

    let target_output = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "load".to_owned(),
        "--dir".to_owned(),
        target_dir.to_string_lossy().into_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(target_output["plugin"]["kind"], "wasm");
    assert_eq!(target_output["plugin"]["manifest"]["target"], "wasm");
    assert_eq!(
        target_output["plugin"]["wasmPath"],
        target_dir.join("plugin.wasm").to_string_lossy().as_ref()
    );
    assert!(target_output["plugin"]["entryPath"].is_null());

    remove_dir_all(target_dir).expect("cleanup target load");
}

#[test]
fn plugin_manifest_load_plan_reports_entry_and_artifact_precedence() {
    let dir = make_temp_dir("entry-precedence");
    write(
        dir.join("index.ts"),
        b"export default () => ({ ok: true });\n",
    )
    .expect("entry");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");
    write_manifest(
        &dir,
        &json!({
            "name": "source-first",
            "version": "1.0.0",
            "entry": "index.ts",
            "wasm": "plugin.wasm",
            "sdk": "*",
            "target": "js",
            "artifact": { "path": "dist/index.js", "sha256": null }
        }),
    );

    let output = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "load".to_owned(),
        "--dir".to_owned(),
        dir.to_string_lossy().into_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(output["plugin"]["kind"], "ts");
    assert_eq!(
        output["plugin"]["entryPath"],
        dir.join("index.ts").to_string_lossy().as_ref()
    );
    assert_eq!(output["plugin"]["manifest"]["target"], "js");
    assert_eq!(
        output["plugin"]["manifest"]["artifact"]["path"],
        "dist/index.js"
    );
    assert!(output["plugin"]["manifest"]["artifact"]["sha256"].is_null());

    remove_dir_all(dir).expect("cleanup entry");
}

#[test]
fn plugin_manifest_plan_rejects_manifest_validation_errors() {
    let dir = make_temp_dir("failures");
    write(dir.join("plugin.wasm"), b"wasm").expect("wasm");

    let output = run_cli(&[
        "plugin-manifest".to_owned(),
        "parse".to_owned(),
        "--dir".to_owned(),
        dir.to_string_lossy().into_owned(),
        "--json".to_owned(),
        json!({
            "name": "bad-cli",
            "version": "1.0.0",
            "wasm": "plugin.wasm",
            "sdk": "*",
            "cli": { "command": "" }
        })
        .to_string(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(output.code, 2);
    assert!(
        output
            .stderr
            .contains("plugin.json: cli.command must be a non-empty string"),
        "{}",
        output.stderr
    );

    remove_dir_all(dir).expect("cleanup failures");
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    write(dir.join("plugin.json"), manifest.to_string()).expect("write manifest");
}

#[test]
fn plugin_manifest_discover_plan_cli_matches_missing_roots_contract() {
    let root = make_temp_dir("discover-missing");
    let output = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "discover".to_owned(),
        "--scan-dir".to_owned(),
        root.join("missing-root").to_string_lossy().into_owned(),
        "--runtime-version".to_owned(),
        "1.0.0".to_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(output["command"], "plugin-manifest");
    assert_eq!(output["kind"], "discover");
    assert_eq!(output["plugins"], json!([]));
    assert_eq!(output["warnings"], json!([]));

    remove_dir_all(root).expect("cleanup discover missing");
}

#[test]
fn plugin_manifest_discover_plan_cli_matches_registry_gates_and_sorting() {
    let root = make_temp_dir("discover-gates");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");

    write_entry_plugin(
        &plugins_dir,
        "registry-bad-sdk",
        serde_json::Map::from_iter([("sdk".to_owned(), json!(">=999.0.0"))]),
    );
    write_entry_plugin(
        &plugins_dir,
        "registry-legacy-ok",
        serde_json::Map::from_iter([("weight".to_owned(), json!(50))]),
    );
    write_entry_plugin(
        &plugins_dir,
        "registry-disabled-ok",
        serde_json::Map::from_iter([("weight".to_owned(), json!(70))]),
    );
    write(
        plugins_dir.join(".overrides.json"),
        br#"{"registry-disabled-ok":1}"#,
    )
    .expect("overrides");

    let output = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "discover".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--disabled".to_owned(),
        "registry-disabled-ok".to_owned(),
        "--runtime-version".to_owned(),
        "1.0.0".to_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(
        output["plugins"][0]["manifest"]["name"],
        "registry-disabled-ok"
    );
    assert_eq!(output["plugins"][0]["manifest"]["weight"], 1);
    assert_eq!(output["plugins"][0]["disabled"], true);
    assert_eq!(
        output["plugins"][1]["manifest"]["name"],
        "registry-legacy-ok"
    );
    let warning_text = output["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .map(|warning| warning.as_str().expect("warning text"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(warning_text.contains("requires maw SDK"), "{warning_text}");
    assert!(
        warning_text.contains("legacy plugin") || warning_text.contains("legacy plugins"),
        "{warning_text}"
    );

    remove_dir_all(root).expect("cleanup discover gates");
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

#[test]
fn plugin_manifest_import_symbol_plan_cli_returns_whitelisted_exports() {
    let root = make_temp_dir("import-symbol-happy");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");
    write_module_plugin(
        &plugins_dir,
        "helper",
        "./lib.ts",
        &["answer", "greet"],
        false,
    );

    let output = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "import-symbol".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--plugin".to_owned(),
        "helper".to_owned(),
        "--symbol".to_owned(),
        "answer".to_owned(),
        "--module-symbol".to_owned(),
        "answer=42".to_owned(),
        "--module-symbol".to_owned(),
        "greet=hi Nat".to_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(output["command"], "plugin-manifest");
    assert_eq!(output["kind"], "import-symbol");
    assert_eq!(output["plugin"], "helper");
    assert_eq!(output["symbol"], "answer");
    assert_eq!(output["value"], "42");
    assert!(
        output["modulePath"]
            .as_str()
            .expect("module path")
            .ends_with("plugins/helper/lib.ts"),
        "{}",
        output["modulePath"]
    );

    remove_dir_all(root).expect("cleanup import symbol");
}

#[test]
fn plugin_manifest_import_symbol_plan_cli_rejects_private_missing_and_disabled() {
    let root = make_temp_dir("import-symbol-errors");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");
    write_module_plugin(&plugins_dir, "helper", "./lib.ts", &["publicThing"], false);
    write_module_plugin(
        &plugins_dir,
        "disabled-helper",
        "./lib.ts",
        &["answer"],
        false,
    );

    let private = run_cli(&[
        "plugin-manifest".to_owned(),
        "import-symbol".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--plugin".to_owned(),
        "helper".to_owned(),
        "--symbol".to_owned(),
        "privateThing".to_owned(),
        "--module-symbol".to_owned(),
        "privateThing=true".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(private.code, 2);
    assert!(
        private.stderr.contains("does not export 'privateThing'"),
        "{}",
        private.stderr
    );

    let missing_runtime = run_cli(&[
        "plugin-manifest".to_owned(),
        "import-symbol".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--plugin".to_owned(),
        "helper".to_owned(),
        "--symbol".to_owned(),
        "publicThing".to_owned(),
        "--module-symbol".to_owned(),
        "other=true".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(missing_runtime.code, 2);
    assert!(
        missing_runtime
            .stderr
            .contains("module did not provide export 'publicThing'"),
        "{}",
        missing_runtime.stderr
    );

    let disabled = run_cli(&[
        "plugin-manifest".to_owned(),
        "import-symbol".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--disabled".to_owned(),
        "disabled-helper".to_owned(),
        "--plugin".to_owned(),
        "disabled-helper".to_owned(),
        "--symbol".to_owned(),
        "answer".to_owned(),
        "--module-symbol".to_owned(),
        "answer=42".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(disabled.code, 2);
    assert!(
        disabled
            .stderr
            .contains("plugin 'disabled-helper' is disabled"),
        "{}",
        disabled.stderr
    );

    remove_dir_all(root).expect("cleanup import errors");
}

fn write_module_plugin(
    root: &Path,
    name: &str,
    module_path: &str,
    exports: &[&str],
    disabled: bool,
) {
    let dir = root.join(name);
    create_dir_all(&dir).expect("plugin dir");
    let normalized_module_path = module_path.trim_start_matches("./");
    let path = dir.join(normalized_module_path);
    create_dir_all(path.parent().expect("module parent")).expect("module parent");
    write(&path, b"export const answer = 42;\n").expect("module");
    let mut full_manifest = serde_json::Map::from_iter([
        ("name".to_owned(), json!(name)),
        ("version".to_owned(), json!("1.0.0")),
        ("sdk".to_owned(), json!("*")),
        ("target".to_owned(), json!("js")),
        ("entry".to_owned(), json!("index.ts")),
        (
            "module".to_owned(),
            json!({ "path": module_path, "exports": exports }),
        ),
    ]);
    if disabled {
        full_manifest.insert("weight".to_owned(), json!(99));
    }
    write(
        dir.join("index.ts"),
        b"export default async function helper() {}\n",
    )
    .expect("entry");
    write(
        dir.join("plugin.json"),
        serde_json::to_vec_pretty(&serde_json::Value::Object(full_manifest)).expect("json"),
    )
    .expect("manifest");
}

#[test]
fn plugin_manifest_invoke_plan_cli_reports_universal_version_and_help() {
    let root = make_temp_dir("invoke-version-help");
    let plugins_dir = root.join("plugins");
    create_dir_all(&plugins_dir).expect("plugins");
    write_invoke_ts_plugin(
        &plugins_dir,
        "surface",
        serde_json::Map::from_iter([
            ("version".to_owned(), json!("2.3.4")),
            ("description".to_owned(), json!("surface reporter")),
            ("weight".to_owned(), json!(7)),
            (
                "cli".to_owned(),
                json!({ "command": "surface", "help": "maw surface <thing>", "aliases": ["sf"], "flags": { "--name": "string" } }),
            ),
            (
                "api".to_owned(),
                json!({ "path": "/api/surface", "methods": ["GET"] }),
            ),
            ("transport".to_owned(), json!({ "peer": true })),
            ("hooks".to_owned(), json!({ "on": ["message:send"] })),
        ]),
    );

    let version = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "invoke".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--plugin".to_owned(),
        "surface".to_owned(),
        "--arg".to_owned(),
        "--version".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(version["command"], "plugin-manifest");
    assert_eq!(version["kind"], "invoke");
    assert_eq!(version["plugin"], "surface");
    assert_eq!(version["result"]["ok"], true);
    let output = version["result"]["output"]
        .as_str()
        .expect("version output");
    assert!(output.contains("surface v2.3.4 (ts, weight:7)"), "{output}");
    assert!(output.contains("surface reporter"), "{output}");
    assert!(output.contains("cli:surface"), "{output}");
    assert!(output.contains("api:/api/surface"), "{output}");
    assert!(output.contains("hooks"), "{output}");
    assert!(output.contains("peer"), "{output}");

    let help = json_output(&run_cli(&[
        "plugin-manifest".to_owned(),
        "invoke".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.to_string_lossy().into_owned(),
        "--plugin".to_owned(),
        "surface".to_owned(),
        "--arg".to_owned(),
        "sub".to_owned(),
        "--arg".to_owned(),
        "--help".to_owned(),
        "--plan-json".to_owned(),
    ]));
    let output = help["result"]["output"].as_str().expect("help output");
    assert!(output.contains("usage: maw surface <thing>"), "{output}");
    assert!(output.contains("aliases: sf"), "{output}");
    assert!(output.contains("--name"), "{output}");
    assert!(output.contains("api: GET /api/surface"), "{output}");
    assert!(output.contains("hooks: on"), "{output}");

    remove_dir_all(root).expect("cleanup invoke version");
}
