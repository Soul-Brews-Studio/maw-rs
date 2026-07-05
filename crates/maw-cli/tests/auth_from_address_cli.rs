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
fn auth_from_address_plan_resolves_default_and_explicit_oracle() {
    let default_oracle = run(&["auth", "from-address", "--node", "m5", "--plan-json"]);
    assert_eq!(default_oracle.code, 0, "stderr: {}", default_oracle.stderr);
    assert_eq!(default_oracle.stderr, "");
    assert!(default_oracle.stdout.contains("\"command\":\"auth\""));
    assert!(default_oracle.stdout.contains("\"kind\":\"from-address\""));
    assert!(default_oracle.stdout.contains("\"oracle\":null"));
    assert!(default_oracle.stdout.contains("\"node\":\"m5\""));
    assert!(default_oracle.stdout.contains("\"from\":\"mawjs:m5\""));

    let explicit_oracle = run(&[
        "auth",
        "from-address",
        "--oracle",
        "pulse",
        "--node",
        "white",
        "--plan-json",
    ]);
    assert_eq!(
        explicit_oracle.code, 0,
        "stderr: {}",
        explicit_oracle.stderr
    );
    assert!(explicit_oracle.stdout.contains("\"oracle\":\"pulse\""));
    assert!(explicit_oracle.stdout.contains("\"node\":\"white\""));
    assert!(explicit_oracle.stdout.contains("\"from\":\"pulse:white\""));
}

#[test]
fn auth_from_address_plan_rejects_missing_node() {
    let output = run(&["auth", "from-address", "--oracle", "pulse"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth from-address: --node is required"));
    assert!(output.stderr.contains("maw-rs auth from-address"));
}
