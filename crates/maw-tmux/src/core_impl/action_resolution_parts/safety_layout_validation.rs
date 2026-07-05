
/// Scan a command for maw-js `maw tmux send` destructive deny-list patterns.
#[must_use]
pub fn check_destructive(command: &str) -> DestructiveCheck {
    let mut reasons = Vec::new();
    if contains_word(command, "rm") {
        reasons.push("rm — removes files".to_owned());
    }
    if contains_word(command, "sudo") {
        reasons.push("sudo — elevated privileges".to_owned());
    }
    if has_redirect(command, false) {
        reasons.push("> redirect — overwrites".to_owned());
    }
    if has_redirect(command, true) {
        reasons.push(">> redirect — appends (possibly to wrong place)".to_owned());
    }
    if has_operator_with_rhs(command, ';') {
        reasons.push("; command chain — multiple commands".to_owned());
    }
    if has_sequence_with_rhs(command, "&&") {
        reasons.push("&& chain — conditional execution".to_owned());
    }
    if has_operator_with_rhs(command, '|') {
        reasons.push("| pipe — composition (review carefully)".to_owned());
    }
    let lower = command.to_lowercase();
    if lower.contains("git reset --hard") {
        reasons.push("git reset --hard — discards changes".to_owned());
    }
    if contains_word(&lower, "git") && lower.contains("push") && lower.contains("--force") {
        reasons.push("git push --force — rewrites history".to_owned());
    }
    if contains_word(&lower, "git") && lower.contains("clean -f") {
        reasons.push("git clean -f — removes untracked files".to_owned());
    }
    if contains_word(&lower, "gh") && contains_word(&lower, "delete") {
        reasons.push("gh delete — removes GitHub resource".to_owned());
    }
    if contains_word(&lower, "clear") {
        reasons.push("clear — clears pane context".to_owned());
    }
    if contains_word(&lower, "exit") {
        reasons.push("exit — exits the foreground process".to_owned());
    }
    if contains_word(&lower, "kill") {
        reasons.push("kill — terminates process".to_owned());
    }
    if lower.split_whitespace().any(|token| matches!(token, "c-c" | "c-d")) {
        reasons.push("control key — may interrupt or close a session".to_owned());
    }
    if lower.contains("drop table") {
        reasons.push("DROP TABLE — removes database table".to_owned());
    }
    DestructiveCheck {
        destructive: !reasons.is_empty(),
        reasons,
    }
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || bytes.len() < needle.len() {
        return false;
    }
    for index in 0..=bytes.len() - needle.len() {
        if !bytes[index..].starts_with(needle) {
            continue;
        }
        let before = index.checked_sub(1).and_then(|i| bytes.get(i));
        let after = bytes.get(index + needle.len());
        if before.is_none_or(|byte| !is_word_byte(*byte))
            && after.is_none_or(|byte| !is_word_byte(*byte))
        {
            return true;
        }
    }
    false
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn has_redirect(command: &str, append: bool) -> bool {
    let bytes = command.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if append {
            if bytes[index..].starts_with(b">>") && has_non_space_after(&bytes[index + 2..]) {
                return true;
            }
            index += 1;
        } else {
            if bytes[index] == b'>'
                && bytes.get(index + 1) != Some(&b'>')
                && has_non_space_after(&bytes[index + 1..])
            {
                return true;
            }
            index += 1;
        }
    }
    false
}

fn has_operator_with_rhs(command: &str, operator: char) -> bool {
    command
        .split_once(operator)
        .is_some_and(|(_, rhs)| !rhs.trim().is_empty())
}

fn has_sequence_with_rhs(command: &str, sequence: &str) -> bool {
    command
        .split_once(sequence)
        .is_some_and(|(_, rhs)| !rhs.trim().is_empty())
}

fn has_non_space_after(bytes: &[u8]) -> bool {
    bytes.iter().any(|byte| !byte.is_ascii_whitespace())
}

/// Detect Claude Code or version-shaped Claude wrapper pane commands.
#[must_use]
pub fn is_claude_like_pane(pane_current_command: Option<&str>) -> bool {
    let Some(command) = pane_current_command else {
        return false;
    };
    let command = command.to_lowercase();
    if command.contains("claude") {
        return true;
    }
    is_three_part_numeric_version(command.trim())
}

fn is_three_part_numeric_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let first = parts.next().unwrap_or_default();
    let Some(second) = parts.next() else {
        return false;
    };
    let Some(third) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    [first, second, third]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Protect fleet and view sessions from accidental kill operations.
#[must_use]
pub fn is_fleet_or_view_session(session_name: &str, fleet_sessions: &BTreeSet<String>) -> bool {
    fleet_sessions.contains(session_name)
        || session_name == "maw-view"
        || session_name.ends_with("-view")
}

/// Validate maw-js `maw tmux layout` presets.
///
/// # Errors
///
/// Returns a message listing every valid preset when `preset` is invalid.
pub fn validate_layout_preset(preset: &str) -> Result<(), TmuxError> {
    if VALID_LAYOUTS.contains(&preset) {
        Ok(())
    } else {
        Err(TmuxError::new(format!(
            "invalid layout '{preset}'. Valid: {}",
            VALID_LAYOUTS.join(", ")
        )))
    }
}

/// Strip a pane suffix from a tmux target so layout applies to the window.
#[must_use]
pub fn tmux_window_target(resolved: &str) -> String {
    let Some(dot) = resolved.rfind('.') else {
        return resolved.to_owned();
    };
    let Some(colon) = resolved.rfind(':') else {
        return resolved.to_owned();
    };
    if dot > colon + 1
        && resolved[dot + 1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        resolved[..dot].to_owned()
    } else {
        resolved.to_owned()
    }
}

/// Validate and render maw-js `maw tmux split --pct`.
///
/// # Errors
///
/// Returns the maw-js-compatible bounds message for NaN, infinities, and values outside `1..=99`.
pub fn split_pct_arg(pct: f64) -> Result<String, TmuxError> {
    if !pct.is_finite() || !(1.0..=99.0).contains(&pct) {
        return Err(TmuxError::new(format!("--pct must be 1-99 (got {pct})")));
    }
    Ok(format_js_number(pct))
}

fn format_js_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}
