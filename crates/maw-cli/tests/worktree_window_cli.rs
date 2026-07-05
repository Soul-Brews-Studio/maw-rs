use maw_cli::run_cli;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    input: FixtureInput,
    expected: ExpectedResolution,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureInput {
    main_repo_name: String,
    wt_name: String,
    sessions: Vec<FixtureSession>,
}

#[derive(Debug, Deserialize)]
struct FixtureSession {
    name: String,
    windows: Vec<FixtureWindow>,
}

#[derive(Debug, Deserialize)]
struct FixtureWindow {
    index: u32,
    name: String,
    active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ExpectedResolution {
    Bound {
        window: String,
    },
    Ambiguous {
        query: String,
        candidates: Vec<String>,
    },
    None,
}

#[test]
fn worktree_window_plan_cli_matches_maw_js_fixtures() {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../maw-worktree/tests/fixtures/worktree-window-match.fixtures.json"
    ))
    .expect("valid worktree fixtures");

    for fixture in fixtures {
        let mut argv = vec![
            "worktree-window".to_owned(),
            "--plan-json".to_owned(),
            "--main-repo-name".to_owned(),
            fixture.input.main_repo_name.clone(),
            "--wt-name".to_owned(),
            fixture.input.wt_name.clone(),
        ];
        for session in &fixture.input.sessions {
            argv.push("--session".to_owned());
            argv.push(session.name.clone());
            for window in &session.windows {
                argv.push("--window".to_owned());
                argv.push(format!(
                    "{}:{}:{}",
                    window.index, window.name, window.active
                ));
            }
        }

        let output = run_cli(&argv);
        assert_eq!(output.code, 0, "{} stderr: {}", fixture.name, output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
            });
        assert_eq!(json["command"], "worktree-window", "{}", fixture.name);
        assert_eq!(
            json["mainRepoName"], fixture.input.main_repo_name,
            "{}",
            fixture.name
        );
        assert_eq!(json["wtName"], fixture.input.wt_name, "{}", fixture.name);
        match fixture.expected {
            ExpectedResolution::Bound { window } => {
                assert_eq!(json["kind"], "bound", "{}", fixture.name);
                assert_eq!(json["window"], window, "{}", fixture.name);
            }
            ExpectedResolution::Ambiguous { query, candidates } => {
                assert_eq!(json["kind"], "ambiguous", "{}", fixture.name);
                assert_eq!(json["query"], query, "{}", fixture.name);
                let actual = json["candidates"].as_array().expect("candidates array");
                assert_eq!(actual.len(), candidates.len(), "{}", fixture.name);
                for (actual, expected) in actual.iter().zip(candidates) {
                    assert_eq!(
                        actual,
                        &serde_json::Value::String(expected),
                        "{}",
                        fixture.name
                    );
                }
            }
            ExpectedResolution::None => {
                assert_eq!(json["kind"], "none", "{}", fixture.name);
            }
        }
    }
}

#[test]
fn worktree_window_plan_rejects_window_without_session() {
    let argv = vec![
        "worktree-window".to_owned(),
        "--main-repo-name".to_owned(),
        "mawjs-oracle".to_owned(),
        "--wt-name".to_owned(),
        "1-feature".to_owned(),
        "--window".to_owned(),
        "1:feature:true".to_owned(),
    ];

    let output = run_cli(&argv);
    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("--window must follow a --session"),
        "{}",
        output.stderr
    );
}
