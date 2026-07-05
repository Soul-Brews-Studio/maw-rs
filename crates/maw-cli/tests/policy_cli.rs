use maw_cli::run_cli;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureRoot {
    constants: ConstantsFixture,
    weight_to_tier: Vec<WeightFixture>,
    default_active_groups: Vec<DefaultActiveGroupFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConstantsFixture {
    known_tiers: Vec<String>,
    default_tier: String,
}

#[derive(Debug, Deserialize)]
struct WeightFixture {
    name: String,
    weight: i32,
    expected: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultActiveGroupFixture {
    name: String,
    key: String,
    migration: String,
    expected_plugins: Vec<String>,
    excluded_plugins: Vec<String>,
}

#[test]
fn policy_plan_cli_matches_maw_js_fixtures() {
    let fixtures: FixtureRoot = serde_json::from_str(include_str!(
        "../../maw-policy/tests/fixtures/plugin-policy.fixtures.json"
    ))
    .expect("valid plugin policy fixture json");

    let output = run_cli(&[
        "policy".to_owned(),
        "--constants".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("constants invalid json: {error}\n{}", output.stdout));
    assert_eq!(json["command"], "policy");
    assert_eq!(json["kind"], "constants");
    let known_tiers: Vec<String> = json["knownTiers"]
        .as_array()
        .expect("knownTiers array")
        .iter()
        .map(|value| value.as_str().expect("known tier string").to_owned())
        .collect();
    assert_eq!(known_tiers, fixtures.constants.known_tiers);
    assert_eq!(json["defaultTier"], fixtures.constants.default_tier);

    for fixture in fixtures.weight_to_tier {
        let output = run_cli(&[
            "policy".to_owned(),
            "--weight".to_owned(),
            fixture.weight.to_string(),
            "--plan-json".to_owned(),
        ]);
        assert_eq!(output.code, 0, "{}: {}", fixture.name, output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
            });
        assert_eq!(json["command"], "policy", "{}", fixture.name);
        assert_eq!(json["kind"], "weightToTier", "{}", fixture.name);
        assert_eq!(json["weight"], fixture.weight, "{}", fixture.name);
        assert_eq!(json["tier"], fixture.expected, "{}", fixture.name);
    }

    for fixture in fixtures.default_active_groups {
        let output = run_cli(&[
            "policy".to_owned(),
            "--default-active".to_owned(),
            fixture.key.clone(),
            "--plan-json".to_owned(),
        ]);
        assert_eq!(output.code, 0, "{}: {}", fixture.name, output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
            });
        assert_eq!(json["command"], "policy", "{}", fixture.name);
        assert_eq!(json["kind"], "defaultActiveGroup", "{}", fixture.name);
        assert_eq!(json["key"], fixture.key, "{}", fixture.name);
        assert_eq!(json["migration"], fixture.migration, "{}", fixture.name);
        let plugins: Vec<String> = json["plugins"]
            .as_array()
            .expect("plugins array")
            .iter()
            .map(|value| value.as_str().expect("plugin string").to_owned())
            .collect();
        assert_eq!(plugins, fixture.expected_plugins, "{}", fixture.name);

        for plugin in fixture.expected_plugins {
            assert_default_active_includes(&fixture.key, &plugin, true, &fixture.name);
        }
        for plugin in fixture.excluded_plugins {
            assert_default_active_includes(&fixture.key, &plugin, false, &fixture.name);
        }
    }
}

fn assert_default_active_includes(key: &str, plugin: &str, expected: bool, fixture_name: &str) {
    let output = run_cli(&[
        "policy".to_owned(),
        "--default-active".to_owned(),
        key.to_owned(),
        "--includes".to_owned(),
        plugin.to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(output.code, 0, "{fixture_name}/{plugin}: {}", output.stderr);
    let json: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("{fixture_name}/{plugin} invalid json: {error}"));
    assert_eq!(json["kind"], "defaultActiveIncludes", "{fixture_name}");
    assert_eq!(json["included"], expected, "{fixture_name}/{plugin}");
}

#[test]
fn plugin_policy_alias_and_errors_match_plan_contract() {
    let alias_output = run_cli(&[
        "plugin-policy".to_owned(),
        "--weight".to_owned(),
        "50".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(alias_output.code, 0, "{}", alias_output.stderr);
    let json: serde_json::Value = serde_json::from_str(&alias_output.stdout).expect("valid json");
    assert_eq!(json["tier"], "extra");

    let bad_weight = run_cli(&[
        "policy".to_owned(),
        "--weight".to_owned(),
        "heavy".to_owned(),
    ]);
    assert_eq!(bad_weight.code, 2);
    assert!(bad_weight.stderr.contains("--weight must be an integer"));

    let missing_key = run_cli(&[
        "policy".to_owned(),
        "--default-active".to_owned(),
        "9999".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(missing_key.code, 2);
    assert!(missing_key.stderr.contains("unknown --default-active key"));
}
