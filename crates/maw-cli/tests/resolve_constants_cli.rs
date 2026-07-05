use maw_cli::{run_cli, CliOutput};

#[test]
fn resolve_constants_plan_json_locks_maw_js_resolver_vocabulary() {
    let output = run_cli(&[
        "resolve".to_owned(),
        "constants".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(
        output,
        CliOutput {
            code: 0,
            stdout: concat!(
                "{\"command\":\"resolve\",\"kind\":\"constants\",",
                "\"modes\":[\"by-name\",\"session\",\"worktree\"],\"modeAliases\":{\"byName\":\"by-name\"},",
                "\"resultKinds\":[\"exact\",\"fuzzy\",\"ambiguous\",\"none\"],",
                "\"matchLadder\":[\"trim-lowercase-target\",\"case-insensitive-exact\",\"suffix-segment\",\"prefix-or-middle-segment\",\"substring-hints-only\"],",
                "\"modeRules\":{\"session\":{\"fleetSessions\":true,\"numericPrefixBlocksPrefixMiddle\":true},\"worktree\":{\"fleetSessions\":false,\"numericPrefixesAreSequenceCounters\":true},\"by-name\":{\"fleetSessions\":false}},",
                "\"noneBehavior\":{\"emptyTarget\":\"none-no-hints\",\"substringFallback\":\"none-with-hints-never-fuzzy\"},",
                "\"fixtureCounts\":{\"total\":16,\"byName\":12,\"session\":3,\"worktree\":1,\"exact\":2,\"fuzzy\":7,\"ambiguous\":3,\"none\":4}}\n"
            )
            .to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn resolve_constants_plan_rejects_unknown_flags() {
    let output = run_cli(&[
        "resolve".to_owned(),
        "constants".to_owned(),
        "--bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output
        .stderr
        .contains("resolve constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs resolve constants"));
}
