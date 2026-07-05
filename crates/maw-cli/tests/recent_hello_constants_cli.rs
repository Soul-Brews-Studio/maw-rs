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
fn recent_hello_constants_reports_freshness_window() {
    let output = run(&["recent-hello", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"recent-hello\""));
    assert!(output.stdout.contains("\"kind\":\"constants\""));
    assert!(output.stdout.contains("\"windowMs\":60000"));
    assert!(output
        .stdout
        .contains("\"threshold\":\"now-minus-seen-at <= windowMs\""));
}

#[test]
fn recent_hello_constants_rejects_unknown_arguments() {
    let output = run(&["recent-hello", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("recent-hello constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs recent-hello constants"));
}
