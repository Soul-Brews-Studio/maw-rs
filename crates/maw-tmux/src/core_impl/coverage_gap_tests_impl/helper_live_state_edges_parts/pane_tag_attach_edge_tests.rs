
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
