use maw_cli::run_cli;

fn json(output: &maw_cli::CliOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
        panic!("invalid json: {error}\n{}", output.stdout);
    })
}

#[test]
fn plugin_scaffold_validate_plan_cli_matches_maw_js_name_contract() {
    for name in ["hello", "my-plugin-2", "my_plugin"] {
        let output = json(&run_cli(&[
            "plugin-scaffold".to_owned(),
            "validate-name".to_owned(),
            "--name".to_owned(),
            name.to_owned(),
            "--plan-json".to_owned(),
        ]));
        assert_eq!(output["command"], "plugin-scaffold");
        assert_eq!(output["kind"], "validate-name");
        assert_eq!(output["valid"], true, "{name}");
        assert!(output["error"].is_null(), "{name}: {output}");
    }

    for name in ["", "2plugin", "MyPlugin", "my plugin"] {
        let output = json(&run_cli(&[
            "plugin-scaffold".to_owned(),
            "validate-name".to_owned(),
            "--name".to_owned(),
            name.to_owned(),
            "--plan-json".to_owned(),
        ]));
        assert_eq!(output["valid"], false, "{name}");
        assert!(output["error"]
            .as_str()
            .expect("error text")
            .contains(if name.is_empty() {
                "name is required"
            } else {
                "invalid"
            }));
    }
}

#[test]
fn plugin_scaffold_manifest_plan_cli_matches_rust_and_as_contracts() {
    let rust = json(&run_cli(&[
        "plugin-scaffold".to_owned(),
        "manifest".to_owned(),
        "--name".to_owned(),
        "my-rust-plugin".to_owned(),
        "--rust".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(rust["command"], "plugin-scaffold");
    assert_eq!(rust["kind"], "manifest");
    assert_eq!(rust["language"], "rust");
    assert_eq!(rust["manifest"]["name"], "my-rust-plugin");
    assert_eq!(rust["manifest"]["version"], "0.1.0");
    assert_eq!(rust["manifest"]["sdk"], "^1.0.0");
    assert_eq!(
        rust["manifest"]["wasm"],
        "./target/wasm32-unknown-unknown/release/my_rust_plugin.wasm"
    );
    assert_eq!(rust["manifest"]["cli"]["command"], "my-rust-plugin");
    assert_eq!(
        rust["manifest"]["api"]["path"],
        "/api/plugins/my-rust-plugin"
    );
    assert_eq!(
        rust["manifest"]["api"]["methods"],
        serde_json::json!(["GET", "POST"])
    );

    let assembly_script = json(&run_cli(&[
        "plugin-scaffold".to_owned(),
        "manifest".to_owned(),
        "--name".to_owned(),
        "my-as-plugin".to_owned(),
        "--as".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(assembly_script["language"], "assemblyscript");
    assert_eq!(assembly_script["manifest"]["name"], "my-as-plugin");
    assert_eq!(assembly_script["manifest"]["wasm"], "./build/release.wasm");
    assert_eq!(
        assembly_script["manifest"]["cli"]["command"],
        "my-as-plugin"
    );
    assert_eq!(
        assembly_script["manifest"]["api"]["path"],
        "/api/plugins/my-as-plugin"
    );
}

#[test]
fn plugin_scaffold_manifest_plan_normalizes_underscores_like_maw_js() {
    let output = json(&run_cli(&[
        "plugin-scaffold".to_owned(),
        "manifest".to_owned(),
        "--name".to_owned(),
        "my_plugin".to_owned(),
        "--rust".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(output["manifest"]["name"], "my-plugin");
    assert_eq!(output["manifest"]["cli"]["command"], "my-plugin");
    assert_eq!(output["manifest"]["api"]["path"], "/api/plugins/my-plugin");
    assert_eq!(
        output["manifest"]["wasm"],
        "./target/wasm32-unknown-unknown/release/my_plugin.wasm"
    );
}

#[test]
fn plugin_scaffold_plan_rejects_missing_or_conflicting_type_flags() {
    let output = run_cli(&[
        "plugin-scaffold".to_owned(),
        "manifest".to_owned(),
        "--name".to_owned(),
        "my-plugin".to_owned(),
    ]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("Specify either --rust or --as"));

    let output = run_cli(&[
        "plugin-scaffold".to_owned(),
        "manifest".to_owned(),
        "--name".to_owned(),
        "my-plugin".to_owned(),
        "--rust".to_owned(),
        "--as".to_owned(),
    ]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("Specify --rust or --as, not both"));
}
