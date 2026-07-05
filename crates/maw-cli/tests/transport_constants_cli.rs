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
fn transport_constants_locks_router_classifier_and_spec_vocabulary() {
    let value = json(&["transport", "constants", "--plan-json"]);

    assert_eq!(value["command"], "transport");
    assert_eq!(value["kind"], "constants");
    assert_eq!(
        value["actions"],
        serde_json::json!(["classify-error", "classify-empty", "send"])
    );
    assert_eq!(
        value["failureReasons"],
        serde_json::json!([
            "timeout",
            "unreachable",
            "auth",
            "rate_limit",
            "rejected",
            "parse_error",
            "unknown"
        ])
    );
    assert_eq!(
        value["retryableReasons"],
        serde_json::json!(["timeout", "unreachable", "rate_limit"])
    );
    assert_eq!(
        value["fatalReasons"],
        serde_json::json!(["auth", "rejected", "parse_error", "unknown"])
    );
    assert_eq!(
        value["sendFailover"],
        serde_json::json!([
            "skip disconnected",
            "skip unreachable",
            "fall through false",
            "fall through retryable throw",
            "stop on fatal throw",
            "first ok wins"
        ])
    );
    assert_eq!(
        value["transportSpec"],
        serde_json::json!({"shape":"name[:connected][:canReach][:ok|false|throw=err]","booleanValues":["true","false"],"defaultConnected":true,"defaultCanReach":true,"defaultAction":"ok"})
    );
    assert_eq!(
        value["defaultTarget"],
        serde_json::json!({"oracle":"neo","host":null,"tmuxTarget":"neo:1","message":"hello","from":"codex"})
    );
}

#[test]
fn transport_constants_rejects_unknown_flags() {
    let output = run(&["transport", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("transport constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs transport constants"));
}
