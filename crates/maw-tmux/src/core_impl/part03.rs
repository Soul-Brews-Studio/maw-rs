fn clamp_pty(value: u32, max: u32) -> u32 {
    value.clamp(1, max)
}

/// Strip common ANSI CSI sequences that tmux captures from pane output.
#[must_use]
pub fn strip_tmux_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
            index += 2;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b';') {
                index += 1;
            }
            if index < bytes.len()
                && (bytes[index] == b'm' || bytes[index].is_ascii_uppercase())
            {
                index += 1;
                continue;
            }
            out.push('\u{1b}');
            out.push('[');
            continue;
        }
        let ch = input[index..].chars().next().unwrap_or_default();
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// Return true when captured pane output appears to have pending prompt input.
#[must_use]
pub fn pane_input_pending_from_capture(content: &str) -> bool {
    pane_pending_input_from_capture(content).is_some()
}

/// Return the current prompt input from captured pane output when it appears pending.
#[must_use]
pub fn pane_pending_input_from_capture(content: &str) -> Option<String> {
    let lines = content
        .lines()
        .map(normalize_capture_line)
        .collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate().rev() {
        match prompt_line(line) {
            PromptLine::Pending(input) if trailing_after_prompt_is_chrome(&lines, index) => {
                return Some(input);
            }
            PromptLine::Empty if trailing_after_prompt_is_chrome(&lines, index) => return None,
            PromptLine::Pending(_) | PromptLine::Empty | PromptLine::None => {}
        }
    }
    None
}

/// Classification of pane pending input against the exact text maw just sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingInputState {
    Cleared,
    MatchesSent,
    DifferentInput,
}

/// Classify captured pending input for duplicate-safe Enter retries.
#[must_use]
pub fn pending_input_state_from_capture(content: &str, sent: &str) -> PendingInputState {
    let Some(pending) = pane_pending_input_from_capture(content) else {
        return PendingInputState::Cleared;
    };
    if pending_input_matches_sent(&pending, sent) {
        PendingInputState::MatchesSent
    } else {
        PendingInputState::DifferentInput
    }
}

/// Return true only when the captured pending input is the text maw attempted to submit.
#[must_use]
pub fn pending_input_matches_sent(pending: &str, sent: &str) -> bool {
    let pending = normalize_pending_text(pending);
    let sent = normalize_pending_text(sent);
    if pending.is_empty() || sent.is_empty() {
        return false;
    }
    if pending == sent {
        return true;
    }
    sent.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|first_line| pending == first_line)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PromptLine {
    Pending(String),
    Empty,
    None,
}

fn prompt_line(line: &str) -> PromptLine {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let Some(marker) = chars.next() else {
        return PromptLine::None;
    };
    if !is_prompt_marker(marker) {
        return PromptLine::None;
    }
    let rest = chars.as_str();
    let input = rest.trim_start();
    if input.len() == rest.len() {
        return PromptLine::None;
    }
    let input = input.trim_end();
    if input.is_empty() {
        PromptLine::Empty
    } else {
        PromptLine::Pending(input.to_owned())
    }
}

fn is_prompt_marker(ch: char) -> bool {
    matches!(ch, '#' | '$' | '%' | '>' | '›' | '❯' | '»')
}

fn trailing_after_prompt_is_chrome(lines: &[String], index: usize) -> bool {
    lines
        .iter()
        .skip(index + 1)
        .all(|line| line_is_tui_chrome(line))
}

fn line_is_tui_chrome(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    if matches!(prompt_line(trimmed), PromptLine::Empty) {
        return true;
    }
    if trimmed
        .chars()
        .all(|ch| ch.is_whitespace() || matches!(ch, '─' | '━' | '╌' | '┄' | '┈' | '-' | '—'))
    {
        return true;
    }
    if matches!(
        trimmed.chars().next(),
        Some('🖥' | '📡' | '🟢' | '🟡' | '🔴' | '⏵' | '◯' | '⧉')
    ) {
        return true;
    }
    let lower = trimmed.to_lowercase();
    lower.starts_with("gpt-")
        || lower.starts_with("claude")
        || lower.starts_with("opus")
        || lower.starts_with("sonnet")
        || lower.starts_with("haiku")
        || lower.starts_with("fable")
        || lower.starts_with("? for shortcuts")
        || lower.contains("context left")
}

