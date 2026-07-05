
    #[test]
    fn constructors_defaults_and_private_helpers_stay_deterministic() {
        assert_eq!(
            NewSessionOptions::default(),
            NewSessionOptions {
                window: None,
                cwd: None,
                detached: true,
                command: None,
                print_format: None,
            }
        );
        assert_eq!(
            TmuxSplitActionOptions::default(),
            TmuxSplitActionOptions {
                vertical: false,
                pct: 50.0,
                command: None,
            }
        );

        let mut tracker = TmuxSendTracker::default();
        tracker.set(
            "%1",
            SendTrackerEntry {
                last_ts: 10,
                count: 2,
                window_start: 1,
            },
        );
        assert_eq!(
            tracker.get("%1"),
            Some(SendTrackerEntry {
                last_ts: 10,
                count: 2,
                window_start: 1,
            })
        );
        tracker.clear();
        assert_eq!(tracker.get("%1"), None);

        let candidate = PaneTargetCandidate {
            name: "pulse".to_owned(),
            resolved: "%7".to_owned(),
            source: "pane-title".to_owned(),
            target: "pulse:1.0".to_owned(),
        };
        assert_eq!(candidate.name(), "pulse");
        assert_eq!(TmuxError::new("boom").to_string(), "boom");
    }

    #[test]
    fn local_client_constructors_build_tmux_runner_without_executing_tmux() {
        let local = TmuxClient::local();
        assert_eq!(
            local.runner.argv("display-message", &[]),
            vec![OsString::from("tmux"), OsString::from("display-message")]
        );

        let with_socket = TmuxClient::local_with_socket("/tmp/maw.sock");
        assert_eq!(
            with_socket.runner.argv("display-message", &[]),
            vec![
                OsString::from("tmux"),
                OsString::from("-S"),
                OsString::from("/tmp/maw.sock"),
                OsString::from("display-message"),
            ]
        );
    }

    #[test]
    fn list_all_parses_runner_output_in_coverage_gap_module() {
        struct ListAllRunner;

        impl TmuxRunner for ListAllRunner {
            fn run(&mut self, subcommand: &str, _args: &[String]) -> Result<String, TmuxError> {
                assert_eq!(subcommand, "list-windows");
                Ok("demo|||1|||work|||1|||/tmp/demo\n".to_owned())
            }
        }

        let mut client = TmuxClient::new(ListAllRunner);

        assert_eq!(
            client.list_all(),
            vec![TmuxSession {
                name: "demo".to_owned(),
                windows: vec![TmuxWindow {
                    index: 1,
                    name: "work".to_owned(),
                    active: true,
                    cwd: Some("/tmp/demo".to_owned()),
                }],
            }]
        );
    }

    #[test]
    fn command_runner_handles_success_stdin_and_failure_details() {
        let mut runner = CommandTmuxRunner::with_program("sh");

        assert_eq!(
            runner
                .run("-c", &["printf ok".to_owned()])
                .expect("shell printf succeeds"),
            "ok"
        );
        assert_eq!(
            runner
                .run_with_stdin("-c", &["cat".to_owned()], b"stdin payload")
                .expect("shell cat echoes stdin"),
            "stdin payload"
        );

        let stderr_error = runner
            .run("-c", &["printf boom >&2; exit 7".to_owned()])
            .expect_err("non-zero shell exit includes stderr");
        assert_eq!(stderr_error.message, "tmux exited with status 7: boom");

        let empty_error = runner
            .run("-c", &["exit 5".to_owned()])
            .expect_err("non-zero shell exit without output includes status");
        assert_eq!(empty_error.message, "tmux exited with status 5");

        let signal_error = runner
            .run("-c", &["kill -TERM $$".to_owned()])
            .expect_err("terminated shell has no exit code");
        assert_eq!(signal_error.message, "tmux exited with status signal");
    }

    #[test]
    fn command_runner_reports_broken_pipe_when_child_closes_stdin() {
        let mut runner = CommandTmuxRunner::with_program("sh");
        let payload = vec![b'x'; 16 * 1024 * 1024];

        let error = runner
            .run_with_stdin("-c", &["exit 0".to_owned()], &payload)
            .expect_err("closed child stdin should surface write failure");

        assert!(
            error.message.contains("write stdin for"),
            "unexpected error: {}",
            error.message
        );
    }

    #[test]
    fn live_state_falls_back_for_non_standard_tmux_targets() {
        let result = resolve_tmux_live_state(
            &[],
            &[TmuxPane {
                id: "%9".to_owned(),
                command: "zsh".to_owned(),
                target: "scratch-session:broken-target".to_owned(),
                title: "scratch".to_owned(),
                pid: None,
                cwd: None,
                last_activity: None,
            }],
        );

        assert_eq!(result.live[0].session, "scratch-session");
        assert_eq!(result.live[0].window, "");
        assert_eq!(result.live[0].pane, "");
        assert_eq!(fallback_target_parts("bare-session").session, "bare-session");
    }

    #[test]
    fn live_state_match_labels_fall_back_to_node_and_oracle() {
        let peers = vec![
            maw_peer::PeerTarget {
                name: None,
                url: "http://node".to_owned(),
                source: maw_peer::PeerSourceKind::Scout,
                node: Some("scratch".to_owned()),
                oracle: None,
            },
            maw_peer::PeerTarget {
                name: None,
                url: "http://oracle".to_owned(),
                source: maw_peer::PeerSourceKind::Scout,
                node: None,
                oracle: Some("scratch".to_owned()),
            },
        ];
        let result = resolve_tmux_live_state(
            &peers,
            &[TmuxPane {
                id: "%10".to_owned(),
                command: "zsh".to_owned(),
                target: "demo:1.0".to_owned(),
                title: "scratch".to_owned(),
                pid: None,
                cwd: None,
                last_activity: None,
            }],
        );

        assert_eq!(result.live[0].matches, vec!["scratch", "scratch"]);
    }
