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
fn pair_code_store_plan_registers_and_looks_up_live_expired_consumed_and_missing_codes() {
    let live = run(&[
        "pair-code-store",
        "lookup",
        "--seed-code",
        "abc123:60000:1000",
        "--code",
        "ABC-123",
        "--now",
        "61000",
        "--plan-json",
    ]);

    assert_eq!(live.code, 0, "stderr: {}", live.stderr);
    assert_eq!(live.stderr, "");
    assert!(live.stdout.contains("\"command\":\"pair-code-store\""));
    assert!(live.stdout.contains("\"mode\":\"lookup\""));
    assert!(live.stdout.contains("\"normalized\":\"ABC123\""));
    assert!(live.stdout.contains("\"state\":\"live\""));
    assert!(live.stdout.contains("\"entry\":{"));
    assert!(live.stdout.contains("\"expiresAt\":61000"));
    assert!(!live.stdout.contains("abc123"));

    let expired = run(&[
        "pair-code-store",
        "lookup",
        "--seed-code",
        "ABC123:60000:1000",
        "--code",
        "ABC123",
        "--now",
        "61001",
        "--plan-json",
    ]);
    assert_eq!(expired.code, 0, "stderr: {}", expired.stderr);
    assert!(expired.stdout.contains("\"state\":\"expired\""));
    assert!(expired.stdout.contains("\"entry\":null"));

    let consumed = run(&[
        "pair-code-store",
        "consume",
        "--seed-code",
        "ABC123:60000:1000",
        "--code",
        "ABC123",
        "--now",
        "61000",
        "--plan-json",
    ]);
    assert_eq!(consumed.code, 0, "stderr: {}", consumed.stderr);
    assert!(consumed.stdout.contains("\"mode\":\"consume\""));
    assert!(consumed.stdout.contains("\"state\":\"live\""));
    assert!(consumed.stdout.contains("\"consumed\":true"));

    let missing = run(&[
        "pair-code-store",
        "lookup",
        "--code",
        "ZZZ999",
        "--now",
        "1",
        "--plan-json",
    ]);
    assert_eq!(missing.code, 0, "stderr: {}", missing.stderr);
    assert!(missing.stdout.contains("\"state\":\"not-found\""));
}

#[test]
fn pair_code_store_plan_register_mode_and_usage_errors_are_explicit() {
    let registered = run(&[
        "pair-code-store",
        "register",
        "--code",
        "abc123",
        "--ttl-ms",
        "2500",
        "--now",
        "7",
        "--plan-json",
    ]);
    assert_eq!(registered.code, 0, "stderr: {}", registered.stderr);
    assert!(registered.stdout.contains("\"mode\":\"register\""));
    assert!(registered.stdout.contains("\"state\":\"live\""));
    assert!(registered.stdout.contains("\"expiresAt\":2507"));

    let missing_now = run(&["pair-code-store", "lookup", "--code", "ABC123"]);
    assert_eq!(missing_now.code, 2);
    assert!(missing_now
        .stderr
        .contains("pair-code-store: missing --now value"));
    assert!(missing_now.stderr.contains("usage: maw-rs pair-code-store"));

    let bad_seed = run(&[
        "pair-code-store",
        "lookup",
        "--seed-code",
        "ABC123:not-ms:0",
        "--code",
        "ABC123",
        "--now",
        "1",
    ]);
    assert_eq!(bad_seed.code, 2);
    assert!(bad_seed
        .stderr
        .contains("pair-code-store: --seed-code ttl_ms"));
}
