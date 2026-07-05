use maw_cli::run_cli;

#[test]
fn feed_parse_line_plan_cli_matches_maw_js_cases() {
    let parsed = run_cli(&[
        "feed".to_owned(),
        "parse-line".to_owned(),
        "2026-05-18 12:34:56 | alpha | m5 | PreToolUse | /repo | sess-1 » Bash: echo a | b"
            .to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(parsed.code, 0, "{}", parsed.stderr);
    let json: serde_json::Value = serde_json::from_str(&parsed.stdout).expect("valid json");
    assert_eq!(json["command"], "feed");
    assert_eq!(json["kind"], "parseLine");
    assert_eq!(json["parsed"], true);
    assert_eq!(json["event"]["timestamp"], "2026-05-18 12:34:56");
    assert_eq!(json["event"]["oracle"], "alpha");
    assert_eq!(json["event"]["host"], "m5");
    assert_eq!(json["event"]["event"], "PreToolUse");
    assert_eq!(json["event"]["project"], "/repo");
    assert_eq!(json["event"]["sessionId"], "sess-1");
    assert_eq!(json["event"]["message"], "Bash: echo a | b");
    assert!(json["event"]["ts"].as_i64().unwrap_or_default() > 0);

    let fallback = run_cli(&[
        "feed".to_owned(),
        "parse-line".to_owned(),
        "2026-05-18 12:34:56 | beta | white | Stop | /repo | sess-only".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(fallback.code, 0, "{}", fallback.stderr);
    let fallback_json: serde_json::Value =
        serde_json::from_str(&fallback.stdout).expect("valid fallback json");
    assert_eq!(fallback_json["event"]["sessionId"], "sess-only");
    assert_eq!(fallback_json["event"]["message"], "");

    for line in [
        "",
        "not a feed row",
        "2026-05-18 12:00:00 | oracle | host | Event",
        "not-a-date | oracle | host | Notification | project | session » message",
    ] {
        let output = run_cli(&[
            "feed".to_owned(),
            "parse-line".to_owned(),
            line.to_owned(),
            "--plan-json".to_owned(),
        ]);
        assert_eq!(output.code, 1, "{line}");
        let json: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid json");
        assert_eq!(json["parsed"], false, "{line}");
    }
}

#[test]
fn feed_active_plan_cli_matches_maw_js_cases() {
    let output = run_cli(&[
        "feed".to_owned(),
        "active".to_owned(),
        "--now".to_owned(),
        "10000".to_owned(),
        "--window".to_owned(),
        "1000".to_owned(),
        "--event".to_owned(),
        "old:0:stale".to_owned(),
        "--event".to_owned(),
        "alpha:9200:older".to_owned(),
        "--event".to_owned(),
        "beta:9500:beta".to_owned(),
        "--event".to_owned(),
        "alpha:9900:latest".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid json");
    assert_eq!(json["command"], "feed");
    assert_eq!(json["kind"], "active");
    let active = json["active"].as_array().expect("active array");
    let oracles: Vec<&str> = active
        .iter()
        .map(|event| event["oracle"].as_str().expect("oracle string"))
        .collect();
    assert_eq!(oracles, vec!["alpha", "beta"]);
    assert_eq!(active[0]["message"], "latest");
    assert_eq!(active[1]["message"], "beta");
}

#[test]
fn feed_describe_plan_cli_matches_maw_js_cases() {
    for (event, message, expected) in [
        (
            "PreToolUse",
            "Bash: run a command",
            "⚡ Bash: run a command",
        ),
        (
            "PreToolUse",
            "Unknown: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "🔧 Unknown: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx...",
        ),
        ("PreToolUse", "Read ✓", "📖 Read"),
        ("PostToolUse", "Bash ✓ 0", "✓ Bash done"),
        ("PostToolUseFailure", "Edit ✗ failed", "✗ Edit failed"),
        (
            "UserPromptSubmit",
            "uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuu",
            "💬 uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuu...",
        ),
        ("UserPromptSubmit", "", "💬 New prompt"),
        ("SubagentStart", "", "🤖 Subagent started"),
        ("SubagentStop", "", "🤖 Subagent done"),
        ("SessionStart", "", "🟢 Session started"),
        ("SessionEnd", "", "⏹ Session ended"),
        ("Stop", "", "⏹ Stopped"),
        ("Notification", "ping", "🔔 ping"),
        ("Notification", "", "🔔 Notification"),
        ("PluginError", "plugin blew up", "plugin blew up"),
        ("PluginLoad", "", "PluginLoad"),
    ] {
        let output = run_cli(&[
            "feed".to_owned(),
            "describe".to_owned(),
            event.to_owned(),
            "--message".to_owned(),
            message.to_owned(),
            "--plan-json".to_owned(),
        ]);
        assert_eq!(output.code, 0, "{event}/{message}: {}", output.stderr);
        let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|err| {
            panic!("{event}/{message} invalid json: {err}\n{}", output.stdout)
        });
        assert_eq!(json["description"], expected, "{event}/{message}");
    }
}

#[test]
fn feed_plan_rejects_bad_active_arguments() {
    let misplaced = run_cli(&[
        "feed".to_owned(),
        "--event".to_owned(),
        "alpha:1:hello".to_owned(),
    ]);
    assert_eq!(misplaced.code, 2);
    assert!(misplaced.stderr.contains("--event requires active"));

    let bad_ts = run_cli(&[
        "feed".to_owned(),
        "active".to_owned(),
        "--event".to_owned(),
        "alpha:soon:hello".to_owned(),
    ]);
    assert_eq!(bad_ts.code, 2);
    assert!(bad_ts.stderr.contains("ts must be an integer"));
}
