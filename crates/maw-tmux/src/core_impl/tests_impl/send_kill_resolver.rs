    #[test]
    fn tmux_attach_recovery_candidates_and_choices_match_maw_js() {
        let cloned_repos = vec![
            "/opt/Code/github.com/Soul-Brews-Studio/pulse-oracle".to_owned(),
            "/opt/Code/github.com/Soul-Brews-Studio/pulse-helper-oracle".to_owned(),
            "/opt/Code/github.com/Org/sleeping-oracle".to_owned(),
        ];
        assert_eq!(
            wake_arg_for_similar_oracle("pulse-oracle"),
            "pulse".to_owned()
        );
        assert_eq!(
            wake_arg_for_similar_oracle("Soul-Brews-Studio/pulse-oracle"),
            "Soul-Brews-Studio/pulse-oracle".to_owned()
        );

        let candidates = attach_recovery_candidates(
            "pulse",
            "ghost",
            "session-name",
            &[],
            &["/opt/Code/github.com/Soul-Brews-Studio/pulse-oracle".to_owned()],
        );
        assert_eq!(
            candidates,
            vec![AttachRecoveryCandidate {
                oracle: "Soul-Brews-Studio/pulse-oracle".to_owned(),
                label: "Soul-Brews-Studio/pulse-oracle".to_owned(),
            }]
        );
        assert_eq!(
            decide_attach_recovery(&candidates, false, None),
            AttachRecoveryDecision::AutoWake {
                command: SpawnCommand {
                    program: "maw".to_owned(),
                    args: vec![
                        "wake".to_owned(),
                        "Soul-Brews-Studio/pulse-oracle".to_owned(),
                        "-a".to_owned()
                    ],
                },
                label: "Soul-Brews-Studio/pulse-oracle".to_owned(),
            }
        );

        let candidates = attach_recovery_candidates(
            "44-sleeping",
            "44-sleeping",
            "fleet-stem (44-sleeping)",
            &[AttachRecoveryFleetEntry {
                session: "44-sleeping".to_owned(),
                first_window_name: Some("sleeping-oracle".to_owned()),
                repo: Some("Org/sleeping-oracle".to_owned()),
            }],
            &cloned_repos,
        );
        assert_eq!(
            candidates[0],
            AttachRecoveryCandidate {
                oracle: "sleeping".to_owned(),
                label: "sleeping-oracle (cloned)".to_owned(),
            }
        );

        let candidates =
            attach_recovery_candidates("pulse", "pulse", "session-name", &[], &cloned_repos);
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            decide_attach_recovery(&candidates, false, None),
            AttachRecoveryDecision::PrintCandidates {
                candidates: candidates.clone()
            }
        );
        assert_eq!(
            decide_attach_recovery(&candidates, true, None),
            AttachRecoveryDecision::Prompt {
                candidates: candidates.clone()
            }
        );
        assert_eq!(
            decide_attach_recovery(&candidates, true, Some(2)),
            AttachRecoveryDecision::WakeChoice {
                command: SpawnCommand {
                    program: "maw".to_owned(),
                    args: vec![
                        "wake".to_owned(),
                        "Soul-Brews-Studio/pulse-helper-oracle".to_owned(),
                        "-a".to_owned()
                    ],
                }
            }
        );
        assert_eq!(
            decide_attach_recovery(&candidates, true, Some(3)),
            AttachRecoveryDecision::InvalidChoice
        );
        assert_eq!(
            decide_attach_recovery(&[], true, None),
            AttachRecoveryDecision::NoCandidates
        );
    }

    #[test]
    fn tmux_send_tracker_matches_maw_js_cooldown_and_quota_gate() {
        let mut tracker = TmuxSendTracker::default();
        assert_eq!(tracker.check("%1", 1_000, false), SendThrottle::Allowed);
        assert_eq!(
            tracker.check("%1", 1_100, false),
            SendThrottle::Cooldown { cooldown_ms: 500 }
        );
        assert_eq!(tracker.check("%1", 1_600, false), SendThrottle::Allowed);
        assert_eq!(
            tracker.get("%1"),
            Some(SendTrackerEntry {
                last_ts: 1_600,
                count: 2,
                window_start: 1_000,
            })
        );

        tracker.set(
            "%2",
            SendTrackerEntry {
                last_ts: 10_000,
                count: 100,
                window_start: 0,
            },
        );
        assert_eq!(
            tracker.check("%2", 11_000, false),
            SendThrottle::Quota {
                quota_per_minute: 100
            }
        );
        assert_eq!(tracker.check("%2", 61_001, false), SendThrottle::Allowed);
        assert_eq!(
            tracker.get("%2"),
            Some(SendTrackerEntry {
                last_ts: 61_001,
                count: 1,
                window_start: 61_001,
            })
        );

        tracker.set(
            "%3",
            SendTrackerEntry {
                last_ts: 20_000,
                count: 100,
                window_start: 0,
            },
        );
        assert_eq!(tracker.check("%3", 20_001, true), SendThrottle::Allowed);
        assert_eq!(
            tracker.get("%3"),
            Some(SendTrackerEntry {
                last_ts: 20_000,
                count: 100,
                window_start: 0,
            })
        );
    }

    #[test]
    fn tmux_send_action_gates_and_args_match_maw_js_cases() {
        assert_eq!(
            tmux_send_command_args("%1", "echo hello", false),
            vec!["-t", "%1", "echo hello", "Enter"]
        );
        assert_eq!(
            tmux_send_command_args("%1", "C-c", true),
            vec!["-t", "%1", "C-c"]
        );

        let runner = FakeRunner::with_responses(vec![Ok("bash\n"), Ok("")]);
        let mut client = TmuxClient::new(runner);
        let mut tracker = TmuxSendTracker::default();
        let outcome = client
            .send_command_to_pane(
                &mut tracker,
                "%1",
                "echo hello",
                &TmuxSendCommandOptions::default(),
                1_000,
            )
            .expect("send succeeds");
        assert_eq!(outcome, TmuxSendCommandOutcome::Sent);
        assert_eq!(client.runner.calls[0].0, "display-message");
        assert_eq!(
            client.runner.calls[0].1,
            vec!["-p", "-t", "%1", "#{pane_current_command}"]
        );
        assert_eq!(
            client.runner.calls[1],
            (
                "send-keys".to_owned(),
                vec!["-t", "%1", "echo hello", "Enter"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            )
        );

        let runner = FakeRunner::with_responses(vec![Ok("bash\n")]);
        let mut client = TmuxClient::new(runner);
        let mut tracker = TmuxSendTracker::default();
        let error = client
            .send_command_to_pane(
                &mut tracker,
                "%2",
                "rm -rf /tmp/junk",
                &TmuxSendCommandOptions::default(),
                2_000,
            )
            .expect_err("destructive command blocked");
        assert!(error.message.contains("refusing to send"));
        assert!(error.message.contains("--allow-destructive"));
        assert!(client.runner.calls.is_empty());

        let runner = FakeRunner::with_responses(vec![Ok("claude\n")]);
        let mut client = TmuxClient::new(runner);
        let mut tracker = TmuxSendTracker::default();
        let error = client
            .send_command_to_pane(
                &mut tracker,
                "%3",
                "echo hello",
                &TmuxSendCommandOptions::default(),
                3_000,
            )
            .expect_err("claude-like pane blocked");
        assert!(error.message.contains("claude-like"));
        assert_eq!(client.runner.calls.len(), 1);

        let runner = FakeRunner::with_responses(vec![Ok("claude\n"), Ok("")]);
        let mut client = TmuxClient::new(runner);
        let mut tracker = TmuxSendTracker::default();
        let outcome = client
            .send_command_to_pane(
                &mut tracker,
                "%4",
                "C-c",
                &TmuxSendCommandOptions {
                    literal: true,
                    allow_destructive: true,
                    force: true,
                },
                4_000,
            )
            .expect("force bypasses claude-like pane");
        assert_eq!(outcome, TmuxSendCommandOutcome::Sent);
        assert_eq!(
            client.runner.calls[1].1,
            vec!["-t", "%4", "C-c"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn pane_target_resolver_indexes_titles_roles_and_worktree_aliases() {
        let raw = [
            "%101|||47-mawjs:1.0|||codex-headless-demo-layout|||tile-1|||/opt/Code/github.com/Soul-Brews-Studio/mawjs-oracle.wt-7-codex-headless",
            "%202|||47-mawjs:1.1|||notes|||researcher|||/opt/Code/github.com/Soul-Brews-Studio/notes-oracle.wt-2-researcher",
        ]
        .join("\n");

        let names = pane_target_candidates_from_list_panes_output(&raw)
            .into_iter()
            .map(|candidate| {
                format!(
                    "{}:{}:{}",
                    candidate.name, candidate.source, candidate.resolved
                )
            })
            .collect::<Vec<_>>();

        assert!(names.contains(&"codex-headless-demo-layout:pane-title:%101".to_owned()));
        assert!(names.contains(&"tile-1:tile-role:%101".to_owned()));
        assert!(names.contains(&"codex-headless:worktree-role:%101".to_owned()));
        assert!(names.contains(&"mawjs-codex-headless:worktree-alias:%101".to_owned()));

        let hit = resolve_pane_target_from_list_panes_output("mawjs-codex-headless", &raw);
        assert_eq!(
            hit,
            PaneTargetResolution::Match {
                candidate: PaneTargetCandidate {
                    name: "mawjs-codex-headless".to_owned(),
                    resolved: "%101".to_owned(),
                    source: "worktree-alias".to_owned(),
                    target: "47-mawjs:1.0".to_owned(),
                }
            }
        );

        let hit = resolve_pane_target_from_list_panes_output("codex-headless-demo-layout", &raw);
        assert_eq!(
            hit,
            PaneTargetResolution::Match {
                candidate: PaneTargetCandidate {
                    name: "codex-headless-demo-layout".to_owned(),
                    resolved: "%101".to_owned(),
                    source: "pane-title".to_owned(),
                    target: "47-mawjs:1.0".to_owned(),
                }
            }
        );
    }

    #[test]
    fn pane_target_resolver_keeps_ambiguous_matches_safe() {
        let raw = [
            "%1|||47-mawjs:1.0|||codex-a|||worker|||/tmp/mawjs-oracle.wt-1-codex",
            "%2|||47-mawjs:1.1|||codex-b|||worker|||/tmp/mawjs-oracle.wt-2-codex",
        ]
        .join("\n");
        let hit = resolve_pane_target_from_list_panes_output("worker", &raw);
        let debug = format!("{hit:?}");
        assert!(debug.starts_with("Ambiguous"));
        assert!(debug.contains("resolved: \"%1\""));
        assert!(debug.contains("resolved: \"%2\""));

        let candidates = vec![
            PaneTargetCandidate {
                name: "fleet-alpha".to_owned(),
                resolved: "%1".to_owned(),
                source: "pane-title".to_owned(),
                target: "s:1.1".to_owned(),
            },
            PaneTargetCandidate {
                name: "one-view".to_owned(),
                resolved: "%2".to_owned(),
                source: "pane-title".to_owned(),
                target: "s:1.2".to_owned(),
            },
            PaneTargetCandidate {
                name: "two-view".to_owned(),
                resolved: "%3".to_owned(),
                source: "pane-title".to_owned(),
                target: "s:1.3".to_owned(),
            },
        ];
        assert_eq!(
            resolve_pane_target_from_candidates("alpha", &candidates),
            PaneTargetResolution::Match {
                candidate: candidates[0].clone()
            }
        );
        assert_eq!(
            resolve_pane_target_from_candidates("view", &candidates),
            PaneTargetResolution::Ambiguous {
                candidates: vec![candidates[1].clone(), candidates[2].clone()]
            }
        );
    }

    #[test]
    fn tmux_kill_action_refuses_fleet_and_force_kills_session() {
        let runner = FakeRunner::with_responses(vec![Ok("")]);
        let mut client = TmuxClient::new(runner);
        let fleet = BTreeSet::from(["101-mawjs".to_owned()]);
        let target = TmuxKillTarget {
            resolved: "101-mawjs:0.1".to_owned(),
            source: "session:w.p".to_owned(),
        };

        let error = client
            .kill_target_action(&target, &fleet, &TmuxKillCommandOptions::default())
            .expect_err("fleet session protected");
        assert!(error
            .message
            .contains("refusing to kill: session '101-mawjs' is fleet or view"));
        assert!(client.runner.calls.is_empty());

        let outcome = client
            .kill_target_action(
                &target,
                &fleet,
                &TmuxKillCommandOptions {
                    force: true,
                    session: true,
                },
            )
            .expect("forced session kill succeeds");
        assert_eq!(
            outcome,
            TmuxKillOutcome::Session {
                session: "101-mawjs".to_owned()
            }
        );
        assert_eq!(
            client.runner.calls,
            vec![(
                "kill-session".to_owned(),
                vec!["-t".to_owned(), "101-mawjs".to_owned()]
            )]
        );
    }

    #[test]
    fn tmux_kill_action_uses_orphan_pane_fallback_and_wraps_errors() {
        let raw = "%101|||scratch:0.0|||worker|||tile-a|||/tmp/repo.wt-1-scout\n";
        let target =
            resolve_kill_target_with_pane_fallback("scout", "scout", "session-name", false, raw)
                .expect("fallback target");
        assert_eq!(
            target,
            TmuxKillTarget {
                resolved: "%101".to_owned(),
                source: "worktree-role (scout)".to_owned(),
            }
        );

        let runner = FakeRunner::with_responses(vec![Ok("")]);
        let mut client = TmuxClient::new(runner);
        let outcome = client
            .kill_target_action(
                &target,
                &BTreeSet::new(),
                &TmuxKillCommandOptions::default(),
            )
            .expect("pane kill succeeds");
        assert_eq!(
            outcome,
            TmuxKillOutcome::Pane {
                target: "%101".to_owned()
            }
        );
        assert_eq!(
            client.runner.calls,
            vec![(
                "kill-pane".to_owned(),
                vec!["-t".to_owned(), "%101".to_owned()]
            )]
        );

        let runner = FakeRunner::with_responses(vec![Err(TmuxError::new("kill denied"))]);
        let mut client = TmuxClient::new(runner);
        let error = client
            .kill_target_action(
                &target,
                &BTreeSet::new(),
                &TmuxKillCommandOptions::default(),
            )
            .expect_err("kill failure wrapped");
        assert_eq!(
            error.message,
            "kill failed for '%101' (from worktree-role (scout)): kill denied"
        );
    }