fn normalize_capture_line(line: &str) -> String {
    strip_tmux_ansi(line)
        .replace('\r', "")
        .replace('\u{a0}', " ")
        .replace('\u{200b}', "")
}

fn normalize_pending_text(text: &str) -> String {
    normalize_capture_line(text).trim().to_owned()
}

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

/// Build tmux args for maw-js `cmdTmuxSplit`.
///
/// # Errors
///
/// Returns pct validation errors.
pub fn tmux_split_action_args(
    resolved: &str,
    options: &TmuxSplitActionOptions,
) -> Result<Vec<String>, TmuxError> {
    let mut args = vec![
        if options.vertical { "-v" } else { "-h" }.to_owned(),
        "-l".to_owned(),
        format!("{}%", split_pct_arg(options.pct)?),
        "-t".to_owned(),
        resolved.to_owned(),
    ];
    if let Some(command) = &options.command {
        args.push(command.clone());
    }
    Ok(args)
}

/// Build tmux args for `send-keys -l <text>` literal typing.
#[must_use]
pub fn tmux_send_keys_literal_args(target: &str, text: &str) -> Vec<String> {
    vec![
        "-t".to_owned(),
        target.to_owned(),
        "-l".to_owned(),
        text.to_owned(),
    ]
}

/// Build tmux args for sending one Enter key.
#[must_use]
pub fn tmux_send_enter_args(target: &str) -> Vec<String> {
    vec!["-t".to_owned(), target.to_owned(), "Enter".to_owned()]
}

/// Build tmux args for maw-js `cmdTmuxSend`.
#[must_use]
pub fn tmux_send_command_args(resolved: &str, command: &str, literal: bool) -> Vec<String> {
    let mut args = vec!["-t".to_owned(), resolved.to_owned(), command.to_owned()];
    if !literal {
        args.push("Enter".to_owned());
    }
    args
}

/// Pure branch selector for maw-js `cmdTmuxAttach`.
#[must_use]
pub fn decide_tmux_attach_action(
    resolved: &str,
    alive_sessions: &BTreeSet<String>,
    print: bool,
    is_tty: bool,
    in_tmux: bool,
) -> TmuxAttachAction {
    let session = match resolve_tmux_attach_session(resolved, alive_sessions) {
        TmuxAttachSessionResolution::Match { session } => session,
        TmuxAttachSessionResolution::Ambiguous { query, .. }
        | TmuxAttachSessionResolution::Missing { session: query } => {
            return TmuxAttachAction::Recover { session: query };
        }
    };
    if print || !is_tty {
        return TmuxAttachAction::Print { session };
    }
    if in_tmux {
        TmuxAttachAction::SwitchClient { session }
    } else {
        TmuxAttachAction::Attach { session }
    }
}

/// Resolve a user-supplied attach target to one live tmux session.
///
/// The ladder mirrors the maw-js attach resolver's practical cases while
/// preserving exact-match priority: exact name first, canonical fleet suffix /
/// dashless aliases second, and loose prefix/substring aliases last.
#[must_use]
pub fn resolve_tmux_attach_session(
    target: &str,
    alive_sessions: &BTreeSet<String>,
) -> TmuxAttachSessionResolution {
    let query = target
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let normalized_query = query.to_lowercase();
    if normalized_query.is_empty() {
        return TmuxAttachSessionResolution::Missing { session: query };
    }

    for tier in 0..=2 {
        let candidates = alive_sessions
            .iter()
            .filter(|session| attach_session_match_tier(session, &normalized_query) == Some(tier))
            .cloned()
            .collect::<Vec<_>>();
        match candidates.len() {
            0 => {}
            1 => {
                return TmuxAttachSessionResolution::Match {
                    session: candidates[0].clone(),
                };
            }
            _ => {
                if tier == 1 {
                    if let Some(session) = preferred_numbered_attach_candidate(&candidates) {
                        return TmuxAttachSessionResolution::Match { session };
                    }
                }
                return TmuxAttachSessionResolution::Ambiguous { query, candidates };
            }
        }
    }

    TmuxAttachSessionResolution::Missing { session: query }
}

