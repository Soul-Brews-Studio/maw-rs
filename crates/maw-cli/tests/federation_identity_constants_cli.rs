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
fn federation_identity_constants_reports_identity_contract() {
    let output = run(&["federation-identity", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output
        .stdout
        .contains("\"command\":\"federation-identity\""));
    assert!(output.stdout.contains("\"action\":\"constants\""));
    assert!(output.stdout.contains("\"defaultNode\":\"local\""));
    assert!(output.stdout.contains("\"defaultUrl\":\"\""));
    assert!(output.stdout.contains("\"agentShape\":\"oracle=node\""));
    assert!(output
        .stdout
        .contains("\"hostedRule\":\"route-node-equals-local-node\""));
    assert!(output
        .stdout
        .contains("\"routesShape\":\"oracle-to-node-map\""));
}

#[test]
fn federation_identity_constants_rejects_unknown_arguments() {
    let output = run(&["federation-identity", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("federation-identity constants: unknown argument --bad"));
    assert!(output
        .stderr
        .contains("maw-rs federation-identity constants"));
}
