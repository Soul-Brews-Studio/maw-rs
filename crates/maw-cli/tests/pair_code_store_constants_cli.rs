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
fn pair_code_store_constants_plan_locks_store_modes_states_and_shapes() {
    let value = json(&["pair-code-store", "constants", "--plan-json"]);

    assert_eq!(value["command"], "pair-code-store");
    assert_eq!(value["action"], "constants");
    assert_eq!(
        value["modes"],
        serde_json::json!(["register", "lookup", "consume"])
    );
    assert_eq!(
        value["states"],
        serde_json::json!(["live", "not-found", "expired", "consumed"])
    );
    assert_eq!(value["seedCodeShape"], "code:ttl_ms:created_at_ms");
    assert_eq!(
        value["entryFields"],
        serde_json::json!(["code", "expiresAt", "createdAt", "consumed"])
    );
    assert_eq!(value["normalization"], "normalize-pair-code");
    assert_eq!(value["registerRequires"], serde_json::json!(["ttl-ms"]));
    assert_eq!(value["lookupRequires"], serde_json::json!(["code", "now"]));
}

#[test]
fn pair_code_store_constants_rejects_unknown_flags() {
    let output = run(&["pair-code-store", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("pair-code-store constants: unknown arg --bad"));
    assert!(output.stderr.contains("maw-rs pair-code-store constants"));
}
