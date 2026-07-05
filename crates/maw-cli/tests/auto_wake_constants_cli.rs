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
fn auto_wake_constants_plan_locks_sites_flags_and_manifest_vocabulary() {
    let value = json(&["auto-wake", "constants", "--plan-json"]);

    assert_eq!(value["command"], "auto-wake");
    assert_eq!(value["action"], "constants");
    assert_eq!(
        value["sites"],
        serde_json::json!(["view", "hey", "api-send", "api-wake", "peek", "bud", "wake-cmd"])
    );
    assert_eq!(
        value["fleetFlags"],
        serde_json::json!(["fleet-known", "unknown-fleet"])
    );
    assert_eq!(
        value["livenessFlags"],
        serde_json::json!(["live", "not-live"])
    );
    assert_eq!(
        value["overrideFlags"],
        serde_json::json!(["wake", "no-wake"])
    );
    assert_eq!(
        value["targetFlags"],
        serde_json::json!(["canonical-target"])
    );
    assert_eq!(
        value["manifestFields"],
        serde_json::json!(["manifest-source", "manifest-live"])
    );
    assert_eq!(
        value["manifestLiveValues"],
        serde_json::json!(["true", "false"])
    );
}

#[test]
fn auto_wake_constants_rejects_unknown_flags() {
    let output = run(&["auto-wake", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auto-wake constants: unknown arg --bad"));
    assert!(output.stderr.contains("maw-rs auto-wake constants"));
}
