fn kill_ambiguous_session(target: &str, candidates: &[KillSession]) -> String {
    let mut out = format!(
        "  \x1b[31m✗\x1b[0m '{target}' is ambiguous — matches {} sessions:",
        candidates.len()
    );
    for session in candidates {
        let _ = write!(out, "\n  \x1b[90m    • {}\x1b[0m", session.name);
    }
    out.push_str("\n  \x1b[90m  use the full name: maw kill <exact-session>\x1b[0m");
    out
}

fn kill_missing_session(target: &str, hints: Option<&[KillSession]>) -> String {
    let mut out = format!("  \x1b[31m✗\x1b[0m session '{target}' not found");
    if let Some(hints) = hints.filter(|hints| !hints.is_empty()) {
        out.push_str("\n  \x1b[90m  did you mean:\x1b[0m");
        for session in hints {
            let _ = write!(out, "\n  \x1b[90m    • {}\x1b[0m", session.name);
        }
    } else {
        out.push_str("\n  \x1b[90m  try: maw ls\x1b[0m");
    }
    out
}

fn kill_ambiguous_window(session: &KillSession, raw_window: &str, matches: &[u32]) -> String {
    let mut out = format!(
        "window '{raw_window}' is ambiguous in session {} — matches {} windows:",
        session.name,
        matches.len()
    );
    for index in matches {
        if let Some(window) = session.windows.iter().find(|window| window.index == *index) {
            let _ = write!(out, "\n    • {}:{}", window.index, window.name);
        }
    }
    out.push_str("\n  use --index N to kill one, or --all to kill all matching windows");
    out
}

fn kill_ambiguous_panes(target: &str, candidates: &[maw_tmux::PaneTargetCandidate]) -> String {
    let mut out = format!(
        "  \x1b[31m✗\x1b[0m '{target}' is ambiguous — matches {} panes:",
        candidates.len()
    );
    for candidate in candidates {
        let _ = write!(
            out,
            "\n  \x1b[90m    • {} → {} ({}) [{}]\x1b[0m",
            candidate.name, candidate.resolved, candidate.target, candidate.source
        );
    }
    out.push_str("\n  \x1b[90m  use the pane id or full session:window.pane target\x1b[0m");
    out
}

#[cfg(test)]
mod kill_tests {
    include!("kill_kill_tests/01_kill_fake_tmux_to_kill_ambiguous_window_a02c62.rs");
    include!("kill_kill_tests/02_kill_pane_lists_valid_9427d9_to_kill_rejects_bad_fl_64cb3e.rs");
}
