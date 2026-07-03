#[test]
fn enum_string_helpers_cover_all_public_variants() {
    assert_eq!(CliFlagKind::Boolean.as_str(), "boolean");
    assert_eq!(CliFlagKind::String.as_str(), "string");
    assert_eq!(CliFlagKind::Number.as_str(), "number");
    assert_eq!(PluginTier::Core.as_str(), "core");
    assert_eq!(PluginTier::Standard.as_str(), "standard");
    assert_eq!(PluginTier::Extra.as_str(), "extra");
    assert_eq!(HookPolicy::BestEffort.as_str(), "best-effort");
    assert_eq!(HookPolicy::FailFast.as_str(), "fail-fast");
}

#[test]
fn additional_parser_errors_cover_missing_arrays_and_type_branches() {
    expect_error(
        &json!({ "cli": { "command": "x", "flags": { "bad": 1 } } }),
        r#"plugin.json: cli.flags["bad"] must be"#,
    );
    expect_api_error(
        &json!({ "api": { "path": "/api/demo" } }),
        "plugin.json: api.methods must be an array",
    );
    expect_module_error(
        &json!({ "module": { "path": "./mod.ts" } }),
        "plugin.json: module.exports must be a non-empty array of strings",
    );
    expect_dependencies_error(
        &json!({ "dependencies": { "plugins": [""] } }),
        "plugin.json: dependencies.plugins must be an array of plugin names",
    );
    expect_tier_error(&json!({ "tier": 1 }), "plugin.json: tier must be");
}

#[test]
fn help_hook_keys_cover_all_lifecycle_and_array_surfaces_through_help_rendering() {
    let hooks = parse_hooks(&json!({
        "hooks": {
            "gate": ["a"],
            "filter": ["b"],
            "on": ["c"],
            "late": ["d"],
            "wake": {},
            "sleep": {},
            "serve": {}
        }
    }))
    .expect("valid hooks")
    .expect("hooks present");

    assert!(hooks.gate.is_some());
    assert!(hooks.filter.is_some());
    assert!(hooks.on.is_some());
    assert!(hooks.late.is_some());
    assert!(hooks.wake.is_some());
    assert!(hooks.sleep.is_some());
    assert!(hooks.serve.is_some());
}

fn expect_error(input: &serde_json::Value, expected: &str) {
    let error = parse_cli(input).expect_err("expected parse_cli error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_api_error(input: &serde_json::Value, expected: &str) {
    let error = parse_api(input).expect_err("expected parse_api error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_hooks_error(input: &serde_json::Value, expected: &str) {
    let error = parse_hooks(input).expect_err("expected parse_hooks error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_cron_error(input: &serde_json::Value, expected: &str) {
    let error = parse_cron(input).expect_err("expected parse_cron error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_module_error(input: &serde_json::Value, expected: &str) {
    let error = parse_module(input).expect_err("expected parse_module error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_transport_error(input: &serde_json::Value, expected: &str) {
    let error = parse_transport(input).expect_err("expected parse_transport error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_engine_error(input: &serde_json::Value, expected: &str) {
    let error = parse_engine(input).expect_err("expected parse_engine error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_target_error(input: &serde_json::Value, expected: &str) {
    let error = parse_target(input).expect_err("expected parse_target error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_capability_namespaces_error(input: &serde_json::Value, expected: &str) {
    let error =
        parse_capability_namespaces(input).expect_err("expected parse_capability_namespaces error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_capabilities_error(input: &serde_json::Value, expected: &str) {
    let error = parse_capabilities(input, &[]).expect_err("expected parse_capabilities error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_dependencies_error(input: &serde_json::Value, expected: &str) {
    let error = parse_dependencies(input).expect_err("expected parse_dependencies error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_artifact_error(input: &serde_json::Value, expected: &str) {
    let error = parse_artifact(input).expect_err("expected parse_artifact error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

fn expect_tier_error(input: &serde_json::Value, expected: &str) {
    let error = parse_tier(input).expect_err("expected parse_tier error");
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}
