// #813: ONE agent-pane heuristic for the whole workspace.
//
// "Does this tmux pane hold a running AI agent?" was answered independently in
// nine places and the copies drifted. The load-bearing fact they drifted on:
//
//   A live Claude Code pane reports its own VERSION STRING as
//   `pane_current_command` (e.g. `2.1.233`), not a process name -- on the npm
//   launch path. On other launch paths the same build reports `claude`.
//   Measured 2026-08-16: host m5 = 7/7 panes report a version string and 0
//   report `claude`; host black = 19/19 report `claude` and 0 report a version string.
//   Same code, opposite outcome, decided purely by launch path --
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
/// The shape of the argv0 is the only signal we can use for this heuristic. The
/// detection now declares its matching policy explicitly:
///
/// - low-collision names are matched as `Substring` on the command shape;
/// - distinctive binaries are matched as `ExactProgram` (basename only);
/// - bare argv0 runtime names are excluded before registry matching;
/// - three-part version strings remain supported for macOS-only Claude launches.
#[must_use]
pub fn is_agent_pane_command(pane_current_command: Option<&str>) -> bool {
    let Some(command) = pane_current_command else {
        return false;
    };
    if command.trim().is_empty() {
        return false;
    }
    let lower = command.to_lowercase();
    let program = agent_pane_program_name(&lower);
    if is_generic_runtime_program(program) {
        return false;
    }
    if is_known_agent_name(&lower, program) {
        return true;
    }
    is_claude_like_pane(Some(lower.as_str()))
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum AgentNameMatch {
    Substring,
    ExactProgram,
}

struct KnownAgentName {
    name: &'static str,
    match_kind: AgentNameMatch,
}

const KNOWN_AGENT_NAMES: &[KnownAgentName] = &[
    KnownAgentName { name: "claude", match_kind: AgentNameMatch::Substring },
    KnownAgentName { name: "codex", match_kind: AgentNameMatch::Substring },
    KnownAgentName { name: "gemini", match_kind: AgentNameMatch::ExactProgram },
    KnownAgentName { name: "omx", match_kind: AgentNameMatch::ExactProgram },
    KnownAgentName { name: "opencode", match_kind: AgentNameMatch::ExactProgram },
];

const GENERIC_RUNTIME_EXACT: &[&str] = &[
    "bun", "bunx", "deno", "node", "nodejs", "npx", "python", "uv", "uvx",
];

fn is_known_agent_name(lower: &str, program: &str) -> bool {
    KNOWN_AGENT_NAMES.iter().any(|entry| match entry.match_kind {
        AgentNameMatch::Substring => lower.contains(entry.name),
        AgentNameMatch::ExactProgram => program == entry.name,
    })
}

fn is_generic_runtime_program(program: &str) -> bool {
    if GENERIC_RUNTIME_EXACT.contains(&program) {
        return true;
    }
    is_python_runtime(program)
}

fn is_python_runtime(program: &str) -> bool {
    if program == "python" {
        return true;
    }
    if let Some(suffix) = program.strip_prefix("python") {
        return !suffix.is_empty()
            && suffix
                .chars()
                .all(|character| character.is_ascii_digit() || character == '.');
    }
    false
}

/// argv0 with any directory prefix stripped, e.g. `/usr/bin/node --x` -> `node`.
fn agent_pane_program_name(lower_command: &str) -> &str {
    let argv0 = lower_command.split_whitespace().next().unwrap_or_default();
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    base
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

#[cfg(test)]
mod agent_pane_heuristic_tests {
    use super::*;

    #[test]
    fn registry_and_runtime_rails() {
        let positive = [
            "claude",
            "codex",
            "gemini",
            "omx",
            "opencode",
            "/usr/bin/omx",
            "/usr/bin/opencode",
        ];
        for command in positive {
            assert!(is_agent_pane_command(Some(command)));
        }
        let negative = [
            "xomx",
            "opencode-helper",
            "bun run claude",
            "bun run dev",
            "node",
            "nodejs",
            "bun",
            "bunx",
            "deno",
            "python",
            "python3",
            "python3.12",
            "npx",
            "uv",
            "uvx",
            "/opt/bin/node",
            "python3 -c 'print(1)'",
        ];
        for command in negative {
            assert!(!is_agent_pane_command(Some(command)), "{command}");
        }
        assert!(GENERIC_RUNTIME_EXACT
            .iter()
            .all(|runtime| is_generic_runtime_program(runtime)));
        assert!(
            KNOWN_AGENT_NAMES
                .iter()
                .all(|agent| !is_generic_runtime_program(agent.name))
        );
    }
}
