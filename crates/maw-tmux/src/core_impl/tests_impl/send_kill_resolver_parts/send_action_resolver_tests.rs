
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
