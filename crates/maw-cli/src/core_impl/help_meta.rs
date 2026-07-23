#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HelpTier {
    Core,
    Standard,
    Extra,
    Other,
}

#[derive(Clone, Copy, Debug)]
struct HelpMeta {
    tier: HelpTier,
    description: &'static str,
    order: usize,
}

const HELP_TIER_ORDER: &[HelpTier] = &[
    HelpTier::Core,
    HelpTier::Standard,
    HelpTier::Extra,
    HelpTier::Other,
];

const HELP_META: &[(&str, HelpTier, &str)] = &[
    ("a", HelpTier::Core, "Alias for attach — jump to a live or waking oracle without spelling the full verb."),
    ("attach", HelpTier::Core, "Smart attach to a live tmux session, or wake a fleet oracle before attaching."),
    ("ls", HelpTier::Core, "List live local sessions; use fleet for the persistent registry and discover for peers."),
    ("wake", HelpTier::Core, "Launch or reuse an oracle engine pane; use bring when you want it split into your current view."),
    ("work", HelpTier::Core, "Open or create a repo workspace, with optional issue/task context for the engine."),
    ("run", HelpTier::Core, "Type text into a target pane and press Enter; use send for raw text without submission."),
    ("send-enter", HelpTier::Core, "Press Enter in a target pane; pair with send when pending input needs manual submission."),
    ("send-key", HelpTier::Core, "Send one allowlisted tmux key to a target pane; use send-text for normal text input."),
    ("send-escape", HelpTier::Core, "Send Escape to a target pane for cancelling prompts or leaving terminal modes."),
    ("bg", HelpTier::Core, "Run a long command in detached tmux and sample it later without blocking this pane."),
    ("hey", HelpTier::Core, "Send a signed message to another oracle over federation; use messages to inspect the ledger."),
    ("kill", HelpTier::Core, "Immediately remove a tmux session, window, or pane; use done for worktree cleanup."),
    ("bud", HelpTier::Core, "Create a new oracle workspace from this one; use awaken for bud plus first trigger."),
    ("token", HelpTier::Core, "List and switch engine auth tokens; use codex/profile for adjacent account state."),
    ("bring", HelpTier::Core, "Bring an oracle here by planning a wake split into the current session."),
    ("b", HelpTier::Core, "Alias for bring — short path for pulling another oracle into your current session."),
    ("feed", HelpTier::Core, "Inspect local activity feed data, including active status and parse-line diagnostics."),
    ("x", HelpTier::Core, "Run a plugin from a source spec with optional sha256 pin verification."),
    ("plugin", HelpTier::Core, "Inspect and manage installed plugins; use x for one-shot source-spec execution."),
    ("update", HelpTier::Core, "Check or apply maw binary updates on the stable or alpha channel."),
    ("calver", HelpTier::Core, "Compute the next CalVer release version for stable or alpha release flows."),
    ("version", HelpTier::Core, "Show the build version for this maw binary."),
    ("send-text", HelpTier::Core, "Type text and press Enter into a target pane; use send when Enter must be separate."),
    ("sleep", HelpTier::Core, "Gracefully stop one oracle window; use kill for immediate removal or done for worktrees."),
    ("health", HelpTier::Core, "Check local and peer health surfaces before routing work across the fleet."),
    ("oracle", HelpTier::Core, "Inspect and manage oracle records; use ls for live panes and fleet for registry repair."),
    ("stop", HelpTier::Core, "Stop all fleet sessions; use sleep for one oracle or kill for immediate tmux removal."),
    ("done", HelpTier::Core, "Finish a worktree session, save retrospective state, kill the window, and remove the worktree."),
    ("finish", HelpTier::Core, "Alias for done — complete a worktree session rather than merely sleeping it."),
    ("take", HelpTier::Core, "Move a tmux window between oracle sessions; use pane/swap for pane-level movement."),
    ("squad", HelpTier::Core, "Lead-centric team workflow for start, join, say, and list without typing a team name."),
    ("awaken", HelpTier::Core, "Bud, wake, and fire the awakening ritual in one command."),
    ("send", HelpTier::Core, "Send raw text to a pane without pressing Enter; pair with send-enter to submit later."),
    ("init", HelpTier::Core, "Run first-time maw configuration setup for the local user."),
    ("pane", HelpTier::Core, "Work with panes in the current tmux window; use panes to list pane metadata."),
    ("swarm", HelpTier::Core, "Spawn multiple AI-agent panes side by side for collaborative work."),
    ("scaffold", HelpTier::Core, "Create an oracle repo skeleton without committing, waking, or awakening it."),
    ("awake", HelpTier::Core, "Launch an oracle process with an optional engine without firing the awaken trigger."),
    ("new", HelpTier::Core, "Create a plain tmux workspace session when you do not need oracle scaffolding."),
    ("preflight", HelpTier::Core, "Run readiness checks for version, plugins, dead agents, and config before a workflow."),
    // ===== #648 fanout: pair-serve =====
    ("pair-code", HelpTier::Other, ""),
    ("pair-code-store", HelpTier::Other, ""),
    ("panes", HelpTier::Standard, "List pane metadata; use pane swap to move panes or tile to arrange and spawn grids."),
    ("park", HelpTier::Standard, "Park a tmux tab for later restoration; use open when you only need to rejoin a hidden pane."),
    ("peek", HelpTier::Standard, "Peek at recent agent output without attaching; use capture when you need full scrollback."),
    ("peer", HelpTier::Standard, "Alias for peers — manage federation aliases, probe connectivity, and inspect remote nodes."),
    ("peer-probe", HelpTier::Other, ""),
    ("peer-sources", HelpTier::Other, ""),
    ("peers", HelpTier::Standard, "Manage federation peer aliases and probe remote nodes; use scout for liveliness discovery."),
    ("ping", HelpTier::Extra, "Ping all peer nodes or one named node to check connectivity and authentication status."),
    ("plugin-artifact", HelpTier::Other, ""),
    ("plugin-manifest", HelpTier::Other, ""),
    ("plugin-policy", HelpTier::Other, ""),
    ("plugin-scaffold", HelpTier::Extra, "Validate plugin names and generate Rust or AssemblyScript manifest scaffolds; use plugin build afterward."),
    ("plugins", HelpTier::Standard, "Manage installed plugin sets and enablement; use plugin for lifecycle commands and x for one-shot execution."),
    ("policy", HelpTier::Extra, "Inspect plugin weight and default-activation policy; use plugins to change installed or enabled sets."),
    ("pr", HelpTier::Standard, "Open a pull request for the current work window, or inspect its existing PR metadata."),
    ("profile", HelpTier::Extra, "List, inspect, and switch named maw configuration profiles; use token for engine credentials."),
    ("project", HelpTier::Standard, "Learn, incubate, find, or list tracked repositories; use work when you are ready to open one."),
    ("promote", HelpTier::Other, ""),
    ("reboot", HelpTier::Standard, "Alias for restart — update if requested, stop the fleet, then wake it again."),
    ("recent-hello", HelpTier::Other, ""),
    ("reindex-gpu", HelpTier::Other, ""),
    ("rename", HelpTier::Standard, "Rename a tmux tab by index or name; use rename-pane for a pane title."),
    ("rename-pane", HelpTier::Standard, "Rename a target pane title; use rename when changing a whole tmux tab."),
    ("reply", HelpTier::Standard, "Reply to a correlated request, or list pending replies for an oracle."),
    ("resize", HelpTier::Standard, "Resize the current pane by direction and amount, or equalize the window layout."),
    ("resolve", HelpTier::Extra, "Resolve a target against names, sessions, or worktrees without performing a mutation."),
    ("rest", HelpTier::Standard, "Stop all oracle fleet sessions for a full rest; use sleep for one oracle."),
    ("restart", HelpTier::Standard, "Restart the whole fleet, optionally updating to a selected git ref first."),
    ("resume", HelpTier::Standard, "Resume sleeping oracle fleet sessions; use wake when starting one specific oracle."),
    ("reunion", HelpTier::Other, ""),
    ("route", HelpTier::Extra, "Explain how a target routes across peers, agents, sessions, and windows without sending anything."),
    ("rp", HelpTier::Other, ""),
    ("scatter", HelpTier::Other, ""),
    ("schedule", HelpTier::Extra, "Manage persistent per-oracle schedules, runs, logs, and pause or resume state."),
    ("scope", HelpTier::Standard, "List and manage access-control scopes and membership; use trust for pairwise authorization."),
    ("scout", HelpTier::Standard, "Discover peer liveliness through opt-in Scout or Zenoh transports; use peers to persist aliases."),
    ("serve", HelpTier::Standard, "Run or manage the maw HTTP and WebSocket server for APIs, federation, and browser views."),
    ("serve-identity", HelpTier::Other, ""),
];

fn help_meta_for(command: &str) -> HelpMeta {
    HELP_META
        .iter()
        .enumerate()
        .find_map(|(order, (name, tier, description))| {
            (*name == command).then_some(HelpMeta {
                tier: *tier,
                description,
                order,
            })
        })
        .unwrap_or(HelpMeta {
            tier: HelpTier::Other,
            description: "",
            order: usize::MAX,
        })
}

fn help_tier_label(tier: HelpTier) -> &'static str {
    match tier {
        HelpTier::Core => "core",
        HelpTier::Standard => "standard",
        HelpTier::Extra => "extra",
        HelpTier::Other => "other",
    }
}
