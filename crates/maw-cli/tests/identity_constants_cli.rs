use maw_cli::{run_cli, CliOutput};

#[test]
fn identity_constants_plan_json_locks_maw_js_identity_vocabulary() {
    let output = run_cli(&[
        "identity".to_owned(),
        "constants".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(
        output,
        CliOutput {
            code: 0,
            stdout: concat!(
                "{\"command\":\"identity\",\"kind\":\"constants\",",
                "\"actions\":[\"session-name\",\"node-identity\"],",
                "\"sessionName\":{\"suffixRemoved\":\"-oracle\",\"gitSuffixRemoved\":\".git\",\"slotRange\":[0,99],\"slotPadding\":2,\"maxStemChars\":50,\"sanitization\":[\"lowercase\",\"whitespace-to-dash\",\"ascii-alnum-dot-underscore-dash-only\",\"collapse-dot-runs\",\"trim-leading-dash-dot\",\"trim-trailing-dash-dot-run\",\"strip-leading-numeric-fleet-slot\"]},",
                "\"nodeIdentity\":{\"fallbackHost\":\"local\",\"separator\":\"@\",\"preserveAlreadyCanonical\":true,\"omitUserWhenSameAsHost\":true,\"trimInputs\":true},",
                "\"validation\":{\"reservedOracleSuffixes\":[\"-view\"]},",
                "\"fixtureCounts\":{\"canonicalSessionName\":5,\"canonicalNodeIdentity\":5}}\n"
            )
            .to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn identity_constants_plan_rejects_unknown_flags() {
    let output = run_cli(&[
        "identity".to_owned(),
        "constants".to_owned(),
        "--bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output
        .stderr
        .contains("identity constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs identity constants"));
}
