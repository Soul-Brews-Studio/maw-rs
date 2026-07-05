use maw_cli::run_cli;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    input: FixtureInput,
    expected: Option<ExpectedDecision>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureInput {
    pane_current_command: Option<String>,
    no_attach: Option<bool>,
    requested_policy: Option<String>,
    force_split: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ExpectedDecision {
    action: String,
    reason: String,
}

#[test]
fn split_policy_plan_cli_matches_maw_js_fixtures() {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../maw-split/tests/fixtures/split-policy.fixtures.json"
    ))
    .expect("valid split policy fixtures");

    for fixture in fixtures {
        let mut argv = vec!["split-policy".to_owned(), "--plan-json".to_owned()];
        if let Some(command) = &fixture.input.pane_current_command {
            argv.push("--pane-current-command".to_owned());
            argv.push(command.clone());
        }
        if let Some(policy) = &fixture.input.requested_policy {
            argv.push("--requested-policy".to_owned());
            argv.push(policy.clone());
        }
        if fixture.input.no_attach.unwrap_or(false) {
            argv.push("--no-attach".to_owned());
        }
        if fixture.input.force_split.unwrap_or(false) {
            argv.push("--force-split".to_owned());
        }

        let output = run_cli(&argv);
        if let Some(error) = fixture.error {
            assert_eq!(output.code, 2, "{}", fixture.name);
            assert!(
                output.stderr.contains(&error),
                "{}: {}",
                fixture.name,
                output.stderr
            );
            continue;
        }

        assert_eq!(output.code, 0, "{} stderr: {}", fixture.name, output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
            });
        let expected = fixture.expected.expect("expected split decision");
        assert_eq!(json["command"], "split-policy", "{}", fixture.name);
        assert_eq!(json["action"], expected.action, "{}", fixture.name);
        assert_eq!(json["reason"], expected.reason, "{}", fixture.name);
    }
}

#[test]
fn split_policy_accepts_maw_js_claude_pane_policy_alias() {
    let argv = vec![
        "split-policy".to_owned(),
        "--plan-json".to_owned(),
        "--pane-current-command".to_owned(),
        "claude".to_owned(),
        "--claude-pane-policy".to_owned(),
        "link-window".to_owned(),
    ];

    let output = run_cli(&argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid json");
    assert_eq!(json["action"], "link-window");
    assert_eq!(json["reason"], "claude-policy");
}
