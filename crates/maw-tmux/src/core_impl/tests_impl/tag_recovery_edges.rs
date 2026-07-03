    #[test]
    fn tmux_client_tag_pane_writes_title_and_normalized_metadata() {
        let runner = FakeRunner::with_responses(vec![Ok(""), Ok(""), Ok("")]);
        let mut client = TmuxClient::new(runner);

        client
            .tag_pane(
                "%2",
                Some("worker"),
                &[
                    ("role".to_owned(), "executor".to_owned()),
                    ("@node".to_owned(), "alpha".to_owned()),
                ],
            )
            .expect("tag writes");

        assert_eq!(
            client.runner.calls,
            vec![
                (
                    "select-pane".to_owned(),
                    vec![
                        "-t".to_owned(),
                        "%2".to_owned(),
                        "-T".to_owned(),
                        "worker".to_owned(),
                    ],
                ),
                (
                    "set-option".to_owned(),
                    vec![
                        "-p".to_owned(),
                        "-t".to_owned(),
                        "%2".to_owned(),
                        "@role".to_owned(),
                        "executor".to_owned(),
                    ],
                ),
                (
                    "set-option".to_owned(),
                    vec![
                        "-p".to_owned(),
                        "-t".to_owned(),
                        "%2".to_owned(),
                        "@node".to_owned(),
                        "alpha".to_owned(),
                    ],
                ),
            ]
        );

        let runner = FakeRunner::with_responses(vec![Ok("")]);
        let mut client = TmuxClient::new(runner);
        client
            .tag_pane("%3", None, &[("role".to_owned(), "observer".to_owned())])
            .expect("metadata-only tag writes");
        assert_eq!(client.runner.calls.len(), 1);
        assert_eq!(client.runner.calls[0].0, "set-option");
    }

    #[test]
    fn tmux_client_simple_query_and_tag_errors_propagate_runner_context() {
        let mut client = TmuxClient::new(FakeRunner::with_responses(vec![Err(TmuxError::new(
            "no windows",
        ))]));
        assert_eq!(
            client.list_windows("demo").expect_err("list error").message,
            "no windows"
        );

        let mut client = TmuxClient::new(FakeRunner::with_responses(vec![Err(TmuxError::new(
            "no command",
        ))]));
        assert_eq!(
            client
                .get_pane_command("%1")
                .expect_err("command error")
                .message,
            "no command"
        );

        let mut client = TmuxClient::new(FakeRunner::with_responses(vec![Err(TmuxError::new(
            "no info",
        ))]));
        assert_eq!(
            client.get_pane_info("%1").expect_err("info error").message,
            "no info"
        );

        let mut client = TmuxClient::new(FakeRunner::with_responses(vec![Err(TmuxError::new(
            "title denied",
        ))]));
        assert_eq!(
            client
                .tag_pane("%1", Some("title"), &[])
                .expect_err("title error")
                .message,
            "title denied"
        );

        let mut client = TmuxClient::new(FakeRunner::with_responses(vec![
            Ok(""),
            Err(TmuxError::new("meta denied")),
        ]));
        assert_eq!(
            client
                .tag_pane(
                    "%1",
                    Some("title"),
                    &[("role".to_owned(), "worker".to_owned())]
                )
                .expect_err("meta error")
                .message,
            "meta denied"
        );

        let mut client = TmuxClient::new(FakeRunner::with_responses(vec![Err(TmuxError::new(
            "no title",
        ))]));
        assert_eq!(
            client
                .read_pane_tags("%1")
                .expect_err("title read error")
                .message,
            "no title"
        );
    }

    #[test]
    fn fake_runner_no_response_and_resolution_none_paths_are_explicit() {
        let mut client = TmuxClient::new(FakeRunner::default());
        let error = client
            .list_windows("missing")
            .expect_err("empty fake runner reports no response");
        assert_eq!(error.message, "no response");

        let target = resolve_kill_target_with_pane_fallback(
            "ghost",
            "ghost",
            "session-name",
            false,
            "%1|||demo:1.1|||worker|||role|||/tmp/repo.wt-1-codex\n",
        )
        .expect("no pane fallback preserves session target");
        assert_eq!(
            target,
            TmuxKillTarget {
                resolved: "ghost".to_owned(),
                source: "session-name".to_owned(),
            }
        );

        assert_eq!(
            resolve_pane_target_from_candidates("ghost", &[]),
            PaneTargetResolution::None
        );
        assert_eq!(
            format_pane_ambiguity_error(
                "worker",
                &[
                    PaneTargetCandidate {
                        name: "worker".to_owned(),
                        resolved: "%1".to_owned(),
                        source: "pane-title".to_owned(),
                        target: String::new(),
                    },
                    PaneTargetCandidate {
                        name: "worker".to_owned(),
                        resolved: "%2".to_owned(),
                        source: "tile-role".to_owned(),
                        target: "demo:1.2".to_owned(),
                    },
                ],
            ),
            "'worker' is ambiguous — matches 2 panes:\n    • worker → %1 [pane-title]\n    • worker → %2 (demo:1.2) [tile-role]\n  use the pane id or full session:window.pane target"
        );
        assert_eq!(unescape_tmux_quoted_value("tail\\"), "tail\\");
    }

    #[test]
    fn attach_recovery_includes_fleet_window_clone_label_and_dedupes_repo_candidate() {
        let fleet_entries = vec![AttachRecoveryFleetEntry {
            session: "101-mawjs".to_owned(),
            first_window_name: Some("pulse-oracle".to_owned()),
            repo: Some("pulse-oracle".to_owned()),
        }];
        let cloned_repos = vec![
            "/opt/Code/github.com/Soul-Brews-Studio/pulse-oracle".to_owned(),
            "/opt/Code/github.com/Soul-Brews-Studio/pulse-helper-oracle".to_owned(),
        ];

        assert_eq!(
            attach_recovery_candidates(
                "pulse",
                "101-mawjs",
                "fleet-window (pulse)",
                &fleet_entries,
                &cloned_repos,
            ),
            vec![
                AttachRecoveryCandidate {
                    oracle: "pulse".to_owned(),
                    label: "pulse-oracle (cloned)".to_owned(),
                },
                AttachRecoveryCandidate {
                    oracle: "Soul-Brews-Studio/pulse-oracle".to_owned(),
                    label: "Soul-Brews-Studio/pulse-oracle".to_owned(),
                },
                AttachRecoveryCandidate {
                    oracle: "Soul-Brews-Studio/pulse-helper-oracle".to_owned(),
                    label: "Soul-Brews-Studio/pulse-helper-oracle".to_owned(),
                },
            ]
        );
    }
