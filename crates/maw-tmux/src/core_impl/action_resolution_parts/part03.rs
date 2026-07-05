
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
    if normalized_query.bytes().all(|byte| byte.is_ascii_digit())
        && attach_numeric_fleet_prefix(&name) == Some(normalized_query)
    {
        return Some(1);
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
        && (name.starts_with(normalized_query)
            || stem.starts_with(normalized_query)
            || stem.contains(normalized_query)))
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

fn attach_numeric_fleet_prefix(value: &str) -> Option<&str> {
    let (prefix, _) = value.split_once('-')?;
    (!prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit())).then_some(prefix)
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
