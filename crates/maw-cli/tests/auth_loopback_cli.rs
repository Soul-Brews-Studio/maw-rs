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
fn auth_loopback_plan_classifies_maw_js_loopback_addresses() {
    let local = run(&["auth", "loopback", "--address", "127.9.0.1", "--plan-json"]);
    assert_eq!(local.code, 0, "stderr: {}", local.stderr);
    assert_eq!(local.stderr, "");
    assert!(local.stdout.contains("\"command\":\"auth\""));
    assert!(local.stdout.contains("\"kind\":\"loopback\""));
    assert!(local.stdout.contains("\"address\":\"127.9.0.1\""));
    assert!(local.stdout.contains("\"loopback\":true"));

    let localhost = run(&["auth", "loopback", "--address", "localhost", "--plan-json"]);
    assert_eq!(localhost.code, 0, "stderr: {}", localhost.stderr);
    assert!(localhost.stdout.contains("\"loopback\":true"));

    let remote = run(&["auth", "loopback", "--address", "10.0.0.2", "--plan-json"]);
    assert_eq!(remote.code, 0, "stderr: {}", remote.stderr);
    assert!(remote.stdout.contains("\"loopback\":false"));
}

#[test]
fn auth_loopback_plan_rejects_missing_address() {
    let output = run(&["auth", "loopback"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth loopback: --address is required"));
    assert!(output.stderr.contains("maw-rs auth loopback"));
}
