// Noticing when a delivery landed somewhere no agent will ever read it.
//
// A misaddressed `hey` still succeeds: the text is typed into whatever pane the
// target resolved to, even a plain shell. These predicates look at the pane's
// running command and title, decide whether it looks like an agent at all, and
// hand the sender back a warning naming exactly what was found (#709). It warns
// rather than refuses on purpose -- the pane may legitimately be something new.

/// #709: a bare session target resolves to a specific window/pane (often
/// index 0) with no check that anything agent-shaped is actually there --
/// window renames, pane replacement, or an agent closing leave `delivered`
/// reporting success into a plain shell.
///
/// #813: the local keyword/semver duplicate that used to live here is gone --
/// it had already drifted from the `/api/agents` copy (this one accepted
/// `1.2.3.4` as a version, that one did not). Both now call the one shared
/// `maw_tmux::is_agent_pane`.
fn serve_pane_looks_like_agent(command: &str, title: &str) -> bool {
    maw_tmux::is_agent_pane(Some(command), Some(title))
}

/// `resolved` (from `resolve_route_target`) is `session:windowIndex` -- but
/// `TmuxPane::target` (from `list_panes`) reports panes as `session:
/// windowNAME.paneIndex`. Resolve the index to the window's actual name
/// against the session list before any pane lookup can work at all; without
/// this step a naive string match against `resolved` silently never finds
/// the pane (caught by this fix's own test before it shipped).
fn serve_window_name_for_resolved_target(sessions: &[RouteSession], resolved: &str) -> Option<String> {
    let (session_name, window_part) = resolved.split_once(':')?;
    let window_part = window_part.split('.').next().unwrap_or(window_part);
    let session = sessions.iter().find(|session| session.name == session_name)?;
    if let Ok(index) = window_part.parse::<u32>() {
        session
            .windows
            .iter()
            .find(|window| window.index == index)
            .map(|window| window.name.clone())
    } else {
        session
            .windows
            .iter()
            .find(|window| window.name.eq_ignore_ascii_case(window_part))
            .map(|window| window.name.clone())
    }
}

/// Find the live pane(s) under a resolved session:window, matched by window
/// NAME (see `serve_window_name_for_resolved_target`) -- the first pane
/// (lowest pane index) is what a windowless target like `session:0` sends to.
fn serve_pane_for_resolved_target<'a>(
    panes: &'a [TmuxPane],
    session_name: &str,
    window_name: &str,
) -> Option<&'a TmuxPane> {
    let prefix = format!("{session_name}:{window_name}.");
    let mut matches: Vec<&TmuxPane> = panes.iter().filter(|pane| pane.target.starts_with(&prefix)).collect();
    matches.sort_by(|left, right| left.target.cmp(&right.target));
    matches.into_iter().next()
}

/// `None` when the pane could not be found at all (already covered by the
/// existing toctou/target-disappeared checks) or when it looks agent-shaped.
/// `Some(warning)` names exactly what `delivered` actually reached, per #709's
/// third ask ("delivered should name what it delivered to"). Pure over
/// already-fetched session/pane data so it's testable without a real tmux.
fn serve_non_agent_pane_warning_from_panes(
    sessions: &[RouteSession],
    panes: &[TmuxPane],
    resolved: &str,
) -> Option<String> {
    let (session_name, _) = resolved.split_once(':')?;
    let window_name = serve_window_name_for_resolved_target(sessions, resolved)?;
    let pane = serve_pane_for_resolved_target(panes, session_name, &window_name)?;
    // INVERTED CALL SITE: matching SUPPRESSES the warning.
    //
    // Everywhere else a match means "act on this pane". Here it means "stay
    // quiet", so widening the predicate NARROWS this guard, and any widening
    // is a decision to go silent for the widened shapes. That is why bare
    // `node` must not match: a `node` pane where an agent used to be is the
    // pane-replacement case #709 was built for, and matching it would delete
    // both this warning and the delivered-non-agent lifecycle log.
    //
    // The reverse error is not free either: this warning fired on 7 of 7
    // healthy panes on m5 and its reader learned to ignore it, which destroys
    // the true positives along with the false ones. Both directions cost
    // something, so changes here get pinned by symptom in the parity table,
    // not by boolean.
    if serve_pane_looks_like_agent(&pane.command, &pane.title) {
        return None;
    }
    Some(format!(
        "delivered to {resolved} (window '{window_name}') but its pane runs '{}' (title: '{}'), not an agent -- likely misaddressed",
        pane.command, pane.title
    ))
}

fn serve_non_agent_pane_warning(resolved: &str) -> Option<String> {
    // #860: this runs *after* delivery already succeeded, purely to decide
    // whether to attach a best-effort "landed in a non-agent pane" warning.
    // The function's only channel back to its caller is `Option<String>` --
    // there is nowhere sensible to surface "tmux became unreachable between
    // delivery and this check", so a tmux error here degrades to "no warning"
    // rather than losing the (already-successful) delivery outcome.
    let mut tmux = TmuxClient::local();
    let sessions = route_sessions_from_tmux(&mut tmux).unwrap_or_default();
    let panes = tmux.list_panes().unwrap_or_default();
    serve_non_agent_pane_warning_from_panes(&sessions, &panes, resolved)
}

fn serve_log_non_agent_pane_warning(state: &ServeState, resolved: &str) -> Option<String> {
    let warning = serve_non_agent_pane_warning(resolved)?;
    serve_log_lifecycle(
        state,
        json!({
            "kind": "context.message",
            "direction": "inbound",
            "state": "delivered-non-agent",
            "route": "local",
            "target": resolved,
            "warning": &warning,
            "source": "maw-rs-native",
        }),
    );
    Some(warning)
}
