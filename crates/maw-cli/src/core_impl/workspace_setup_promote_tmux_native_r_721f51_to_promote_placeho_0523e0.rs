fn promote_execute(options: &PromoteOptionsNative, tmux: &mut impl PromoteTmuxNative) -> Result<String, String> {
    let planned = promote_resolve_ready(options, tmux)?;
    let ready = promote_revalidate_ready(options, &planned, tmux)?;
    let dst_exists_now = tmux.promote_has_session(&ready.dst_session);
    if dst_exists_now && !ready.force {
        return Err(promote_destination_exists_error(&options.target, &ready.dst_session));
    }

    let mut state = PromoteMutationStateNative { created_dst_by_this_run: false };
    if !dst_exists_now {
        tmux.promote_new_session(&ready.dst_session, PROMOTE_PLACEHOLDER).map_err(|error| format!("promote: tmux new-session failed — {error}"))?;
        state.created_dst_by_this_run = true;
    }

    let src_target = ready.src_target();
    let dst_target = ready.dst_target();
    promote_validate_tmux_target(&src_target, "source target")?;
    promote_validate_tmux_target(&dst_target, "destination target")?;

    if let Err(error) = tmux.promote_move_window(&src_target, &dst_target) {
        promote_rollback_after_failure(tmux, &ready, &state, "move-window failure");
        return Err(format!("promote: tmux move failed — {error}"));
    }

    let dst_windows = match tmux.promote_list_windows(&ready.dst_session) {
        Ok(windows) => windows,
        Err(error) => {
            promote_cleanup_after_unknown_verify_failure(tmux, &ready, &state);
            return Err(format!(
                "promote: tmux move verification failed — cannot list destination session '{}': {error}; no session rollback performed because ownership cannot be verified; inspect and clean '{}' manually if needed",
                ready.dst_session, ready.dst_session
            ));
        }
    };

    if !promote_window_exists(&dst_windows, &ready.src_window) {
        promote_rollback_after_verify_miss(tmux, &ready, &state, &dst_windows);
        let suffix = if state.created_dst_by_this_run && promote_windows_only_placeholder(&dst_windows) { "; rolled back placeholder session" } else { "" };
        return Err(format!(
            "promote: tmux move verification failed — '{}' did not appear in '{}' after move-window{suffix}",
            src_target, ready.dst_session
        ));
    }

    if state.created_dst_by_this_run {
        let _ = tmux.promote_kill_window(&ready.placeholder_target());
    }

    Ok(promote_render_success(&ready))
}

fn promote_resolve_ready(options: &PromoteOptionsNative, tmux: &mut impl PromoteTmuxNative) -> Result<PromoteResolvedNative, String> {
    let sessions = tmux.promote_list_all();
    promote_resolve_ready_from_sessions(options, tmux, &sessions, "promote planning")
}

fn promote_revalidate_ready(options: &PromoteOptionsNative, planned: &PromoteResolvedNative, tmux: &mut impl PromoteTmuxNative) -> Result<PromoteResolvedNative, String> {
    let sessions = tmux.promote_list_all();
    let fresh = promote_resolve_ready_from_sessions(options, tmux, &sessions, "promote mutation")?;
    if fresh.src_session != planned.src_session || fresh.src_window != planned.src_window {
        return Err(format!(
            "promote: source changed before mutation (planned {}:{}, now {}:{})",
            planned.src_session, planned.src_window, fresh.src_session, fresh.src_window
        ));
    }
    if fresh.dst_session != planned.dst_session {
        return Err(format!("promote: destination changed before mutation (planned {}, now {})", planned.dst_session, fresh.dst_session));
    }
    Ok(fresh)
}

fn promote_resolve_ready_from_sessions(
    options: &PromoteOptionsNative,
    tmux: &mut impl PromoteTmuxNative,
    sessions: &[TmuxSession],
    phase: &str,
) -> Result<PromoteResolvedNative, String> {
    let resolved = promote_resolve_target(&options.target, sessions)?;
    let PromoteResolveResultNative::Resolved { session: src_session, window: src_window } = resolved else {
        return Err(promote_resolution_error_message(&options.target, resolved));
    };
    promote_validate_tmux_name(&src_session, "source session")?;
    promote_validate_tmux_name(&src_window, "source window")?;
    let source_windows = tmux.promote_list_windows(&src_session).map_err(|error| format!("promote: cannot list windows in source session '{src_session}': {error}"))?;
    if source_windows.len() <= 1 {
        return Err(promote_only_window_error(&src_session, &src_window));
    }
    if !promote_window_exists(&source_windows, &src_window) {
        return Err(format!("promote: source '{src_session}:{src_window}' disappeared before {phase}"));
    }
    let dst_session = promote_destination_session(options, &src_window)?;
    Ok(PromoteResolvedNative { src_session, src_window, dst_session, attach: options.attach, force: options.force })
}

