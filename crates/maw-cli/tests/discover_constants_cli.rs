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
fn discover_constants_reports_inventory_vocabulary() {
    let output = run(&["discover", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"discover\""));
    assert!(output.stdout.contains("\"action\":\"constants\""));
    assert!(output
        .stdout
        .contains("\"peerSources\":[\"config\",\"scout\",\"both\"]"));
    assert!(output
        .stdout
        .contains("\"views\":[\"json\",\"tree\",\"awake\"]"));
    assert!(output.stdout.contains("\"inventorySources\":[\"fleet-config\",\"oracle-manifest\",\"plugin-registry\",\"ghq\",\"tmux\"]"));
    assert!(output
        .stdout
        .contains("\"paneShape\":\"id|command|target|title|pid|cwd|last_activity\""));
}

#[test]
fn discover_constants_rejects_unknown_arguments() {
    let output = run(&["discover", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("discover constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs discover constants"));
}
