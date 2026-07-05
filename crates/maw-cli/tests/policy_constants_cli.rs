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
fn policy_constants_subcommand_locks_tiers_thresholds_and_default_active_waves() {
    let value = json(&["policy", "constants", "--plan-json"]);

    assert_eq!(value["command"], "policy");
    assert_eq!(value["kind"], "constants");
    assert_eq!(
        value["knownTiers"],
        serde_json::json!(["core", "standard", "extra"])
    );
    assert_eq!(value["defaultTier"], "core");
    assert_eq!(
        value["weightThresholds"],
        serde_json::json!({"core":"weight < 10","standard":"10 <= weight < 50","extra":"weight >= 50"})
    );
    assert_eq!(
        value["defaultActiveKeys"],
        serde_json::json!(["1500", "1514", "1523", "1524", "1531"])
    );
    assert_eq!(
        value["defaultActiveMigrations"],
        serde_json::json!([
            "defaultActivePlugins1500",
            "defaultActivePlugins1514",
            "defaultActivePlugins1523",
            "defaultActivePlugins1524",
            "defaultActivePlugins1531"
        ])
    );
    assert_eq!(
        value["aliases"],
        serde_json::json!(["policy", "plugin-policy"])
    );
}

#[test]
fn policy_constants_subcommand_rejects_unknown_flags() {
    let output = run(&["policy", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("policy constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs policy constants"));
}
