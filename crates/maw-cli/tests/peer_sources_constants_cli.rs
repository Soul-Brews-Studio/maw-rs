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
fn peer_sources_constants_reports_resolution_vocabulary() {
    let output = run(&["peer-sources", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"peer-sources\""));
    assert!(output.stdout.contains("\"action\":\"constants\""));
    assert!(output
        .stdout
        .contains("\"modes\":[\"config\",\"scout\",\"both\"]"));
    assert!(output
        .stdout
        .contains("\"configShapes\":[\"peer-url\",\"named-peer\"]"));
    assert!(output
        .stdout
        .contains("\"discoveryStates\":[\"ok\",\"error\",\"hint\"]"));
    assert!(output
        .stdout
        .contains("\"discoveredShape\":\"node|host|oracle|locator[,locator]\""));
}

#[test]
fn peer_sources_constants_rejects_unknown_arguments() {
    let output = run(&["peer-sources", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("peer-sources constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs peer-sources constants"));
}
