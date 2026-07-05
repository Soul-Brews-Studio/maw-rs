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
fn recent_hello_plan_reports_fresh_stale_and_missing_zids() {
    let fresh = run(&[
        "recent-hello",
        "--hello",
        "z-fresh:1000",
        "--hello",
        "z-stale:1000",
        "--zid",
        "z-fresh",
        "--now",
        "61000",
        "--plan-json",
    ]);

    assert_eq!(fresh.code, 0, "stderr: {}", fresh.stderr);
    assert_eq!(fresh.stderr, "");
    assert!(fresh.stdout.contains("\"command\":\"recent-hello\""));
    assert!(fresh.stdout.contains("\"zid\":\"z-fresh\""));
    assert!(fresh.stdout.contains("\"recent\":true"));
    assert!(fresh.stdout.contains("\"windowMs\":60000"));

    let stale = run(&[
        "recent-hello",
        "--hello",
        "z-stale:1000",
        "--zid",
        "z-stale",
        "--now",
        "61001",
        "--plan-json",
    ]);

    assert_eq!(stale.code, 0, "stderr: {}", stale.stderr);
    assert!(stale.stdout.contains("\"recent\":false"));

    let missing = run(&[
        "recent-hello",
        "--zid",
        "missing",
        "--now",
        "1",
        "--plan-json",
    ]);
    assert_eq!(missing.code, 0, "stderr: {}", missing.stderr);
    assert!(missing.stdout.contains("\"recent\":false"));
}

#[test]
fn recent_hello_plan_rejects_missing_and_malformed_inputs() {
    let missing_now = run(&["recent-hello", "--zid", "zid"]);
    assert_eq!(missing_now.code, 2);
    assert!(missing_now
        .stderr
        .contains("recent-hello: missing --now value"));
    assert!(missing_now.stderr.contains("usage: maw-rs recent-hello"));

    let bad_hello = run(&[
        "recent-hello",
        "--hello",
        "zid:not-ms",
        "--zid",
        "zid",
        "--now",
        "1",
    ]);
    assert_eq!(bad_hello.code, 2);
    assert!(bad_hello
        .stderr
        .contains("recent-hello: invalid hello timestamp"));
}
