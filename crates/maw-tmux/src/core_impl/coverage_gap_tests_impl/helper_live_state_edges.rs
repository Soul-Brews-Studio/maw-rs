
    use super::*;

    #[derive(Default)]
    struct RecordingRunner {
        calls: Vec<(String, Vec<String>)>,
    }

    impl TmuxRunner for RecordingRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            Ok(String::new())
        }
    }

    #[test]
    fn tag_pane_writes_title_before_metadata() {
        let runner = RecordingRunner::default();
        let mut client = TmuxClient::new(runner);

        client
            .tag_pane(
                "%1",
                Some("pulse"),
                &[("role".to_owned(), "worker".to_owned())],
            )
            .expect("tag pane");

        assert_eq!(client.runner.calls.len(), 2);
        assert_eq!(
            client.runner.calls[0],
            (
                "select-pane".to_owned(),
                vec!["-t", "%1", "-T", "pulse"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            )
        );
        assert_eq!(client.runner.calls[1].0, "set-option");
        assert!(client.runner.calls[1].1.contains(&"@role".to_owned()));
    }

    #[test]
    fn ansi_stripper_preserves_unknown_escape_and_removes_uppercase_csi() {
        assert_eq!(strip_tmux_ansi("a\u{1b}[31mb"), "ab");
        assert_eq!(strip_tmux_ansi("a\u{1b}[2Jb"), "ab");
        assert_eq!(strip_tmux_ansi("a\u{1b}[?25lb"), "a\u{1b}[?25lb");
    }

    #[test]
    fn version_and_duration_helpers_reject_empty_and_unknown_units() {
        assert!(!is_claude_like_pane(Some("")));
        assert_eq!(parse_active_duration_seconds(Some("5w")), None);
        assert_eq!(
            active_duration_arg(&["--active=5w".to_owned()], "--active"),
            None
        );
        assert_eq!(
            active_duration_arg(&["--active=2h".to_owned()], "--active"),
            Some("2h".to_owned())
        );
    }

    #[test]
    fn attach_recovery_uses_uncloned_fleet_window_label_and_dedupes_similar_repo() {
        let candidates = attach_recovery_candidates(
            "pulse",
            "44-pulse",
            "fleet-window",
            &[AttachRecoveryFleetEntry {
                session: "44-pulse".to_owned(),
                first_window_name: Some("pulse-oracle".to_owned()),
                repo: Some("Soul-Brews-Studio/pulse-oracle".to_owned()),
            }],
            &[],
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0],
            AttachRecoveryCandidate {
                oracle: "pulse".to_owned(),
                label: "pulse-oracle (not cloned)".to_owned(),
            }
        );
    }

    #[test]
    fn worktree_cwd_names_include_role_alias_for_oracle_repos() {
        assert_eq!(
            worktree_names_from_cwd("/tmp/mawjs-oracle.wt-1-executor"),
            vec![
                (
                    "mawjs-oracle.wt-1-executor".to_owned(),
                    "worktree-dir".to_owned()
                ),
                ("executor".to_owned(), "worktree-role".to_owned()),
                ("mawjs-executor".to_owned(), "worktree-alias".to_owned()),
            ]
        );
    }

    #[test]
    fn helper_edges_cover_missing_fleet_entry_empty_role_and_bad_duration_forms() {
        assert_eq!(
            attach_recovery_candidates(
                "pulse",
                "missing-session",
                "fleet-window",
                &[AttachRecoveryFleetEntry {
                    session: "other".to_owned(),
                    first_window_name: Some("pulse-oracle".to_owned()),
                    repo: None,
                }],
                &[],
            ),
            Vec::<AttachRecoveryCandidate>::new()
        );
        assert_eq!(
            worktree_names_from_cwd("/tmp/mawjs-oracle.wt-1"),
            vec![("mawjs-oracle.wt-1".to_owned(), "worktree-dir".to_owned())]
        );
        assert_eq!(parse_active_duration_seconds(Some("0m")), None);
        assert_eq!(
            parse_active_duration_seconds(Some("999999999999999999999999999999m")),
            None
        );
        assert_eq!(
            active_duration_arg(&["--active".to_owned(), "--bad".to_owned()], "--active"),
            None
        );
        assert_eq!(
            active_duration_arg(&["--active=bad".to_owned()], "--active"),
            None
        );
    }

    #[test]
    fn session_created_formats_zero_and_valid_epoch() {
        assert_eq!(format_session_created(None), "—");
        assert_eq!(format_session_created(Some(0)), "—");
        assert_eq!(
            format_session_created(Some(1_700_000_000)),
            "2023-11-14 22:13:20"
        );
    }

    #[test]
    fn nested_agents_worktree_cwd_names_match_legacy_aliases() {
        assert_eq!(
            worktree_names_from_cwd("/tmp/mawjs-oracle/agents/1-executor"),
            vec![
                ("1-executor".to_owned(), "worktree-dir".to_owned()),
                ("executor".to_owned(), "worktree-role".to_owned()),
                ("mawjs-executor".to_owned(), "worktree-alias".to_owned()),
            ]
        );
        assert_eq!(
            worktree_names_from_cwd("/tmp/mawjs-oracle/agents/codex"),
            vec![
                ("codex".to_owned(), "worktree-dir".to_owned()),
                ("codex".to_owned(), "worktree-role".to_owned()),
                ("mawjs-codex".to_owned(), "worktree-alias".to_owned()),
            ]
        );
    }

    #[test]
    fn tag_pane_title_error_propagates_before_metadata() {
        struct FailingTitleRunner;

        impl TmuxRunner for FailingTitleRunner {
            fn run(&mut self, subcommand: &str, _args: &[String]) -> Result<String, TmuxError> {
                assert_eq!(subcommand, "select-pane");
                Err(TmuxError::new("title failed"))
            }
        }

        let mut client = TmuxClient::new(FailingTitleRunner);
        let error = client
            .tag_pane(
                "%1",
                Some("pulse"),
                &[("role".to_owned(), "worker".to_owned())],
            )
            .expect_err("title failure should stop tagging");
        assert_eq!(error.message, "title failed");
    }

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

    #[test]
    fn live_state_match_labels_use_oracle_and_empty_cwd_is_ignored() {
        let peers = vec![maw_peer::PeerTarget {
            name: None,
            url: "http://scratch".to_owned(),
            source: maw_peer::PeerSourceKind::Scout,
            node: None,
            oracle: Some("scratch".to_owned()),
        }];
        let result = resolve_tmux_live_state(
            &peers,
            &[TmuxPane {
                id: "%11".to_owned(),
                command: "zsh".to_owned(),
                target: "demo:1.0".to_owned(),
                title: "scratch".to_owned(),
                pid: None,
                cwd: Some("////".to_owned()),
                last_activity: None,
            }],
        );

        assert_eq!(result.live[0].matches, vec!["scratch"]);
        assert_eq!(path_basename("////"), None);
    }

    #[test]
    fn mark_peer_targets_live_reports_targets_sessions_and_cwd_matches() {
        let peers = vec![maw_peer::PeerTarget {
            name: Some("scratch".to_owned()),
            url: "http://scratch".to_owned(),
            source: maw_peer::PeerSourceKind::Config,
            node: Some("node-a".to_owned()),
            oracle: Some("oracle-a".to_owned()),
        }];
        let live = vec![
            DiscoverLivePane {
                source: "tmux".to_owned(),
                id: "%1".to_owned(),
                target: "05-scratch:1.0".to_owned(),
                session: "05-scratch".to_owned(),
                window: "main".to_owned(),
                pane: "0".to_owned(),
                command: Some("zsh".to_owned()),
                title: None,
                pid: None,
                cwd: Some("/tmp/scratch".to_owned()),
                last_activity: None,
                awake: true,
                matches: Vec::new(),
            },
            DiscoverLivePane {
                source: "tmux".to_owned(),
                id: "%2".to_owned(),
                target: "other:1.0".to_owned(),
                session: "other".to_owned(),
                window: "scratch".to_owned(),
                pane: "0".to_owned(),
                command: Some("zsh".to_owned()),
                title: None,
                pid: None,
                cwd: None,
                last_activity: None,
                awake: true,
                matches: Vec::new(),
            },
        ];

        let marked = mark_peer_targets_live(&peers, &live);

        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].name, Some("scratch".to_owned()));
        assert_eq!(marked[0].url, "http://scratch");
        assert_eq!(marked[0].source, maw_peer::PeerSourceKind::Config);
        assert_eq!(marked[0].node, Some("node-a".to_owned()));
        assert_eq!(marked[0].oracle, Some("oracle-a".to_owned()));
        assert!(marked[0].awake);
        assert_eq!(
            marked[0].live_targets,
            vec!["05-scratch:1.0".to_owned(), "other:1.0".to_owned()]
        );
        assert_eq!(
            marked[0].live_sessions,
            vec!["05-scratch".to_owned(), "other".to_owned()]
        );
    }

    #[test]
    fn io_error_formatter_includes_action_program_and_error() {
        let error = tmux_program_io_error(
            "collect output from",
            std::ffi::OsStr::new("tmux"),
            &std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe closed"),
        );

        assert!(error.message.contains("failed to collect output from tmux"));
        assert!(error.message.contains("pipe closed"));
    }

    #[test]
    fn command_runner_writes_stdin_and_collects_stdout() {
        let mut runner = CommandTmuxRunner::with_program("/bin/cat");

        let output = runner
            .run_with_stdin("-", &[], b"hello from stdin")
            .expect("cat should echo stdin");

        assert_eq!(output, "hello from stdin");
    }
