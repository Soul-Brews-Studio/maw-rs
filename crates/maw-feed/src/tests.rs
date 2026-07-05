use super::*;

#[test]
fn timestamp_parser_rejects_invalid_months_and_non_leap_days() {
    assert_eq!(parse_timestamp_ms("2026-13-01 00:00:00"), None);
    assert_eq!(parse_timestamp_ms("2026-02-29 00:00:00"), None);
    assert!(parse_timestamp_ms("2024-02-29 00:00:00").is_some());
}

#[test]
fn days_in_month_rejects_zero_month() {
    assert_eq!(days_in_month(2026, 0), None);
}

#[test]
fn remaining_timestamp_and_tool_icon_edges_are_covered() {
    for tool in [
        ("Edit", "✏️"),
        ("Write", "📝"),
        ("Grep", "🔍"),
        ("Glob", "📂"),
        ("Agent", "🤖"),
        ("WebFetch", "🌐"),
        ("WebSearch", "🔎"),
    ] {
        let mut event = FeedEvent {
            timestamp: "2026-05-21 00:00:00".to_owned(),
            oracle: "pulse".to_owned(),
            host: "white".to_owned(),
            event: "PreToolUse".to_owned(),
            project: "maw".to_owned(),
            session_id: "s".to_owned(),
            message: format!("{}: detail", tool.0),
            ts: 1,
        };
        assert_eq!(
            describe_activity(&event),
            format!("{} {}: detail", tool.1, tool.0)
        );
        event.message = tool.0.to_owned();
        assert_eq!(describe_activity(&event), format!("{} {}", tool.1, tool.0));
    }

    for timestamp in [
        "2026 00:00:00",
        "2026-05 00:00:00",
        "2026-05-aa 00:00:00",
        "2026-05-21 00",
        "2026-05-21 00:aa:00",
        "2026-05-21 00:00:aa",
        "2026-04-31 00:00:00",
    ] {
        assert_eq!(parse_timestamp_ms(timestamp), None, "{timestamp}");
    }

    assert!(days_from_civil(-1, 1, 1).is_some());
}

#[test]
fn activity_descriptions_cover_empty_and_unknown_messages() {
    let event = FeedEvent {
        timestamp: "2026-05-21 00:00:00".to_owned(),
        oracle: "pulse".to_owned(),
        host: "white".to_owned(),
        event: "PostToolUse".to_owned(),
        project: "maw".to_owned(),
        session_id: "s".to_owned(),
        message: "  ".to_owned(),
        ts: 1,
    };
    assert_eq!(describe_activity(&event), "✓ Tool done");

    let mut unknown = event.clone();
    unknown.event = "CustomEvent".to_owned();
    unknown.message.clear();
    assert_eq!(describe_activity(&unknown), "CustomEvent");
}
