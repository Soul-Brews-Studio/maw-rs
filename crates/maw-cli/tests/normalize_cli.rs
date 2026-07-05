// Ported from maw-js test/spec/normalize-target.fixtures.json into the maw-rs
// side-by-side dry-run CLI normalize surface.

use maw_cli::{run_cli, CliOutput};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct NormalizeFixture {
    name: String,
    input: String,
    expected: String,
}

#[test]
fn normalize_plan_json_matches_maw_js_fixtures() {
    let fixtures: Vec<NormalizeFixture> = serde_json::from_str(include_str!(
        "../../maw-matcher/tests/fixtures/normalize-target.fixtures.json"
    ))
    .expect("valid normalize fixtures");

    assert_eq!(fixtures.len(), 12, "maw-js normalize fixture count changed");
    for fixture in fixtures {
        let output = run_cli(&[
            "normalize".to_owned(),
            fixture.input.clone(),
            "--plan-json".to_owned(),
        ]);
        assert_eq!(
            output,
            CliOutput {
                code: 0,
                stdout: format!(
                    "{{\"command\":\"normalize\",\"input\":{},\"normalized\":{}}}\n",
                    serde_json::to_string(&fixture.input).expect("input serializes"),
                    serde_json::to_string(&fixture.expected).expect("expected serializes"),
                ),
                stderr: String::new(),
            },
            "fixture failed: {}",
            fixture.name
        );
    }
}

#[test]
fn normalize_plan_rejects_missing_target() {
    let output = run_cli(&["normalize".to_owned(), "--plan-json".to_owned()]);
    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.contains("normalize: expected <target>"));
}