fn preferred_numbered_attach_candidate(candidates: &[String]) -> Option<String> {
    let numbered = candidates
        .iter()
        .filter(|candidate| attach_strip_numeric_fleet_prefix(candidate) != candidate.as_str())
        .collect::<Vec<_>>();
    (numbered.len() == 1).then(|| numbered[0].clone())
}

fn attach_session_match_tier(session: &str, normalized_query: &str) -> Option<u8> {
    let name = session.trim().to_lowercase();
    if name == normalized_query {
        return Some(0);
    }
    if name.ends_with(&format!("-{normalized_query}"))
        || name == format!("{normalized_query}-oracle")
        || name.ends_with(&format!("-{normalized_query}-oracle"))
        || strip_dashes(&name) == strip_dashes(normalized_query)
        || legacy_dashless_attach_match(&name, normalized_query)
    {
        return Some(1);
    }

    let stem = attach_strip_oracle_suffix(attach_strip_numeric_fleet_prefix(&name));
    (!normalized_query.is_empty()
        && (stem.starts_with(normalized_query) || stem.contains(normalized_query)))
    .then_some(2)
}

fn legacy_dashless_attach_match(name: &str, normalized_query: &str) -> bool {
    strip_dashes(attach_strip_oracle_suffix(attach_strip_numeric_fleet_prefix(name)))
        == strip_dashes(attach_strip_oracle_suffix(attach_strip_numeric_fleet_prefix(normalized_query)))
}

fn attach_strip_numeric_fleet_prefix(value: &str) -> &str {
    let Some((prefix, rest)) = value.split_once('-') else {
        return value;
    };
    if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        rest
    } else {
        value
    }
}

fn attach_strip_oracle_suffix(value: &str) -> &str {
    value.strip_suffix("-oracle").unwrap_or(value)
}

fn strip_dashes(value: &str) -> String {
    value.replace('-', "")
}

/// Build the `tmux` process command selected for a live attach action.
#[must_use]
pub fn tmux_attach_spawn_command(action: &TmuxAttachAction) -> Option<SpawnCommand> {
    match action {
        TmuxAttachAction::SwitchClient { session } => Some(SpawnCommand {
            program: "tmux".to_owned(),
            args: vec!["switch-client".to_owned(), "-t".to_owned(), session.clone()],
        }),
        TmuxAttachAction::Attach { session } => Some(SpawnCommand {
            program: "tmux".to_owned(),
            args: vec!["attach".to_owned(), "-t".to_owned(), session.clone()],
        }),
        TmuxAttachAction::Print { .. } | TmuxAttachAction::Recover { .. } => None,
    }
}

/// Strip `-oracle` from bare repo names while preserving org/repo slugs.
#[must_use]
pub fn wake_arg_for_similar_oracle(candidate: &str) -> String {
    if candidate.contains('/') {
        candidate.to_owned()
    } else {
        candidate
            .strip_suffix("-oracle")
            .unwrap_or(candidate)
            .to_owned()
    }
}

fn maw_wake_attach_command(oracle: &str) -> SpawnCommand {
    SpawnCommand {
        program: "maw".to_owned(),
        args: vec!["wake".to_owned(), oracle.to_owned(), "-a".to_owned()],
    }
}

