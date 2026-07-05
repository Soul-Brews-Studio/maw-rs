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
fn split_policy_constants_locks_actions_reasons_and_precedence() {
    let value = json(&["split-policy", "constants", "--plan-json"]);

    assert_eq!(value["command"], "split-policy");
    assert_eq!(value["kind"], "constants");
    assert_eq!(
        value["actions"],
        serde_json::json!(["split", "background-tab", "link-window", "refuse"])
    );
    assert_eq!(
        value["reasons"],
        serde_json::json!([
            "not-attaching",
            "force-split",
            "not-claude",
            "claude-policy"
        ])
    );
    assert_eq!(value["defaultClaudePolicy"], "background-tab");
    assert_eq!(
        value["policyFlags"],
        serde_json::json!(["--requested-policy", "--claude-pane-policy"])
    );
    assert_eq!(
        value["precedence"],
        serde_json::json!(["no-attach", "force-split", "not-claude", "claude-policy"])
    );
    assert_eq!(
        value["claudeLikeCommands"],
        serde_json::json!(["claude", "version-like semver command"])
    );
}

#[test]
fn split_policy_constants_rejects_unknown_flags() {
    let output = run(&["split-policy", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("split-policy constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs split-policy constants"));
}
