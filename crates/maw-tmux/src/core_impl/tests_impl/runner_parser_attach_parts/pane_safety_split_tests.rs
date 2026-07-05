
    #[test]
    fn client_pane_commands_match_maw_js_arg_order() {
        let runner = FakeRunner::with_responses(vec![
            Ok("%9\n"),
            Ok("claude\n"),
            Ok("zsh\t/repo\n"),
            Ok("%10\n"),
            Ok(""),
            Ok(""),
            Ok(""),
        ]);
        let mut client = TmuxClient::new(runner);
        assert_eq!(client.first_pane_id("maw:agent"), Some("%9".to_owned()));
        assert_eq!(
            client.get_pane_command("%9").expect("pane command"),
            "claude"
        );
        assert_eq!(
            client.get_pane_info("%9").expect("pane info"),
            ("zsh".to_owned(), "/repo".to_owned())
        );
        let split = client
            .split_window(
                Some("maw:agent"),
                &SplitWindowOptions {
                    cwd: Some("/repo".to_owned()),
                    command: Some("exec zsh -li".to_owned()),
                    print_format: Some("#{pane_id}".to_owned()),
                },
            )
            .expect("split ok");
        assert_eq!(split, "%10\n");
        client
            .select_pane(
                "%10",
                &SelectPaneOptions {
                    title: Some("oracle".to_owned()),
                },
            )
            .expect("select pane ok");
        client
            .send_keys_literal("%10", "hello | world")
            .expect("literal send ok");
        client
            .send_keys("%10", &["Enter".to_owned()])
            .expect("send keys ok");

        assert_eq!(client.runner.calls[0].0, "list-panes");
        assert_eq!(client.runner.calls[3].0, "split-window");
        assert_eq!(
            client.runner.calls[3].1,
            vec![
                "-P",
                "-F",
                "#{pane_id}",
                "-t",
                "maw:agent",
                "-c",
                "/repo",
                "exec zsh -li",
            ]
        );
        assert_eq!(client.runner.calls[5].0, "send-keys");
        assert_eq!(
            client.runner.calls[5].1,
            vec!["-t", "%10", "-l", "hello | world"]
        );
    }

    #[test]
    fn tmux_safety_destructive_patterns_match_maw_js_cases() {
        let cases = [
            ("ls -la", false),
            ("echo hello", false),
            ("date", false),
            ("pwd && cd /", true),
            ("rm file.txt", true),
            ("rm -rf /tmp/junk", true),
            ("sudo apt update", true),
            ("echo > /etc/passwd", true),
            ("echo >> ~/.bashrc", true),
            ("cat file ; echo done", true),
            ("test && rm -f", true),
            ("cat file | grep x", true),
            ("git reset --hard HEAD", true),
            ("git push --force origin main", true),
            ("git clean -fd", true),
            ("gh repo delete foo/bar", true),
            ("C-c", true),
            ("C-d", true),
            ("clear", true),
            ("exit", true),
            ("kill 12345", true),
            ("kill -9 12345", true),
            ("DROP TABLE users", true),
            ("drop table users", true),
            ("echo 'rm trick'", true),
            ("", false),
        ];
        for (command, destructive) in cases {
            let check = check_destructive(command);
            assert_eq!(check.destructive, destructive, "{command}");
            assert_eq!(check.reasons.is_empty(), !destructive, "{command}");
        }
        let multi = check_destructive("sudo rm -rf /");
        assert!(multi.destructive);
        assert!(multi.reasons.len() >= 2);
    }

    #[test]
    fn tmux_safety_claude_like_pane_matches_maw_js_cases() {
        assert!(is_claude_like_pane(Some("claude")));
        assert!(is_claude_like_pane(Some("CLAUDE")));
        assert!(is_claude_like_pane(Some("bun run claude")));
        assert!(is_claude_like_pane(Some("2.1.111")));
        assert!(!is_claude_like_pane(Some("2.0.0-alpha.105")));
        assert!(!is_claude_like_pane(Some("bash")));
        assert!(!is_claude_like_pane(Some("vim")));
        assert!(!is_claude_like_pane(None));
        assert!(!is_claude_like_pane(Some("")));
    }

    #[test]
    fn tmux_safety_fleet_or_view_session_matches_maw_js_cases() {
        let fleet = BTreeSet::from([
            "101-mawjs".to_owned(),
            "112-fusion".to_owned(),
            "114-mawjs-no2".to_owned(),
        ]);
        assert!(is_fleet_or_view_session("101-mawjs", &fleet));
        assert!(is_fleet_or_view_session("maw-view", &fleet));
        assert!(is_fleet_or_view_session("mawjs-view", &fleet));
        assert!(is_fleet_or_view_session("fusion-view", &fleet));
        assert!(!is_fleet_or_view_session("random-session", &fleet));
        assert!(!is_fleet_or_view_session("view-something", &fleet));
        assert!(is_fleet_or_view_session("maw-view", &BTreeSet::new()));
        assert!(is_fleet_or_view_session("anything-view", &BTreeSet::new()));
    }

    #[test]
    fn tmux_action_layout_and_split_validation_match_maw_js_cases() {
        let error = validate_layout_preset("bogus").expect_err("invalid layout");
        assert!(error.message.contains("invalid layout 'bogus'"));
        assert!(error.message.contains("even-horizontal"));
        assert!(error.message.contains("main-horizontal"));
        assert!(error.message.contains("tiled"));
        assert!(validate_layout_preset("tiled").is_ok());

        for pct in [0.0, 100.0, -5.0, f64::NAN] {
            let error = split_pct_arg(pct).expect_err("invalid pct");
            assert!(error.message.contains("--pct must be 1-99"));
        }
        assert_eq!(split_pct_arg(50.0).expect("valid pct"), "50");
        assert_eq!(split_pct_arg(12.5).expect("valid fractional pct"), "12.5");
        assert_eq!(
            tmux_split_action_args(
                "alpha:0.1",
                &TmuxSplitActionOptions {
                    vertical: false,
                    pct: 40.0,
                    command: Some("bash -lc 'echo hi'".to_owned()),
                },
            )
            .expect("valid split args"),
            vec!["-h", "-l", "40%", "-t", "alpha:0.1", "bash -lc 'echo hi'"]
        );
        assert_eq!(tmux_window_target("some-session:0.1"), "some-session:0");
        assert_eq!(tmux_window_target("some-session"), "some-session");
    }

    #[test]
    fn tmux_split_and_layout_actions_wrap_host_failures_like_maw_js() {
        let target = TmuxKillTarget {
            resolved: "%1".to_owned(),
            source: "pane-id".to_owned(),
        };
        let runner = FakeRunner::with_responses(vec![Err(TmuxError::new("split bad"))]);
        let mut client = TmuxClient::new(runner);
        let error = client
            .split_target_action(&target, &TmuxSplitActionOptions::default())
            .expect_err("split error wrapped");
        assert_eq!(
            error.message,
            "split-window failed for '%1' (from pane-id): split bad"
        );

        let target = TmuxKillTarget {
            resolved: "demo:1.2".to_owned(),
            source: "session:w.p".to_owned(),
        };
        let runner = FakeRunner::with_responses(vec![Err(TmuxError::new("layout denied"))]);
        let mut client = TmuxClient::new(runner);
        let error = client
            .select_layout_action(&target, "tiled")
            .expect_err("layout error wrapped");
        assert_eq!(
            error.message,
            "select-layout failed for 'demo:1' (from session:w.p): layout denied"
        );
        assert_eq!(
            client.runner.calls,
            vec![(
                "select-layout".to_owned(),
                vec!["-t".to_owned(), "demo:1".to_owned(), "tiled".to_owned()]
            )]
        );

        let error = client
            .select_layout_action(&target, "spiral")
            .expect_err("invalid layout");
        assert!(error.message.contains("invalid layout 'spiral'"));
    }