/// Build attach recovery candidates from a stale fleet session and similar oracle repos.
#[must_use]
pub fn attach_recovery_candidates(
    target: &str,
    session: &str,
    source: &str,
    fleet_entries: &[AttachRecoveryFleetEntry],
    cloned_repos: &[String],
) -> Vec<AttachRecoveryCandidate> {
    let mut candidates = Vec::new();
    if source.starts_with("fleet-stem")
        || source.starts_with("fleet-window")
        || source.starts_with("live-session")
    {
        if let Some(candidate) =
            fleet_recovery_candidate_for_session(session, fleet_entries, cloned_repos)
        {
            candidates.push(candidate);
        }
    }

    for similar in similar_oracle_candidates_from_repos(target, cloned_repos) {
        let oracle = wake_arg_for_similar_oracle(&similar);
        if !candidates
            .iter()
            .any(|candidate| candidate.oracle == oracle)
        {
            candidates.push(AttachRecoveryCandidate {
                oracle,
                label: similar,
            });
        }
    }
    candidates
}

fn fleet_recovery_candidate_for_session(
    session: &str,
    fleet_entries: &[AttachRecoveryFleetEntry],
    cloned_repos: &[String],
) -> Option<AttachRecoveryCandidate> {
    let entry = fleet_entries.iter().find(|entry| entry.session == session)?;
    let window = entry.first_window_name.as_ref()?;
    let oracle = window.strip_suffix("-oracle").unwrap_or(window).to_owned();
    let cloned = entry
        .repo
        .as_deref()
        .and_then(|repo| {
            cloned_repos
                .iter()
                .find(|path| path.ends_with(&format!("/{repo}")))
        })
        .is_some();
    Some(AttachRecoveryCandidate {
        oracle,
        label: format!("{window} ({})", if cloned { "cloned" } else { "not cloned" }),
    })
}

/// Decide attach recovery behavior after candidates are known.
#[must_use]
pub fn decide_attach_recovery(
    candidates: &[AttachRecoveryCandidate],
    is_tty: bool,
    choice: Option<usize>,
) -> AttachRecoveryDecision {
    match candidates.len() {
        0 => AttachRecoveryDecision::NoCandidates,
        1 => AttachRecoveryDecision::AutoWake {
            command: maw_wake_attach_command(&candidates[0].oracle),
            label: candidates[0].label.clone(),
        },
        _ if !is_tty => AttachRecoveryDecision::PrintCandidates {
            candidates: candidates.to_vec(),
        },
        _ => match choice {
            Some(choice) if (1..=candidates.len()).contains(&choice) => {
                AttachRecoveryDecision::WakeChoice {
                    command: maw_wake_attach_command(&candidates[choice - 1].oracle),
                }
            }
            Some(_) => AttachRecoveryDecision::InvalidChoice,
            None => AttachRecoveryDecision::Prompt {
                candidates: candidates.to_vec(),
            },
        },
    }
}

/// Return the session component from a tmux target.
#[must_use]
pub fn tmux_session_from_target(resolved: &str) -> String {
    resolved.split(':').next().unwrap_or_default().to_owned()
}

/// Apply maw-js orphan-pane fallback for `cmdTmuxKill`.
///
/// Only unresolved bare session-name fallbacks (`source == "session-name"` and `resolved == target`)
/// consult pane titles, tile roles, and worktree aliases. Exact pane IDs and qualified targets are
/// preserved.
///
/// # Errors
///
/// Returns an ambiguity error with concrete candidates when a natural name matches multiple panes.
pub fn resolve_kill_target_with_pane_fallback(
    target: &str,
    resolved: &str,
    source: &str,
    session_kill: bool,
    list_panes_output: &str,
) -> Result<TmuxKillTarget, TmuxError> {
    if !session_kill && source == "session-name" && resolved == target {
        match resolve_pane_target_from_list_panes_output(target, list_panes_output) {
            PaneTargetResolution::Match { candidate } => {
                return Ok(TmuxKillTarget {
                    resolved: candidate.resolved,
                    source: format!("{} ({})", candidate.source, candidate.name),
                });
            }
            PaneTargetResolution::Ambiguous { candidates } => {
                return Err(TmuxError::new(format_pane_ambiguity_error(
                    target,
                    &candidates,
                )));
            }
            PaneTargetResolution::None => {}
        }
    }
    Ok(TmuxKillTarget {
        resolved: resolved.to_owned(),
        source: source.to_owned(),
    })
}
