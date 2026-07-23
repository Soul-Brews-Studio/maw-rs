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
    // ===== #648 fanout: consent-pair =====
    ("consent-trust-revoke", HelpTier::Other, ""),
    ("cross-team-queue", HelpTier::Other, ""),
    ("discord", HelpTier::Standard, "Manage Discord fleet credentials, access, status, routing, and service lifecycle."),
    ("discord-inv", HelpTier::Extra, "Inspect a Discord bot's guild and channel inventory, with optional threads and JSON output."),
    ("discover", HelpTier::Standard, "List configured and discovered federation peers, inventory sources, and live tmux state."),
    ("doctor", HelpTier::Standard, "Diagnose maw installation health and repair supported configuration problems."),
    ("federation", HelpTier::Standard, "Inspect and synchronize multi-node federation state; use pair to add a peer."),
    ("federation-health", HelpTier::Extra, "Compare local and remote federation reachability, peer sets, latency, and clock health."),
    ("federation-identity", HelpTier::Extra, "Render the node URL and oracle ownership map advertised to federation peers."),
    ("federation-sync", HelpTier::Extra, "Plan or apply federation identity differences; use federation for the grouped workflow."),
    ("find", HelpTier::Standard, "Search agents, sessions, and fleet metadata for matching oracle context."),
    ("fleet", HelpTier::Standard, "Manage the persistent fleet registry; use ls for currently live sessions."),
    ("focus", HelpTier::Standard, "Focus a tmux pane by target or direction; use inspect to verify pane metadata."),
    ("footer", HelpTier::Other, ""),
    ("forget", HelpTier::Extra, "Remove an oracle's local registry state with a dry-run preview before destructive cleanup."),
    ("forward-error", HelpTier::Other, ""),
    ("fuzzy", HelpTier::Other, ""),
    ("gather", HelpTier::Standard, "Gather team member panes into the lead window; use scatter to separate them again."),
    ("handover", HelpTier::Standard, "Alias for take — move a tmux window from one oracle session to another."),
    ("help", HelpTier::Standard, "Show the curated command menu; add --all for the full tiered verb catalog."),
    ("identity", HelpTier::Other, ""),
    ("inbox", HelpTier::Standard, "Read durable oracle messages and manage the cross-scope approval queue."),
    ("inspect", HelpTier::Standard, "Inspect pane metadata as text or JSON; use peek or capture for pane output."),
    ("join", HelpTier::Standard, "Join a source pane into a destination tmux window; use gather for whole teams."),
    ("learn", HelpTier::Standard, "Route codebase exploration to the oracle learning workflow, with fast or deep scope."),
    ("locate", HelpTier::Standard, "Locate an oracle's repo, session, fleet config, and federation node without side effects."),
    ("messages", HelpTier::Standard, "Query and operate the durable hey/message lifecycle ledger; use inbox for recipient work."),
    ("more", HelpTier::Other, ""),
    ("mv", HelpTier::Standard, "Renumber a tmux window within its session; use take to move it between sessions."),
    ("normalize", HelpTier::Other, ""),
    ("notify", HelpTier::Standard, "Persist a routine message to a recipient inbox without injecting text into its pane."),
    ("on", HelpTier::Other, ""),
    ("oracle-recruit", HelpTier::Extra, "Recruit an oracle into a squad through the consent-gated pairing flow."),
    ("oracle-skills", HelpTier::Extra, "Manage oracle skills across AI coding agents through arra-oracle-skills."),
    ("oracle-workon", HelpTier::Extra, "Spawn an oracle worktree team by composing wake task context, split, and swarm."),
    ("oracles", HelpTier::Standard, "Alias for oracle — list, scan, search, register, and inspect oracle records."),
    ("overview", HelpTier::Extra, "Open the paged tmux overview of fleet oracles; use ls for a compact live list."),
    ("pair", HelpTier::Standard, "Pair federation peers through an ephemeral-code handshake; use federation to inspect status."),
    ("pair-api", HelpTier::Other, ""),
    ("pair-api-auto", HelpTier::Other, ""),
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
