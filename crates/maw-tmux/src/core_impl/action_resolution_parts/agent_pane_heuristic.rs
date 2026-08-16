// #813: ONE agent-pane heuristic for the whole workspace.
//
// "Does this tmux pane hold a running AI agent?" was answered independently in
// nine places and the copies drifted. The load-bearing fact they drifted on:
//
//   A live Claude Code pane reports its own VERSION STRING as
//   `pane_current_command` (e.g. `2.1.233`), not a process name -- on the npm
//   launch path. On other launch paths the same build reports `claude`.
//   Measured 2026-08-16: host m5 = 7/7 panes report a version string and 0
//   report `claude`; host black = 19/19 report `claude` and 0 report a
//   version. Same code, opposite outcome, decided purely by launch path --
//   which is why a version-blind copy looks fine from one dev machine.
//
// Consequence for any check that leads with the TITLE: Claude Code sets the
// pane title to "Claude Code" only while IDLE and to its current task line
// while BUSY. So a title-keyword check is silent exactly while the agent is
// working and fires only while it idles -- anti-correlated with the thing it
// is meant to protect.
//
// Therefore: COMMAND SHAPE LEADS, title is corroboration only. A substring
// test over a free-form human task line cannot be made correct, only tuned;
// it must never be the only arm.

/// Command-shape agent detection -- the load-bearing arm (#813).
///
/// Composes [`is_claude_like_pane`] (claude substring + three-part numeric
/// version) with the other agent launchers. Distinctive names (`claude`,
/// `codex`) match anywhere in the command line; generic English words
/// (`node`, `gemini`) must be the program actually being run, so `nodemon`
/// and `geminix` stay out.
#[must_use]
pub fn is_agent_pane_command(pane_current_command: Option<&str>) -> bool {
    if is_claude_like_pane(pane_current_command) {
        return true;
    }
    let Some(command) = pane_current_command else {
        return false;
    };
    let lower = command.to_lowercase();
    if lower.contains("codex") {
        return true;
    }
    matches!(agent_pane_program_name(&lower), "node" | "gemini")
}

/// argv0 with any directory prefix stripped, e.g. `/usr/bin/node --x` -> `node`.
fn agent_pane_program_name(lower_command: &str) -> &str {
    let argv0 = lower_command.split_whitespace().next().unwrap_or_default();
    argv0.rsplit('/').next().unwrap_or(argv0)
}

/// Title corroboration -- NEVER sufficient on its own, and never the lead arm.
///
/// The pane title is free-form text an agent rewrites as it works, so this is
/// a tuned heuristic, not a decision procedure. It exists to catch agents
/// whose command shape is unrecognised (a wrapper, a new engine), which is
/// why it stays a union arm under [`is_agent_pane`] rather than a filter.
#[must_use]
pub fn agent_pane_title_hint(pane_title: Option<&str>) -> bool {
    let Some(title) = pane_title else {
        return false;
    };
    let lower = title.to_lowercase();
    ["agent", "oracle", "codex", "claude"]
        .iter()
        .any(|keyword| lower.contains(keyword))
}

/// The shared agent-pane predicate: command shape first, title as backup.
///
/// Call sites that only have the command pass `None` for the title and get
/// exactly [`is_agent_pane_command`]; the two must never disagree on the
/// command arm, which is what `agent_pane_parity_tests` pins.
#[must_use]
pub fn is_agent_pane(pane_current_command: Option<&str>, pane_title: Option<&str>) -> bool {
    is_agent_pane_command(pane_current_command) || agent_pane_title_hint(pane_title)
}
