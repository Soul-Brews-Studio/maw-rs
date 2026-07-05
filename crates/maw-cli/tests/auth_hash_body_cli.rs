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
fn auth_hash_body_plan_matches_maw_js_empty_and_body_contract() {
    let absent = run(&["auth", "hash-body", "--plan-json"]);
    assert_eq!(absent.code, 0, "stderr: {}", absent.stderr);
    assert_eq!(absent.stderr, "");
    assert!(absent.stdout.contains("\"command\":\"auth\""));
    assert!(absent.stdout.contains("\"kind\":\"hash-body\""));
    assert!(absent.stdout.contains("\"present\":false"));
    assert!(absent.stdout.contains("\"bodyHash\":\"\""));

    let empty = run(&["auth", "hash-body", "--body", "", "--plan-json"]);
    assert_eq!(empty.code, 0, "stderr: {}", empty.stderr);
    assert!(empty.stdout.contains("\"present\":true"));
    assert!(empty.stdout.contains("\"bodyHash\":\"\""));

    let body = run(&["auth", "hash-body", "--body", "body", "--plan-json"]);
    assert_eq!(body.code, 0, "stderr: {}", body.stderr);
    assert!(body.stdout.contains("\"present\":true"));
    assert!(body.stdout.contains(
        "\"bodyHash\":\"230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5\""
    ));
}

#[test]
fn auth_hash_body_plan_rejects_unknown_arguments() {
    let output = run(&["auth", "hash-body", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("auth hash-body: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs auth hash-body"));
}