fn promote_resolve_target(target: &str, sessions: &[TmuxSession]) -> Result<PromoteResolveResultNative, String> {
    promote_validate_target(target, "target")?;
    let explicit_session = promote_target_session(target)?;
    if let Some(explicit_window) = promote_target_window(target)? {
        return promote_resolve_explicit(&explicit_session, &explicit_window, sessions);
    }
    let mut matches = promote_exact_window_matches(target, sessions);
    if matches.is_empty() {
        if let Some(canonical) = promote_strip_tmux_display_suffix(target) { matches = promote_exact_window_matches(canonical, sessions); }
    }
    Ok(match matches.len() {
        0 => PromoteResolveResultNative::None,
        1 => {
            let candidate = matches.remove(0);
            PromoteResolveResultNative::Resolved { session: candidate.session, window: candidate.window }
        }
        _ => PromoteResolveResultNative::Ambiguous(matches),
    })
}

fn promote_resolve_explicit(session: &str, window: &str, sessions: &[TmuxSession]) -> Result<PromoteResolveResultNative, String> {
    promote_validate_tmux_name(session, "source session")?;
    promote_validate_tmux_name(window, "source window")?;
    let Some(src_session) = sessions.iter().find(|candidate| candidate.name.eq_ignore_ascii_case(session)) else {
        return Ok(PromoteResolveResultNative::Resolved { session: session.to_owned(), window: window.to_owned() });
    };
    if let Some(exact) = src_session.windows.iter().find(|candidate| candidate.name.eq_ignore_ascii_case(window)) {
        return Ok(PromoteResolveResultNative::Resolved { session: src_session.name.clone(), window: exact.name.clone() });
    }
    if let Some(canonical) = promote_strip_tmux_display_suffix(window) {
        if let Some(exact) = src_session.windows.iter().find(|candidate| candidate.name.eq_ignore_ascii_case(canonical)) {
            return Ok(PromoteResolveResultNative::Resolved { session: src_session.name.clone(), window: exact.name.clone() });
        }
    }
    Ok(PromoteResolveResultNative::Resolved { session: src_session.name.clone(), window: window.to_owned() })
}

fn promote_exact_window_matches(target: &str, sessions: &[TmuxSession]) -> Vec<PromoteCandidateNative> {
    sessions.iter().flat_map(|session| {
        session.windows.iter().filter(move |window| window.name == target).map(move |window| PromoteCandidateNative { session: session.name.clone(), window: window.name.clone() })
    }).collect()
}

fn promote_resolution_error_message(target: &str, resolved: PromoteResolveResultNative) -> String {
    match resolved {
        PromoteResolveResultNative::None => format!("promote: no window matches '{target}'"),
        PromoteResolveResultNative::Ambiguous(candidates) => {
            let mut message = format!("promote: '{target}' matches {} windows", candidates.len());
            for candidate in candidates {
                let _ = write!(message, "
  [90m• {}:{}[0m", candidate.session, candidate.window);
            }
            let _ = write!(message, "
  [90muse: maw promote <session>:<window>[0m");
            message
        }
        PromoteResolveResultNative::Resolved { .. } => unreachable!("resolved handled by caller"),
    }
}

fn promote_destination_session(options: &PromoteOptionsNative, src_window: &str) -> Result<String, String> {
    let destination = if let Some(value) = &options.as_session { promote_validate_session_name(value, "--as")? } else { wake_session_name(src_window) };
    promote_validate_session_name(&destination, "destination session")
}

fn promote_validate_target(value: &str, label: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') || value.contains('\0') {
        return Err(format!("promote {label} must be non-empty, unpadded, and not start with '-'"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!("promote {label} must not contain whitespace or control characters"));
    }
    Ok(value.to_owned())
}

fn promote_validate_session_name(value: &str, label: &str) -> Result<String, String> {
    promote_validate_target(value, label)?;
    promote_validate_tmux_name(value, label)?;
    Ok(value.to_owned())
}

fn promote_validate_tmux_name(value: &str, label: &str) -> Result<(), String> {
    wake_validate_tmux_name(value, label).map_err(|_| format!("promote: invalid {label}"))
}

fn promote_target_session(target: &str) -> Result<String, String> {
    let session = target.split(':').next().unwrap_or(target);
    promote_validate_tmux_name(session, "source session")?;
    Ok(session.to_owned())
}

fn promote_target_window(target: &str) -> Result<Option<String>, String> {
    let window = target.split(':').skip(1).collect::<Vec<_>>().join(":");
    let trimmed = window.trim();
    if trimmed.is_empty() { return Ok(None); }
    promote_validate_tmux_name(trimmed, "source window")?;
    Ok(Some(trimmed.to_owned()))
}

fn promote_strip_tmux_display_suffix(window: &str) -> Option<&str> {
    if window.ends_with('-') && window.len() > 1 { Some(&window[..window.len() - 1]) } else { None }
}

fn promote_window_exists(windows: &[maw_tmux::TmuxWindow], name: &str) -> bool {
    windows.iter().any(|window| window.name == name)
}

fn promote_only_window_error(src_session: &str, src_window: &str) -> String {
    format!("promote refused — '{src_window}' is the only window in session '{src_session}'.
  [90mthat would just be a session rename, not an eject.[0m
  [90muse: tmux rename-session -t {src_session} <new-name>[0m")
}

fn promote_destination_exists_error(target: &str, dst_session: &str) -> String {
    format!("promote refused — session '{dst_session}' already exists.
  [90muse: maw promote {target} --as <new-name>[0m
  [90mor:  maw promote {target} --force[0m  (merges into existing)")
}

fn promote_placeholder_target(dst_session: &str) -> String {
    format!("{dst_session}:{PROMOTE_PLACEHOLDER}")
}

