use maw_cli::{run_cli, CliOutput};

#[test]
fn worktree_window_constants_plan_json_locks_maw_js_matching_vocabulary() {
    let output = run_cli(&[
        "worktree-window".to_owned(),
        "constants".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(
        output,
        CliOutput {
            code: 0,
            stdout: concat!(
                "{\"command\":\"worktree-window\",\"kind\":\"constants\",",
                "\"inputs\":[\"main-repo-name\",\"wt-name\",\"session\",\"window\"],",
                "\"windowShape\":\"index:name:active\",",
                "\"resultKinds\":[\"bound\",\"ambiguous\",\"none\"],",
                "\"parentSessionRules\":[\"strip -oracle suffix from main repo\",\"match fleet numeric session suffix\",\"prefer parent-scoped windows before global fallback\"],",
                "\"queryRules\":[\"strip numeric worktree prefix\",\"try repo-qualified worktree name before stripped suffix\",\"dedupe same-named windows across sessions\",\"fallback to global single match\",\"fail loud on ambiguous stripped suffix\"],",
                "\"usageErrors\":[\"missing-main-repo-name\",\"missing-wt-name\",\"window-without-session\",\"bad-window-shape\",\"unknown-argument\"],",
                "\"fixtureCounts\":{\"total\":8,\"bound\":6,\"ambiguous\":1,\"none\":1}}
"
            )
            .to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn worktree_window_constants_plan_rejects_unknown_flags() {
    let output = run_cli(&[
        "worktree-window".to_owned(),
        "constants".to_owned(),
        "--bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output
        .stderr
        .contains("worktree-window constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs worktree-window constants"));
}
