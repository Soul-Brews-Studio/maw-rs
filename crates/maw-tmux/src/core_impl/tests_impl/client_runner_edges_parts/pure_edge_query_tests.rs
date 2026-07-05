
    #[test]
    fn pure_edge_cases_cover_malformed_ansi_targets_and_duration_inputs() {
        assert_eq!(
            strip_tmux_ansi("left\u{1b}[2Kright\u{1b}[1G!"),
            "leftright!"
        );
        assert_eq!(strip_tmux_ansi("left\u{1b}[?right"), "left\u{1b}[?right");
        assert_eq!(strip_tmux_ansi("wide λ"), "wide λ");
        assert!(!pane_input_pending_from_capture("\n \n\t"));
        assert!(contains_word("please rm now", "rm"));
        assert!(!contains_word("farmhouse", "rm"));
        assert!(!check_destructive("program").destructive);
        assert!(!has_redirect("echo hi >", false));
        assert!(!has_redirect("echo hi >>", true));
        assert!(!is_claude_like_pane(Some(".")));
        assert!(!is_claude_like_pane(Some("1.")));
        assert!(!is_claude_like_pane(None));
        assert_eq!(tmux_window_target("session.window.1"), "session.window.1");
        assert_eq!(tmux_window_target("session:win.x"), "session:win.x");
        assert_eq!(
            parse_session_activity_list("s\t123\nbad\tnope\n"),
            BTreeMap::from([("s".to_owned(), 123)])
        );
        assert_eq!(parse_active_duration_seconds(Some("10s")), Some(10));
        assert_eq!(parse_active_duration_seconds(Some("15x")), None);
        assert_eq!(parse_active_duration_seconds(Some("")), None);
        assert_eq!(
            active_duration_arg(&["--active".to_owned()], "--active"),
            None
        );
        assert_eq!(
            active_duration_arg(&["--active=15m".to_owned()], "--active"),
            Some("15m".to_owned())
        );
        assert_eq!(
            active_duration_arg(&["--active=0m".to_owned()], "--active"),
            None
        );
        assert_eq!(
            active_duration_arg(&["--active".to_owned(), "-v".to_owned()], "--active"),
            None
        );
        assert_eq!(format_session_created(Some(1)), "1970-01-01 00:00:01");
        assert_eq!(
            similar_oracle_candidates_from_repos("plain", &["plain-oracle".to_owned()]),
            vec!["plain-oracle"]
        );
        assert_eq!(
            tmux_shell_command(Some(""), "list-panes", &[]),
            "tmux -S '' list-panes"
        );
        assert_eq!(
            parse_pane_tag_options("@broken\nnot-meta value\n"),
            BTreeMap::new()
        );
        assert_eq!(
            parse_pane_tag_options("@quoted \"value\\\\tail\\\\\""),
            BTreeMap::from([("@quoted".to_owned(), "value\\tail\\".to_owned())])
        );
        assert_eq!(parse_list_all_windows("too|||short\n"), Vec::new());
        assert!(pane_target_candidates_from_list_panes_output("||||||||||||").is_empty());
        assert_eq!(basename("///"), "///");
        assert!(worktree_names_from_cwd("").is_empty());
        assert_eq!(
            worktree_names_from_cwd("/tmp/project-oracle.wt-7-codex")
                .into_iter()
                .map(|(name, source)| format!("{source}:{name}"))
                .collect::<Vec<_>>(),
            vec![
                "worktree-dir:project-oracle.wt-7-codex",
                "worktree-role:codex",
                "worktree-alias:project-codex",
            ]
        );
        assert_eq!(parse_tmux_pane_target(":win.1"), None);
        assert_eq!(parse_tmux_pane_target("session:.1"), None);
        assert_eq!(parse_tmux_pane_target("session:win."), None);
    }

    #[test]
    fn tmux_client_remaining_simple_queries_use_runner_outputs() {
        let runner = FakeRunner::with_responses(vec![
            Ok("1:main:1\n2:logs:0\n"),
            Ok("bash\nzsh\n"),
            Ok("vim\t/tmp/repo\n"),
            Ok("pane title\n"),
            Ok("@role worker\n@quoted \"hello\\\\ world\"\nwindow-option ignored\nmalformed\n"),
        ]);
        let mut client = TmuxClient::new(runner);

        assert_eq!(
            client.list_windows("demo").expect("windows parse"),
            vec![
                TmuxWindow {
                    index: 1,
                    name: "main".to_owned(),
                    active: true,
                    cwd: None,
                },
                TmuxWindow {
                    index: 2,
                    name: "logs".to_owned(),
                    active: false,
                    cwd: None,
                },
            ]
        );
        assert_eq!(client.get_pane_command("%1").expect("command"), "bash");
        assert_eq!(
            client.get_pane_info("%1").expect("pane info"),
            ("vim".to_owned(), "/tmp/repo".to_owned())
        );
        assert_eq!(
            client.read_pane_tags("%1").expect("tags"),
            PaneTags {
                title: "pane title".to_owned(),
                meta: BTreeMap::from([
                    ("@quoted".to_owned(), "hello\\ world".to_owned()),
                    ("@role".to_owned(), "worker".to_owned()),
                ]),
            }
        );
    }
