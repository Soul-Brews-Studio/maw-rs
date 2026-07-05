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
fn pair_api_auto_constants_plan_locks_auto_pair_status_and_flag_vocabulary() {
    let value = json(&["pair-api-auto", "constants", "--plan-json"]);

    assert_eq!(value["command"], "pair-api-auto");
    assert_eq!(value["action"], "constants");
    assert_eq!(
        value["requiredInput"],
        serde_json::json!(["remote-node", "remote-url", "zid"])
    );
    assert_eq!(value["helloShape"], "zid:seen_at_ms");
    assert_eq!(
        value["addOutcomes"],
        serde_json::json!(["ok", "one-way", "pubkey-mismatch", "error"])
    );
    assert_eq!(
        value["errorCodes"],
        serde_json::json!([
            "missing_fields",
            "no_recent_hello",
            "pubkey_mismatch",
            "add_error"
        ])
    );
    assert_eq!(value["httpStatuses"]["ok"], 200);
    assert_eq!(value["httpStatuses"]["badRequest"], 400);
    assert_eq!(value["httpStatuses"]["forbidden"], 403);
    assert_eq!(value["httpStatuses"]["conflict"], 409);
    assert_eq!(
        value["redactedFields"],
        serde_json::json!(["federationToken"])
    );
    assert_eq!(value["markSymmetricCheckOnSuccess"], true);
}

#[test]
fn pair_api_auto_constants_rejects_unknown_flags() {
    let output = run(&["pair-api-auto", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("pair-api-auto constants: unknown arg --bad"));
    assert!(output.stderr.contains("maw-rs pair-api-auto constants"));
}
