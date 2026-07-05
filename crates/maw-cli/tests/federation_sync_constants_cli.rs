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
fn federation_sync_constants_reports_diff_and_flag_vocabulary() {
    let output = run(&["federation-sync", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"federation-sync\""));
    assert!(output.stdout.contains("\"action\":\"constants\""));
    assert!(output
        .stdout
        .contains("\"diffBuckets\":[\"add\",\"stale\",\"conflict\",\"unreachable\"]"));
    assert!(output
        .stdout
        .contains("\"flags\":[\"dry-run\",\"check\",\"force\",\"prune\"]"));
    assert!(output
        .stdout
        .contains("\"identityReachability\":[\"reachable\",\"unreachable\"]"));
    assert!(output
        .stdout
        .contains("\"checkExitCodes\":{\"clean\":0,\"dirty\":1}"));
}

#[test]
fn federation_sync_constants_rejects_unknown_arguments() {
    let output = run(&["federation-sync", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("federation-sync constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs federation-sync constants"));
}
