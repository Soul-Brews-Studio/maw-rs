use super::*;

pub(super) fn discord_validate_name(value: &str, label: &str, log: &mut Vec<String>) -> bool {
    if value.is_empty()
        || rejects_option_arg(value)
        || value.contains("..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(|c| c.is_control())
    {
        log.push(format!("✗ invalid {label}: #67 guard rejected value"));
        return false;
    }
    true
}

pub(super) fn discord_validate_channel_arg(value: &str, log: &mut Vec<String>) -> bool {
    if value.is_empty()
        || rejects_option_arg(value)
        || value.contains("..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(|c| c.is_control())
    {
        log.push("✗ invalid channel: #67 guard rejected value".to_owned());
        return false;
    }
    true
}

pub(super) fn discord_validate_snowflake_for_log(value: &str, label: &str, log: &mut Vec<String>) -> bool {
    if !is_numeric_snowflake(value) {
        log.push(format!(
            "✗ invalid {label} id: numeric Discord snowflake required"
        ));
        return false;
    }
    true
}

pub(super) fn discord_redact(input: &str) -> String {
    input
        .split_whitespace()
        .map(discord_redact_word)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn discord_redact_word(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if lower.starts_with("bearer")
        || word.starts_with("ghp_")
        || word.starts_with("github_pat_")
        || word.contains("://")
            && word
                .split("://")
                .nth(1)
                .is_some_and(|rest| rest.contains('@'))
    {
        "[REDACTED]".to_owned()
    } else {
        word.to_owned()
    }
}
