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
fn pair_api_constants_plan_locks_endpoint_status_and_seed_shapes() {
    let value = json(&["pair-api", "constants", "--plan-json"]);

    assert_eq!(value["command"], "pair-api");
    assert_eq!(value["action"], "constants");
    assert_eq!(
        value["endpoints"],
        serde_json::json!(["generate", "probe", "accept", "status"])
    );
    assert_eq!(
        value["probeStatuses"],
        serde_json::json!(["live", "not_found", "expired", "consumed", "invalid_shape"])
    );
    assert_eq!(
        value["acceptErrors"],
        serde_json::json!([
            "bad_request",
            "not_found",
            "expired",
            "consumed",
            "invalid_shape"
        ])
    );
    assert_eq!(
        value["statusStates"],
        serde_json::json!(["live", "consumed", "not_found", "expired", "invalid_shape"])
    );
    assert_eq!(value["httpStatuses"]["generateCreated"], 201);
    assert_eq!(value["httpStatuses"]["ok"], 200);
    assert_eq!(value["httpStatuses"]["badRequest"], 400);
    assert_eq!(value["httpStatuses"]["notFound"], 404);
    assert_eq!(value["httpStatuses"]["gone"], 410);
    assert_eq!(value["seedCodeShape"], "code:ttl_ms:created_at_ms");
    assert_eq!(value["seedAcceptedShape"], "node=url");
    assert_eq!(
        value["redactedFields"],
        serde_json::json!(["federationToken"])
    );
}

#[test]
fn pair_api_constants_rejects_unknown_flags() {
    let output = run(&["pair-api", "constants", "--bogus"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("pair-api constants: unknown arg --bogus"));
}
