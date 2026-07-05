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
fn federation_health_constants_reports_classifier_vocabulary() {
    let output = run(&["federation-health", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"federation-health\""));
    assert!(output.stdout.contains("\"action\":\"constants\""));
    assert!(output
        .stdout
        .contains("\"pairHealth\":[\"healthy\",\"half-up\",\"down\",\"unknown\"]"));
    assert!(output
        .stdout
        .contains("\"peerReachability\":[\"reachable\",\"unreachable\"]"));
    assert!(output
        .stdout
        .contains("\"remoteKinds\":[\"missing-peers\",\"http\",\"fetch-error\",\"peer\"]"));
    assert!(output.stdout.contains("\"clockFlags\":[\"ok\",\"clock\"]"));
}

#[test]
fn federation_health_constants_rejects_unknown_arguments() {
    let output = run(&["federation-health", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("federation-health constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs federation-health constants"));
}
