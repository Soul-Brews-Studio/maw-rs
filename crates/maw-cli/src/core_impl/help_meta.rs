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
    // ===== #648 fanout: about-consent =====
    ("about", HelpTier::Standard, "Show oracle details — session, repo, windows, and fleet metadata; use ls for live status."),
    ("absorb", HelpTier::Extra, "Absorb one oracle into another, archive the donor, and switch to the receiver."),
    ("activity", HelpTier::Standard, "Classify pane activity by diffing peek snapshots; use feed for local activity events."),
    ("agent", HelpTier::Standard, "Alias for agents — list configured agent routes and live panes for one node or the fleet."),
    ("agents", HelpTier::Standard, "List configured agent routes and live panes; use agents gc to prune stale manifest entries."),
    ("alive", HelpTier::Standard, "Check whether a session or oracle is live and count panes; use ls for the full table."),
    ("archive", HelpTier::Extra, "Archive an oracle's tmux session and data."),
    ("artifact", HelpTier::Standard, "Alias for artifacts — list or fetch team task artifacts by team and task id."),
    ("artifacts", HelpTier::Standard, "List or fetch team task artifacts; use team status for active task state."),
    ("assign", HelpTier::Standard, "Fetch a GitHub issue and wake an oracle with its task prompt; use work for repo-first work."),
    ("attach-ssh", HelpTier::Extra, "Attach to a remote tmux session over SSH; use attach for local sessions."),
    ("audit", HelpTier::Extra, "Inspect native dispatch audit events with filters for anomalies, event name, time, or tail limit."),
    ("auth", HelpTier::Standard, "Sign and verify federation/auth payloads; use consent for human approval flows."),
    ("auto-pair-proof", HelpTier::Extra, "Build or verify auto-pair identity proofs for node/oracle trust bootstrapping."),
    ("auto-wake", HelpTier::Extra, "Plan whether a site should wake a target; use wake for direct oracle launch."),
    ("bind-host", HelpTier::Extra, "Explain how maw chooses a bind host from config, peer store, and local defaults."),
    ("break", HelpTier::Extra, "Break a pane out of the current window; use pane/take for higher-level movement."),
    ("buddy", HelpTier::Standard, "Alias for bud — create a new oracle workspace from this one."),
    ("capture", HelpTier::Standard, "Capture visible pane output or full scrollback; use peek for a quick latest-output glance."),
    ("census", HelpTier::Standard, "Render a fleet/node census snapshot; use agents or ls for live-pane detail."),
    ("channel", HelpTier::Standard, "Manage oracle channel plugins and repo/global channel config; wake auto-injects channels."),
    ("check", HelpTier::Standard, "Probe required and optional local tools such as bun, gh, ghq, git, tmux, uv, and uvx."),
    ("cleanup", HelpTier::Standard, "Clean zombie agent panes and prune stale Oracle registry entries."),
    ("codex", HelpTier::Extra, "Switch active per-account CODEX_HOME in .envrc; mirrors token use for Codex profiles."),
    ("commands", HelpTier::Standard, "Alias for help --all — show every registered command grouped by tier."),
    ("completions", HelpTier::Standard, "Generate shell completions and dynamic completion data for zsh, bash, fish, and scripts."),
    ("config", HelpTier::Standard, "Show, explain, and set maw configuration keys; use doctor for health checks."),
    ("consent", HelpTier::Standard, "List, approve, or reject pending consent requests; use consent-store for raw store views."),
    ("consent-approval", HelpTier::Extra, "Plan approve/reject consent outcomes with PIN verification and trust-entry creation."),
    ("consent-cleanup", HelpTier::Extra, "Delete a pending consent request from the store after validation."),
    ("consent-expiry", HelpTier::Extra, "Evaluate or expire a pending consent request at a given timestamp."),
    ("consent-pending-read", HelpTier::Extra, "Read one pending consent request by id without exposing the PIN."),
    ("consent-pending-status", HelpTier::Extra, "Update a pending consent request status to pending, approved, rejected, or expired."),
    ("consent-pin", HelpTier::Extra, "Hash or verify a normalized consent PIN; use consent-approval for approve/reject flows."),
    ("consent-request", HelpTier::Extra, "Create a pending peer consent request with summary, action, PIN, and expiry metadata."),
    ("consent-store", HelpTier::Extra, "Inspect or model trust and pending consent store entries; use consent for the human flow."),
    ("consent-trust-check", HelpTier::Extra, "Check whether a from:to:action consent trust tuple is currently approved."),
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
