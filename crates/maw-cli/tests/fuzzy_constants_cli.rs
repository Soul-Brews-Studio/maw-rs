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
fn fuzzy_constants_plan_locks_matching_defaults_and_ordering() {
    let value = json(&["fuzzy", "constants", "--plan-json"]);

    assert_eq!(value["command"], "fuzzy");
    assert_eq!(value["action"], "constants");
    assert_eq!(value["actions"], serde_json::json!(["distance", "match"]));
    assert_eq!(value["algorithm"], "levenshtein");
    assert_eq!(value["distanceUnit"], "utf16-code-unit");
    assert_eq!(
        value["caseHandling"],
        "case-insensitive scoring, original output preserved"
    );
    assert_eq!(value["dedupe"], "exact candidate string before scoring");
    assert_eq!(value["defaultMaxResults"], 3);
    assert_eq!(value["defaultMaxDistance"], 3);
    assert_eq!(value["emptyInput"], "no matches");
    assert_eq!(value["zeroMaxResults"], "no matches");
    assert_eq!(value["emptyCandidate"], "ignored");
    assert_eq!(
        value["sortOrder"],
        serde_json::json!(["distance asc", "candidate lexicographic asc"])
    );
    assert_eq!(
        value["limitFlags"],
        serde_json::json!(["max-results", "max-distance"])
    );
}

#[test]
fn fuzzy_constants_rejects_unknown_flags() {
    let output = run(&["fuzzy", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("fuzzy constants: unknown arg --bad"));
    assert!(output.stderr.contains("maw-rs fuzzy constants"));
}
