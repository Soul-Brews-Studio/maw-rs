//! Oracle feed parser and activity helpers ported from maw-js `src/lib/feed.ts`.

use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedEvent {
    pub timestamp: String,
    pub oracle: String,
    pub host: String,
    pub event: String,
    pub project: String,
    pub session_id: String,
    pub message: String,
    pub ts: i64,
}

#[must_use]
pub fn parse_line(line: &str) -> Option<FeedEvent> {
    if line.is_empty() || !line.contains(" | ") {
        return None;
    }
    let parts: Vec<&str> = line.split(" | ").map(str::trim).collect();
    if parts.len() < 5 {
        return None;
    }

    let timestamp = parts[0];
    let ts = parse_timestamp_ms(timestamp)?;
    let rest = parts.get(5..).unwrap_or_default().join(" | ");
    let (session_id, message) = match rest.find(" » ") {
        Some(idx) => (
            rest[..idx].trim().to_owned(),
            rest[idx + " » ".len()..].trim().to_owned(),
        ),
        None => (rest.trim().to_owned(), String::new()),
    };

    Some(FeedEvent {
        timestamp: timestamp.to_owned(),
        oracle: parts[1].to_owned(),
        host: parts[2].to_owned(),
        event: parts[3].to_owned(),
        project: parts[4].to_owned(),
        session_id,
        message,
        ts,
    })
}

#[must_use]
pub fn active_oracles(events: &[FeedEvent], window_ms: i64) -> BTreeMap<String, FeedEvent> {
    active_oracles_at(events, now_ms(), window_ms)
}

#[must_use]
pub fn active_oracles_at(
    events: &[FeedEvent],
    now_ms: i64,
    window_ms: i64,
) -> BTreeMap<String, FeedEvent> {
    let cutoff = now_ms - window_ms;
    let mut map: BTreeMap<String, FeedEvent> = BTreeMap::new();
    for event in events {
        if event.ts < cutoff {
            continue;
        }
        let should_replace = map
            .get(&event.oracle)
            .is_none_or(|previous| event.ts > previous.ts);
        if should_replace {
            map.insert(event.oracle.clone(), event.clone());
        }
    }
    map
}

#[must_use]
pub fn describe_activity(event: &FeedEvent) -> String {
    match event.event.as_str() {
        "PreToolUse" => {
            let colon_idx = event.message.find(':');
            let tool = colon_idx.map_or_else(
                || event.message.split(' ').next().unwrap_or_default().trim(),
                |idx| event.message[..idx].trim(),
            );
            let icon = tool_icon(tool);
            let detail = colon_idx.map_or("", |idx| event.message[idx + 1..].trim());
            let short = truncate_60(detail);
            if short.is_empty() {
                format!("{icon} {tool}")
            } else {
                format!("{icon} {tool}: {short}")
            }
        }
        "PostToolUse" | "PostToolUseFailure" => {
            let tool = strip_tool_status(&event.message);
            if event.event == "PostToolUse" {
                format!("✓ {tool} done")
            } else {
                format!("✗ {tool} failed")
            }
        }
        "UserPromptSubmit" => {
            let short = truncate_60(&event.message);
            format!(
                "💬 {}",
                if short.is_empty() {
                    "New prompt"
                } else {
                    &short
                }
            )
        }
        "SubagentStart" => "🤖 Subagent started".to_owned(),
        "SubagentStop" => "🤖 Subagent done".to_owned(),
        "SessionStart" => "🟢 Session started".to_owned(),
        "SessionEnd" => "⏹ Session ended".to_owned(),
        "Stop" => {
            let short = truncate_60(&event.message);
            format!("⏹ {}", if short.is_empty() { "Stopped" } else { &short })
        }
        "Notification" => format!(
            "🔔 {}",
            if event.message.is_empty() {
                "Notification"
            } else {
                &event.message
            }
        ),
        _ => {
            if event.message.is_empty() {
                event.event.clone()
            } else {
                event.message.clone()
            }
        }
    }
}

fn tool_icon(tool: &str) -> &'static str {
    match tool {
        "Bash" => "⚡",
        "Read" => "📖",
        "Edit" => "✏️",
        "Write" => "📝",
        "Grep" => "🔍",
        "Glob" => "📂",
        "Agent" => "🤖",
        "WebFetch" => "🌐",
        "WebSearch" => "🔎",
        _ => "🔧",
    }
}

fn truncate_60(value: &str) -> String {
    if value.chars().count() > 60 {
        let mut short: String = value.chars().take(57).collect();
        short.push_str("...");
        short
    } else {
        value.to_owned()
    }
}

fn strip_tool_status(message: &str) -> String {
    let mut tool = message.trim();
    for marker in [" ✓", " ✗"] {
        if let Some(idx) = tool.find(marker) {
            tool = tool[..idx].trim();
            break;
        }
    }
    if tool.is_empty() {
        "Tool".to_owned()
    } else {
        tool.to_owned()
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

fn parse_timestamp_ms(timestamp: &str) -> Option<i64> {
    let (date, time) = timestamp.split_once(' ')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second = time_parts.next()?.parse::<u32>().ok()?;
    if time_parts.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some(
        (days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
            * 1_000,
    )
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let max_day = days_in_month(year, month)?;
    if day > max_day {
        return None;
    }
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if is_leap_year(year) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests;
