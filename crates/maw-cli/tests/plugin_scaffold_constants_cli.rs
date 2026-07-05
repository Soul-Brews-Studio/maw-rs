use maw_cli::run_cli;
use serde_json::Value;

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

fn json(args: &[&str]) -> Value {
    let output = run(args);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap()
}

#[test]
fn plugin_scaffold_constants_locks_name_manifest_and_copy_contracts() {
    let value = json(&["plugin-scaffold", "constants", "--plan-json"]);

    assert_eq!(value["command"], "plugin-scaffold");
    assert_eq!(value["action"], "constants");
    assert_eq!(
        value["actions"],
        serde_json::json!(["validate-name", "manifest"])
    );
    assert_eq!(
        value["languages"],
        serde_json::json!(["rust", "assemblyscript"])
    );
    assert_eq!(value["nameRules"]["first"], "lowercase ascii letter");
    assert_eq!(
        value["nameRules"]["rest"],
        "lowercase ascii letters, digits, hyphen, underscore"
    );
    assert_eq!(value["nameRules"]["emptyError"], "name is required");
    assert_eq!(
        value["manifestDefaults"],
        serde_json::json!({"version":"0.1.0","sdk":"^1.0.0","author":"","apiMethods":["GET","POST"]})
    );
    assert_eq!(
        value["slugNormalization"],
        serde_json::json!({"slug":"underscores become hyphens","rustWasmArtifact":"hyphens become underscores"})
    );
    assert_eq!(
        value["wasmPaths"],
        serde_json::json!({"rust":"./target/wasm32-unknown-unknown/release/<crate_name>.wasm","assemblyscript":"./build/release.wasm"})
    );
    assert_eq!(
        value["copyTreeSkips"],
        serde_json::json!(["target", ".git", "node_modules"])
    );
    assert_eq!(
        value["guardErrors"],
        serde_json::json!([
            "missing-type",
            "conflicting-types",
            "missing-name",
            "invalid-name",
            "destination-exists",
            "scaffold"
        ])
    );
}

#[test]
fn plugin_scaffold_constants_rejects_unknown_flags() {
    let output = run(&["plugin-scaffold", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("plugin-scaffold constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs plugin-scaffold constants"));
}
