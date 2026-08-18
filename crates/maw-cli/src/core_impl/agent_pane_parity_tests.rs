// #813 parity harness: every agent-pane predicate must give the same verdict.
//
// This file is the deliverable that outlives the fix. The bug was not that one
// predicate was wrong -- it was that nine of them existed and nobody could see
// them disagree. Adding a tenth copy, or "just tweaking" one arm of one copy,
// now fails here with the exact (command, title) pair that split them.
//
// Two columns, because the signatures genuinely differ and that is fine:
//   * `expect_command` -- what every COMMAND-ONLY site must say. Those sites
//     never see a title, so a title-only match is a miss for them by design,
//     not a divergence.
//   * `expect_pane` -- what every (command, title) site must say.
//
// Deliberately NOT in the table (see the #813 report):
//   * `token_apply_is_claude_pane` -- `maw token apply` types a Claude
//     credential; it is Claude-specific on purpose and must not widen to
//     codex/gemini panes.
//   * `view_find_zombie_panes` -- a SAFETY kill-selector. There, matching
//     means "kill this pane", so widening it kills more. Left narrow.
//   * `is_claude_like_pane` -- the base arm `is_agent_pane_command` composes,
//     not a peer of it; it is intentionally narrower.

#[cfg(test)]
mod agent_pane_parity_tests {
    use super::*;
    use crate::serve_core::modules::agent_routes::agents_is_agent_pane;
    use crate::serve_core::modules::agent_status::agentstatus_is_agent_command;
    use crate::serve_core::ServecoreAgentPane;

    struct ParityCase {
        command: &'static str,
        title: &'static str,
        expect_command: bool,
        expect_pane: bool,
        why: &'static str,
    }

    const PARITY_CASES: &[ParityCase] = &[
        // --- the case that is broken today (#813) ---------------------------
        ParityCase {
            command: "2.1.233",
            title: "session-handoff-forward",
            expect_command: true,
            expect_pane: true,
            why: "busy Claude Code pane: version string as command, task line as title, no keyword anywhere",
        },
        ParityCase {
            command: "2.1.233",
            title: "neo-session-253-maw-js-arra-rebrand",
            expect_command: true,
            expect_pane: true,
            why: "busy oracle pane observed on m5 -- fails every title-led copy",
        },
        ParityCase {
            command: "2.1.233",
            title: "View file listing with maw command",
            expect_command: true,
            expect_pane: true,
            why: "busy pane whose task line reads like ordinary English",
        },
        ParityCase {
            command: "2.1.233",
            title: "Claude Code",
            expect_command: true,
            expect_pane: true,
            why: "IDLE Claude Code -- the only state the title arm ever caught",
        },
        ParityCase {
            command: "10.0.1",
            title: "some task status line",
            expect_command: true,
            expect_pane: true,
            why: "two-digit major still a three-part numeric version",
        },
        // --- version shapes that must NOT count -----------------------------
        ParityCase {
            command: "2.1",
            title: "some task",
            expect_command: false,
            expect_pane: false,
            why: "two segments is not a version-shaped command",
        },
        ParityCase {
            command: "v2.1.219",
            title: "some task",
            expect_command: false,
            expect_pane: false,
            why: "leading v is not a bare numeric version",
        },
        ParityCase {
            command: "2..1",
            title: "some task",
            expect_command: false,
            expect_pane: false,
            why: "empty middle segment",
        },
        ParityCase {
            command: "2.0.0-alpha.105",
            title: "some task",
            expect_command: false,
            expect_pane: false,
            why: "four segments -- pinned so the copies that accepted 4+ stay narrowed",
        },
        // --- the other engines ----------------------------------------------
        ParityCase {
            command: "claude",
            title: "some task status line",
            expect_command: true,
            expect_pane: true,
            why: "the launch path taken on host black (19/19 panes)",
        },
        ParityCase {
            command: "codex",
            title: "shell",
            expect_command: true,
            expect_pane: true,
            why: "codex is distinctive enough to match anywhere in the command",
        },
        ParityCase {
            command: "omx",
            title: "shell",
            expect_command: true,
            expect_pane: true,
            why: "omx is now exact-program in the explicit registry",
        },
        ParityCase {
            command: "opencode",
            title: "shell",
            expect_command: true,
            expect_pane: true,
            why: "opencode is now exact-program in the explicit registry",
        },
        ParityCase {
            command: "xomx",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "exact omx should not match substring",
        },
        ParityCase {
            command: "opencode-helper",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "exact opencode should not match substring",
        },
        ParityCase {
            command: "node",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "known false negative: `omx --direct` reports `node` in argv0, so this stays a warning",
        },
        ParityCase {
            command: "nodejs",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "Debian's package name for the same interpreter; out for the same reason as \
                  `node` itself",
        },
        ParityCase {
            command: "bun",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "known false negative: maw-js-daemon under bun reports `bun` in argv0",
        },
        ParityCase {
            command: "bun run dev",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "generic runtime command-line from argv0 should stay out",
        },
        ParityCase {
            command: "python3.12",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "generic runtime variant should stay denied",
        },
        ParityCase {
            command: "npx",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "generic runtime command-line from argv0 should stay out",
        },
        ParityCase {
            command: "uv",
            title: "building the thing",
            expect_command: false,
            expect_pane: false,
            why: "generic runtime command-line from argv0 should stay out",
        },
        ParityCase {
            command: "node",
            title: "delivered to a replaced pane",
            expect_command: false,
            expect_pane: false,
            why: "SYMPTOM at serve_agent_pane_guard, where matching SUPPRESSES: false here means \
                  the #709 'likely misaddressed' warning FIRES, which is what a node process \
                  standing where an agent used to be should produce",
        },
        ParityCase {
            command: "gemini",
            title: "building the thing",
            expect_command: true,
            expect_pane: true,
            why: "gemini as the program actually running",
        },
        // --- generic words that must stay OUT -------------------------------
        ParityCase {
            command: "nodemon",
            title: "watching files",
            expect_command: false,
            expect_pane: false,
            why: "generic launcher names match argv0 only -- pinned by agent_status's own test",
        },
        ParityCase {
            command: "geminix",
            title: "watching files",
            expect_command: false,
            expect_pane: false,
            why: "same rule, pinned by agent_status's configured-bins test",
        },
        ParityCase {
            command: "bash",
            title: "nat-LOCAL (shell)",
            expect_command: false,
            expect_pane: false,
            why: "a plain shell is the thing the misroute warning exists to name",
        },
        ParityCase {
            command: "sudo",
            title: "MAWRS-REMOTE (live, read-only)",
            expect_command: false,
            expect_pane: false,
            why: "read-only viewer pane, not an agent",
        },
        ParityCase {
            command: "vim",
            title: "editing lib.rs",
            expect_command: false,
            expect_pane: false,
            why: "an editor is not an agent",
        },
        // A KNOWN, ACCEPTED false positive of the title arm, pinned so it is a
        // documented cost rather than a surprise: a human editing agent code
        // has "agent" in the pane title. This is the irreducible weakness of
        // matching free-form text, and the reason the command arm leads --
        // every site errs the same way here instead of each erring differently.
        ParityCase {
            command: "vim",
            title: "editing agent_routes.rs",
            expect_command: false,
            expect_pane: true,
            why: "title arm cannot distinguish working ON an agent from being one",
        },
        ParityCase {
            command: "",
            title: "",
            expect_command: false,
            expect_pane: false,
            why: "empty pane data decides nothing",
        },
        // --- title-only corroboration ---------------------------------------
        ParityCase {
            command: "bash",
            title: "kru32-oracle",
            expect_command: false,
            expect_pane: true,
            why: "title arm still catches an unrecognised wrapper -- command-only sites cannot see it",
        },
        ParityCase {
            command: "zsh",
            title: "maw-agent-3",
            expect_command: false,
            expect_pane: true,
            why: "same, via the agent keyword",
        },
    ];

