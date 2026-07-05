use maw_cli::{run_cli, CliOutput};

#[test]
fn normalize_constants_plan_json_locks_maw_js_normalization_vocabulary() {
    let output = run_cli(&[
        "normalize".to_owned(),
        "constants".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(
        output,
        CliOutput {
            code: 0,
            stdout: concat!(
                "{\"command\":\"normalize\",\"kind\":\"constants\",",
                "\"steps\":[\"trim\",\"strip-trailing-slashes\",\"strip-trailing-dot-git-until-stable\"],",
                "\"preserves\":[\"interior characters\",\"case\",\"suffix text named .git without slash-dot\"],",
                "\"emptyBehavior\":{\"empty\":\"empty\",\"whitespaceOnly\":\"empty\"},",
                "\"fixtureCount\":12}\n"
            )
            .to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn normalize_constants_plan_rejects_unknown_flags() {
    let output = run_cli(&[
        "normalize".to_owned(),
        "constants".to_owned(),
        "--bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output
        .stderr
        .contains("normalize constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs normalize constants"));
}
