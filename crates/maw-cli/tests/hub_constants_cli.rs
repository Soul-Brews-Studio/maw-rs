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
fn hub_constants_plan_locks_workspace_loader_and_connection_vocabulary() {
    let value = json(&["hub", "constants", "--plan-json"]);

    assert_eq!(value["command"], "hub");
    assert_eq!(value["action"], "constants");
    assert_eq!(
        value["actions"],
        serde_json::json!(["validate-workspace", "load-workspaces"])
    );
    assert_eq!(
        value["requiredFields"],
        serde_json::json!(["id", "hubUrl", "token", "sharedAgents"])
    );
    assert_eq!(value["validProtocols"], serde_json::json!(["ws", "wss"]));
    assert_eq!(value["workspaceDirName"], "workspaces");
    assert_eq!(value["fileExtension"], "json");
    assert_eq!(value["heartbeatMs"], 30000);
    assert_eq!(value["reconnectBaseMs"], 1000);
    assert_eq!(value["reconnectMaxMs"], 60000);
    assert_eq!(
        value["validationReasons"],
        serde_json::json!([
            "not an object",
            "missing/empty id",
            "missing/empty hubUrl",
            "missing/empty token",
            "sharedAgents must be array",
            "hubUrl must be ws:|wss: (got <protocol>:)",
            "hubUrl not a valid URL"
        ])
    );
    assert_eq!(
        value["warningPrefixes"],
        serde_json::json!([
            "[hub] failed to parse workspace config",
            "[hub] invalid workspace config"
        ])
    );
}

#[test]
fn hub_constants_rejects_unknown_flags() {
    let output = run(&["hub", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("hub constants: unknown arg --bad"));
    assert!(output.stderr.contains("maw-rs hub constants"));
}
