
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
