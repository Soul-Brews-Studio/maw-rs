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
fn peer_probe_handshake_constants_reports_valid_and_invalid_shapes() {
    let output = run(&["peer-probe", "handshake-constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"peer-probe\""));
    assert!(output.stdout.contains("\"action\":\"handshake-constants\""));
    assert!(output
        .stdout
        .contains("\"validShapes\":[\"legacy-true\",\"schema-object-non-empty\"]"));
    assert!(output.stdout.contains(
        "\"invalidShapes\":[\"empty-object\",\"other-truthy\",\"missing\",\"schema-object-empty\"]"
    ));
}

#[test]
fn peer_probe_handshake_constants_rejects_unknown_arguments() {
    let output = run(&["peer-probe", "handshake-constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("peer-probe handshake-constants: unknown argument --bad"));
    assert!(output
        .stderr
        .contains("maw-rs peer-probe handshake-constants"));
}
