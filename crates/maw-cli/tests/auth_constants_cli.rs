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
fn auth_constants_plan_reports_federation_defaults() {
    let output = run(&["auth", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"auth\""));
    assert!(output.stdout.contains("\"kind\":\"constants\""));
    assert!(output.stdout.contains("\"defaultOracle\":\"mawjs\""));
    assert!(output.stdout.contains("\"windowSec\":300"));
}

#[test]
fn auth_constants_plan_rejects_unknown_arguments() {
    let output = run(&["auth", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs auth constants"));
}