    fn parity_servecore_pane(command: &str, title: &str) -> ServecoreAgentPane {
        ServecoreAgentPane {
            id: "%1".to_owned(),
            command: command.to_owned(),
            target: "session:0.0".to_owned(),
            title: title.to_owned(),
            cwd: None,
            pid: None,
            last_activity: None,
        }
    }

    #[test]
    fn every_agent_pane_predicate_agrees_on_command_only_sites() {
        for case in PARITY_CASES {
            let sites: [(&str, bool); 6] = [
                (
                    "maw_tmux::is_agent_pane_command",
                    maw_tmux::is_agent_pane_command(Some(case.command)),
                ),
                ("talkto_is_agent_command", talkto_is_agent_command(case.command)),
                ("tab_is_agent_command", tab_is_agent_command(case.command)),
                ("awaken_is_agent_command", awaken_is_agent_command(case.command)),
                ("is_ls_agent_command", is_ls_agent_command(case.command)),
                (
                    "agentstatus_is_agent_command",
                    agentstatus_is_agent_command(case.command, &[]),
                ),
            ];
            for (site, verdict) in sites {
                assert_eq!(
                    verdict, case.expect_command,
                    "#813 divergence: {site}({:?}) = {verdict}, expected {} -- {}",
                    case.command, case.expect_command, case.why
                );
            }
        }
    }

    #[test]
    fn every_agent_pane_predicate_agrees_on_command_and_title_sites() {
        for case in PARITY_CASES {
            let sites: [(&str, bool); 4] = [
                (
                    "maw_tmux::is_agent_pane",
                    maw_tmux::is_agent_pane(Some(case.command), Some(case.title)),
                ),
                (
                    "route_pane_looks_like_agent",
                    route_pane_looks_like_agent(case.command, case.title),
                ),
                (
                    "serve_pane_looks_like_agent",
                    serve_pane_looks_like_agent(case.command, case.title),
                ),
                (
                    "agents_is_agent_pane",
                    agents_is_agent_pane(&parity_servecore_pane(case.command, case.title)),
                ),
            ];
            for (site, verdict) in sites {
                assert_eq!(
                    verdict, case.expect_pane,
                    "#813 divergence: {site}({:?}, {:?}) = {verdict}, expected {} -- {}",
                    case.command, case.title, case.expect_pane, case.why
                );
            }
        }
    }

    /// The exact regression #813 reported, spelled out on its own so a failure
    /// names the bug rather than a table row index.
    #[test]
    fn busy_claude_pane_with_no_keyword_in_title_is_an_agent_everywhere() {
        let command = "2.1.233";
        let title = "session-handoff-forward";
        assert!(
            route_pane_looks_like_agent(command, title),
            "maw hey warned 'not an obvious agent pane' at a live Claude pane (#813)"
        );
        assert!(talkto_is_agent_command(command), "maw talk-to refused a live Claude pane (#813)");
        assert!(tab_is_agent_command(command), "maw tab send refused a live Claude pane (#813)");
        assert!(awaken_is_agent_command(command), "maw awaken never saw the agent boot (#813)");
        assert!(maw_tmux::is_agent_pane(Some(command), Some(title)));
    }
}
