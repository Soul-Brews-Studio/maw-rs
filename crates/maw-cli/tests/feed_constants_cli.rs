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
fn feed_constants_plan_locks_parser_activity_and_active_vocabulary() {
    let value = json(&["feed", "constants", "--plan-json"]);

    assert_eq!(value["command"], "feed");
    assert_eq!(value["action"], "constants");
    assert_eq!(
        value["actions"],
        serde_json::json!(["parse-line", "describe", "active"])
    );
    assert_eq!(
        value["eventFields"],
        serde_json::json!([
            "timestamp",
            "oracle",
            "host",
            "event",
            "project",
            "sessionId",
            "message",
            "ts"
        ])
    );
    assert_eq!(value["rowSeparator"], " | ");
    assert_eq!(value["messageDelimiter"], " » ");
    assert_eq!(value["timestampFormat"], "YYYY-MM-DD HH:mm:ss");
    assert_eq!(value["activeCutoff"], "ts>=now-window");
    assert_eq!(value["activeOrdering"], "oracle asc, latest ts per oracle");
    assert_eq!(value["descriptionTruncate"]["maxChars"], 60);
    assert_eq!(value["descriptionTruncate"]["prefixChars"], 57);
    assert_eq!(
        value["activityEvents"],
        serde_json::json!([
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "UserPromptSubmit",
            "SubagentStart",
            "SubagentStop",
            "SessionStart",
            "SessionEnd",
            "Stop",
            "Notification"
        ])
    );
    assert_eq!(value["toolIcons"]["Bash"], "⚡");
    assert_eq!(value["toolIcons"]["Read"], "📖");
    assert_eq!(value["toolIcons"]["default"], "🔧");
}

#[test]
fn feed_constants_rejects_unknown_flags() {
    let output = run(&["feed", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("feed constants: unknown arg --bad"));
    assert!(output.stderr.contains("maw-rs feed constants"));
}
