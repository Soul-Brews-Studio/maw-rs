use maw_cli::run_cli;
use serde_json::Value;

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

fn json(args: &[&str]) -> Value {
    let output = run(args);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap()
}

#[test]
fn bind_host_constants_plan_locks_heuristic_inputs_and_reasons() {
    let value = json(&["bind-host", "constants", "--plan-json"]);

    assert_eq!(value["command"], "bind-host");
    assert_eq!(value["action"], "constants");
    assert_eq!(value["hosts"]["loopback"], "127.0.0.1");
    assert_eq!(value["hosts"]["remote"], "0.0.0.0");
    assert_eq!(
        value["inputFlags"],
        serde_json::json!([
            "config-peers-len",
            "config-named-peers-len",
            "maw-host",
            "peers-store-len",
            "peers-store-error"
        ])
    );
    assert_eq!(
        value["remoteReasons"],
        serde_json::json!([
            "config.peers",
            "config.namedPeers",
            "MAW_HOST",
            "peers.json"
        ])
    );
    assert_eq!(value["remoteMawHostValue"], "0.0.0.0");
    assert_eq!(
        value["priority"],
        serde_json::json!([
            "config.peers",
            "config.namedPeers",
            "MAW_HOST",
            "peers.json"
        ])
    );
}

#[test]
fn bind_host_constants_rejects_unknown_flags() {
    let output = run(&["bind-host", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("bind-host constants: unknown arg --bad"));
    assert!(output.stderr.contains("maw-rs bind-host constants"));
}
