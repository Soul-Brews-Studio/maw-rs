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
    // ===== #648 fanout: serve-zoom =====
    ("serve-peer-startup-warnings", HelpTier::Other, ""),
    ("session", HelpTier::Standard, "Alias for whoami — print the current tmux session name."),
    ("setup", HelpTier::Standard, "Configure reboot-safe host services; use user-setup for per-user project configuration."),
    ("shellenv", HelpTier::Other, ""),
    ("signals", HelpTier::Other, ""),
    ("snapshots", HelpTier::Standard, "List, create, or inspect fleet recovery snapshots; use setup auto-wake to restore them after reboot."),
    ("split", HelpTier::Standard, "Split the current tmux pane and attach a target beside it; use bring to wake and split an oracle."),
    ("split-policy", HelpTier::Other, ""),
    ("swap", HelpTier::Standard, "Swap two tmux panes by target; use pane for current-window pane operations."),
    ("t", HelpTier::Standard, "Alias for team — manage agent team lifecycle, tasks, messaging, and recovery."),
    ("tab", HelpTier::Standard, "List, create, rename, or close tmux windows; use session for the current session name."),
    ("tag", HelpTier::Standard, "Read or update maw metadata tags on a tmux target; use panes to inspect pane metadata."),
    ("talk", HelpTier::Standard, "Alias for talk-to — send a signed message through maw federation."),
    ("talk-to", HelpTier::Standard, "Send signed messages to another oracle or agent through maw federation."),
    ("talkto", HelpTier::Standard, "Alias for talk-to — send a signed message through maw federation."),
    ("team", HelpTier::Standard, "Manage agent team lifecycle, tasks, messaging, liveness, shutdown, and recovery."),
    ("tmux", HelpTier::Standard, "Use maw's guarded tmux controls for listing, peeking, splitting, attaching, and pane operations."),
    ("tokens", HelpTier::Extra, "List and switch engine credentials; use token for the primary token-management interface."),
    ("tonk", HelpTier::Extra, "Use Tonk oracle helpers for status, student-style greetings, and GitHub Discussions."),
    ("transport", HelpTier::Other, ""),
    ("trigger", HelpTier::Other, ""),
    ("triggers", HelpTier::Other, ""),
    ("trust", HelpTier::Extra, "Manage pairwise sender-to-target trust used by scope access control."),
    ("trusts", HelpTier::Extra, "Alias for trust — list, pin, or revoke pairwise sender-to-target trust."),
    ("ui", HelpTier::Extra, "Install, launch, or tunnel the maw web UI; use --dev or --3d for development modes."),
    ("upgrade", HelpTier::Standard, "Alias for update — check or install a maw release from the stable or alpha channel."),
    ("user-setup", HelpTier::Standard, "Inspect or apply per-user maw project configuration; use setup for host boot services."),
    ("view", HelpTier::Standard, "Create or attach to an agent tmux view, optionally read-only or split beside this pane."),
    ("wave", HelpTier::Extra, "Coordinate a staged agent wave through start, status, dispatch, and teardown."),
    ("whoami", HelpTier::Standard, "Print the current tmux session, window, pane, and exact target."),
    ("workon", HelpTier::Standard, "Open or resume repository work with optional task, worktree, engine, and layout context."),
    ("workspace", HelpTier::Standard, "Create, join, share, and inspect multi-node workspaces; use workon for local repository work."),
    ("worktree", HelpTier::Standard, "List, add, or clean git worktrees; use done to finish an active worktree session."),
    ("worktree-window", HelpTier::Other, ""),
    ("ws", HelpTier::Other, ""),
    ("xdg", HelpTier::Extra, "Inspect maw XDG paths and validate instance names; use setup for configuration changes."),
    ("zai", HelpTier::Standard, "Monitor, benchmark, and probe the configured Z.AI credential pool without printing tokens."),
    ("zenoh-scout", HelpTier::Other, ""),
    ("zoom", HelpTier::Standard, "Toggle tmux pane zoom for a target; use view when you need a separate attached view."),
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
