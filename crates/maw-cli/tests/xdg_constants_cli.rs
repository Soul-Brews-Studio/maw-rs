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
fn xdg_constants_plan_locks_env_precedence_paths_and_instance_contract() {
    let value = json(&["xdg", "constants", "--plan-json"]);

    assert_eq!(value["command"], "xdg");
    assert_eq!(value["action"], "constants");
    assert_eq!(
        value["actions"],
        serde_json::json!(["paths", "core-paths", "validate-instance"])
    );
    assert_eq!(
        value["truthyMawXdg"],
        serde_json::json!(["1", "true", "yes", "on"])
    );
    assert_eq!(
        value["overrideEnv"],
        serde_json::json!([
            "MAW_HOME",
            "MAW_CONFIG_DIR",
            "MAW_DATA_DIR",
            "MAW_STATE_DIR",
            "MAW_CACHE_DIR"
        ])
    );
    assert_eq!(
        value["xdgBaseEnv"],
        serde_json::json!([
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME"
        ])
    );
    assert_eq!(
        value["legacyDirs"],
        serde_json::json!({"runtime":"$HOME/.maw","config":"$HOME/.config/maw","data":"$HOME/.maw","state":"$HOME/.maw","cache":"$HOME/.maw"})
    );
    assert_eq!(
        value["xdgDirs"],
        serde_json::json!({"runtime":"$XDG_STATE_HOME/maw","config":"$XDG_CONFIG_HOME/maw","data":"$XDG_DATA_HOME/maw","state":"$XDG_STATE_HOME/maw","cache":"$XDG_CACHE_HOME/maw"})
    );
    assert_eq!(
        value["samplePaths"],
        serde_json::json!({"data":["plugins"],"state":["peers.json"],"cache":["registry-cache.json"],"config":["maw.config.json"]})
    );
    assert_eq!(value["corePaths"]["fleetDir"], "configDir/fleet");
    assert_eq!(
        value["corePaths"]["configFile"],
        "configDir/maw.config.json"
    );
    assert_eq!(value["instanceName"]["maxBytes"], 32);
    assert_eq!(value["instanceName"]["first"], "lowercase ascii alnum");
    assert_eq!(
        value["instanceName"]["rest"],
        "lowercase ascii alnum, underscore, hyphen"
    );
}

#[test]
fn xdg_constants_rejects_unknown_flags() {
    let output = run(&["xdg", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("xdg constants: unknown arg --bad"));
    assert!(output.stderr.contains("maw-rs xdg constants"));
}
