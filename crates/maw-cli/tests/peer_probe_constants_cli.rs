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
fn peer_probe_constants_plan_reports_codes_and_exit_codes() {
    let output = run(&["peer-probe", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"peer-probe\""));
    assert!(output.stdout.contains("\"action\":\"constants\""));
    assert!(output.stdout.contains("\"codes\":[\"DNS\",\"REFUSED\",\"TIMEOUT\",\"HTTP_4XX\",\"HTTP_5XX\",\"TLS\",\"BAD_BODY\",\"UNKNOWN\"]"));
    assert!(output.stdout.contains("\"exitCodes\":{\"DNS\":3,\"REFUSED\":4,\"TIMEOUT\":5,\"HTTP_4XX\":6,\"HTTP_5XX\":6,\"TLS\":2,\"BAD_BODY\":2,\"UNKNOWN\":2}"));
}

#[test]
fn peer_probe_constants_rejects_unknown_arguments() {
    let output = run(&["peer-probe", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("peer-probe constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs peer-probe constants"));
}
