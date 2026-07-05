use maw_cli::run_cli;

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

#[test]
fn consent_constants_plan_reports_actions_and_statuses() {
    let output = run(&["consent-constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"consent-constants\""));
    assert!(output
        .stdout
        .contains("\"actions\":[\"hey\",\"team-invite\",\"plugin-install\"]"));
    assert!(output
        .stdout
        .contains("\"statuses\":[\"pending\",\"approved\",\"rejected\",\"expired\"]"));
    assert!(output
        .stdout
        .contains("\"approvedBy\":[\"human\",\"auto\"]"));
}

#[test]
fn consent_constants_plan_rejects_unknown_arguments() {
    let output = run(&["consent-constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("consent-constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs consent-constants"));
}
