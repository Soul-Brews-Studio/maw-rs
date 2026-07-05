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
fn pair_code_constants_plan_reports_shape_invariants() {
    let output = run(&["pair-code", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"pair-code\""));
    assert!(output.stdout.contains("\"kind\":\"constants\""));
    assert!(output
        .stdout
        .contains("\"alphabet\":\"ABCDEFGHJKLMNPQRSTUVWXYZ23456789\""));
    assert!(output.stdout.contains("\"codeLength\":6"));
    assert!(output.stdout.contains("\"prettyGroupSize\":3"));
    assert!(output.stdout.contains("\"separator\":\"-\""));
}

#[test]
fn pair_code_constants_rejects_unknown_arguments() {
    let output = run(&["pair-code", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("pair-code constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs pair-code constants"));
}
