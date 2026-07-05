// Ported from maw-js test/spec/matcher-resolve-target.fixtures.json into the
// maw-rs side-by-side dry-run CLI resolve surface.

use maw_cli::{run_cli, CliOutput};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveFixture {
    name: String,
    mode: Mode,
    input: ResolveInput,
    expected: ExpectedResolve,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Mode {
    #[serde(rename = "byName")]
    ByName,
    #[serde(rename = "session")]
    Session,
    #[serde(rename = "worktree")]
    Worktree,
}

impl Mode {
    const fn cli_value(&self) -> &'static str {
        match self {
            Self::ByName => "by-name",
            Self::Session => "session",
            Self::Worktree => "worktree",
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResolveInput {
    target: String,
    items: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExpectedResolve {
    kind: String,
    #[serde(rename = "match")]
    #[serde(default)]
    match_name: Option<String>,
    #[serde(default)]
    candidates: Option<Vec<String>>,
    #[serde(default)]
    hints: Option<Vec<String>>,
}

fn args(values: &[String]) -> Vec<String> {
    values.to_vec()
}

#[test]
fn resolve_plan_json_matches_maw_js_matcher_fixtures() {
    let fixtures: Vec<ResolveFixture> = serde_json::from_str(include_str!(
        "../../maw-matcher/tests/fixtures/matcher-resolve-target.fixtures.json"
    ))
    .expect("valid matcher fixtures");

    assert_eq!(fixtures.len(), 16, "maw-js matcher fixture count changed");
    for fixture in fixtures {
        let mut argv = vec![
            "resolve".to_owned(),
            "--mode".to_owned(),
            fixture.mode.cli_value().to_owned(),
            fixture.input.target.clone(),
        ];
        argv.extend(fixture.input.items.clone());
        argv.push("--plan-json".to_owned());

        let output = run_cli(&args(&argv));
        assert_eq!(
            output,
            CliOutput {
                code: 0,
                stdout: expected_json(&fixture),
                stderr: String::new(),
            },
            "fixture failed: {}",
            fixture.name
        );
    }
}

#[test]
fn resolve_plan_rejects_missing_target_or_items() {
    let output = run_cli(&["resolve".to_owned(), "--plan-json".to_owned()]);
    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.contains("resolve: expected <target>"));
}

fn expected_json(fixture: &ResolveFixture) -> String {
    let mut fields = vec![
        "\"command\":\"resolve\"".to_owned(),
        format!("\"mode\":{}", json_string(fixture.mode.cli_value())),
        format!("\"target\":{}", json_string(&fixture.input.target)),
        format!("\"kind\":{}", json_string(&fixture.expected.kind)),
    ];
    if let Some(matched) = &fixture.expected.match_name {
        fields.push(format!("\"match\":{}", json_string(matched)));
    }
    if let Some(candidates) = &fixture.expected.candidates {
        fields.push(format!("\"candidates\":{}", json_array(candidates)));
    }
    if let Some(hints) = &fixture.expected.hints {
        fields.push(format!("\"hints\":{}", json_array(hints)));
    }
    format!("{{{}}}\n", fields.join(","))
}

fn json_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serializes")
}
