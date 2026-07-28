// Which window did they mean, when `hey` was given a bare name.
//
// Shares the typed-candidate machinery the other verbs use, so `hey nova` and
// `attach nova` cannot disagree about what nova is, and offers the same picker
// when the answer is genuinely more than one.

fn resolve_send_route_target<R: maw_tmux::TmuxRunner>(
    query: &str,
    config: &RouteConfig,
    sessions: &[RouteSession],
    inside_tmux: bool,
    runner: &mut R,
) -> RouteResult {
    let current_session = if is_self_target_alias(query) {
        match send_current_session_name(inside_tmux, runner) {
            Ok(current_session) => current_session,
            Err(detail) => {
                return RouteResult::Error {
                    reason: "me_needs_tmux".to_owned(),
                    detail,
                    hint: Some("run inside tmux so maw can resolve the current session".to_owned()),
                }
            }
        }
    } else {
        None
    };
    resolve_route_target_with_current_session(query, config, sessions, current_session.as_deref())
}

fn hey_picker_target(target: &str, config: &RouteConfig, sessions: &[RouteSession]) -> Result<String, CliOutput> {
    if target.contains(':') || target.contains('/') || is_self_target_alias(target) { return Ok(target.to_owned()); }
    match typed_picker_plan(target, &hey_typed_candidates(config, sessions), hey_kind_priority, hey_picker_row) {
        TypedPickerPlan::Target(target) => Ok(target),
        TypedPickerPlan::Pick { context, rows } => picker_choose_target("hey", target, context, &rows, false),
    }
}

fn hey_typed_candidates(config: &RouteConfig, sessions: &[RouteSession]) -> Vec<maw_matcher::ResolveTypedCandidate> {
    let alive = sessions.iter().filter(|session| !session.name.ends_with("-view") && session.source.as_deref().is_none_or(|source| source == "local"))
        .map(|session| session.name.clone()).collect::<BTreeSet<_>>();
    let mut candidates = resolver_live_candidates(&alive);
    for session in sessions.iter().filter(|session| !session.name.ends_with("-view") && session.source.as_deref().is_none_or(|source| source == "local")) {
        candidates.extend(session.windows.iter().map(|window| maw_matcher::ResolveTypedCandidate {
            kind: maw_matcher::ResolveCandidateKind::Window,
            name: format!("{}:{}", session.name, window.index),
            aliases: vec![window.name.clone()],
        }));
    }
    candidates.extend(config.agents.keys().map(|agent| maw_matcher::ResolveTypedCandidate {
        kind: maw_matcher::ResolveCandidateKind::Peer, name: agent.clone(), aliases: Vec::new(),
    }));
    candidates
}

fn hey_kind_priority(kind: maw_matcher::ResolveCandidateKind) -> u8 {
    match kind {
        maw_matcher::ResolveCandidateKind::Window => 0,
        maw_matcher::ResolveCandidateKind::LiveSession => 1,
        maw_matcher::ResolveCandidateKind::Peer => 2,
        _ => 3,
    }
}

fn hey_picker_row(matched: maw_matcher::ResolveMatch) -> PickerRow {
    let detail = (matched.candidate.kind == maw_matcher::ResolveCandidateKind::Window)
        .then(|| matched.candidate.aliases.first().cloned()).flatten();
    PickerRow { action: format!("maw hey {} <message>", matched.candidate.name), detail, matched }
}

fn send_current_session_name<R: maw_tmux::TmuxRunner>(
    inside_tmux: bool,
    runner: &mut R,
) -> Result<Option<String>, String> {
    if !inside_tmux {
        return Ok(None);
    }
    let raw = runner
        .run(
            "display-message",
            &["-p".to_owned(), "#{session_name}".to_owned()],
        )
        .map_err(|error| {
            format!("'me' needs a tmux context: tmux display-message failed: {}", error.message)
        })?;
    let session = raw.trim();
    if session.is_empty() {
        return Err("'me' needs a tmux context: tmux did not report a current session".to_owned());
    }
    Ok(Some(session.to_owned()))
}
