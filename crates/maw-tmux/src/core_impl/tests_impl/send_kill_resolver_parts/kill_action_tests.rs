
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
