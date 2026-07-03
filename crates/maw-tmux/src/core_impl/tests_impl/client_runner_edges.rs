    #[test]
    fn client_grouped_session_and_best_effort_mutators_match_arg_order() {
        let runner = FakeRunner::with_responses(vec![
            Ok(""),
            Ok(""),
            Err(TmuxError::new("select ignored")),
            Ok(""),
            Ok(""),
            Ok(""),
            Ok(""),
            Ok(""),
        ]);
        let mut client = TmuxClient::new(runner);

        client
            .new_grouped_session(
                "parent",
                "child",
                &GroupedSessionOptions {
                    cols: Some(120),
                    rows: Some(40),
                    window: Some("agent".to_owned()),
                    window_size: Some("manual".to_owned()),
                },
            )
            .expect("grouped session ok");
        client.select_window("child:agent");
        client.switch_client("child");
        client.kill_window("child:logs");
        client.kill_pane("%2");
        client.set("child", "@maw", "on");

        assert_eq!(
            client.runner.calls,
            vec![
                (
                    "new-session".to_owned(),
                    vec!["-d", "-t", "parent", "-s", "child", "-x", "120", "-y", "40"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "set-option".to_owned(),
                    vec!["-t", "child", "window-size", "manual"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "select-window".to_owned(),
                    vec!["-t", "child:agent"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "select-window".to_owned(),
                    vec!["-t", "child:agent"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "switch-client".to_owned(),
                    vec!["-t", "child"].into_iter().map(str::to_owned).collect()
                ),
                (
                    "kill-window".to_owned(),
                    vec!["-t", "child:logs"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "kill-pane".to_owned(),
                    vec!["-t", "%2"].into_iter().map(str::to_owned).collect()
                ),
                (
                    "set".to_owned(),
                    vec!["-t", "child", "@maw", "on"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
            ]
        );
    }

    #[test]
    fn client_split_layout_resize_and_environment_helpers_match_arg_order() {
        let runner = FakeRunner::with_responses(vec![Ok(""), Ok(""), Ok(""), Ok(""), Ok("")]);
        let mut client = TmuxClient::new(runner);

        client
            .split_pane_action(
                "s:0.1",
                &TmuxSplitActionOptions {
                    vertical: true,
                    pct: 25.0,
                    command: None,
                },
            )
            .expect("split pane action ok");
        client
            .select_layout("s:0", "tiled")
            .expect("select layout ok");
        client
            .select_valid_layout("s:0.1", "even-horizontal")
            .expect("valid layout ok");
        client.resize_window("s:0", 999, 0);
        client
            .set_environment("s", "MAW_MODE", "test")
            .expect("set env ok");

        assert_eq!(
            client.runner.calls,
            vec![
                (
                    "split-window".to_owned(),
                    vec!["-v", "-l", "25%", "-t", "s:0.1"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "select-layout".to_owned(),
                    vec!["-t", "s:0", "tiled"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "select-layout".to_owned(),
                    vec!["-t", "s:0", "even-horizontal"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "resize-window".to_owned(),
                    vec!["-t", "s:0", "-x", "500", "-y", "1"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "set-environment".to_owned(),
                    vec!["-t", "s", "MAW_MODE", "test"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
            ]
        );
    }

    #[test]
    fn runner_default_stdin_and_constructor_paths_are_testable_without_tmux_io() {
        struct RunOnlyRunner {
            calls: Vec<(String, Vec<String>)>,
        }

        impl TmuxRunner for RunOnlyRunner {
            fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, TmuxError> {
                self.calls.push((subcommand.to_owned(), args.to_vec()));
                Ok("fallback".to_owned())
            }
        }

        let mut runner = RunOnlyRunner { calls: Vec::new() };
        assert_eq!(
            runner
                .run_with_stdin("load-buffer", &["-".to_owned()], b"ignored")
                .expect("default stdin delegates"),
            "fallback"
        );
        assert_eq!(
            runner.calls,
            vec![("load-buffer".to_owned(), vec!["-".to_owned()])]
        );

        assert_eq!(
            CommandTmuxRunner::new().argv("display-message", &[]),
            vec![OsString::from("tmux"), OsString::from("display-message")]
        );
        assert_eq!(
            TmuxClient::local().runner.argv(
                "list-sessions",
                &["-F".to_owned(), "#{session_name}".to_owned()]
            ),
            vec![
                OsString::from("tmux"),
                OsString::from("list-sessions"),
                OsString::from("-F"),
                OsString::from("#{session_name}"),
            ]
        );
        assert_eq!(
            TmuxClient::local_with_socket("/tmp/maw.sock")
                .runner
                .argv("list-panes", &[]),
            vec![
                OsString::from("tmux"),
                OsString::from("-S"),
                OsString::from("/tmp/maw.sock"),
                OsString::from("list-panes"),
            ]
        );
    }

    #[test]
    fn command_runner_process_adapter_handles_success_stdin_and_errors_without_tmux() {
        let mut printf_runner = CommandTmuxRunner::with_program("/usr/bin/printf");
        assert_eq!(
            printf_runner
                .run("hello %s", &["world".to_owned()])
                .expect("printf succeeds"),
            "hello world"
        );
        assert_eq!(
            tmux_program_io_error(
                "write stdin for",
                std::ffi::OsStr::new("tmux"),
                &std::io::Error::other("closed")
            )
            .message,
            "failed to write stdin for tmux: closed"
        );

        let mut cat_runner = CommandTmuxRunner::with_program("/bin/cat");
        assert_eq!(
            cat_runner
                .run_with_stdin("-", &[], b"buffer text")
                .expect("cat echoes stdin"),
            "buffer text"
        );

        let mut shell_runner = CommandTmuxRunner::with_program("/bin/sh");
        let error = shell_runner
            .run("-c", &["printf denied >&2; exit 7".to_owned()])
            .expect_err("shell exits non-zero");
        assert_eq!(error.message, "tmux exited with status 7: denied");

        let mut missing_runner = CommandTmuxRunner::with_program("/definitely/not/a/tmux");
        let error = missing_runner
            .run("list-sessions", &[])
            .expect_err("missing program");
        assert!(error
            .message
            .contains("failed to execute /definitely/not/a/tmux"));

        let mut quiet_failure_runner = CommandTmuxRunner::with_program("/bin/sh");
        let error = quiet_failure_runner
            .run("-c", &["exit 9".to_owned()])
            .expect_err("empty stderr/stdout reports status only");
        assert_eq!(error.message, "tmux exited with status 9");
    }

    #[test]
    fn error_display_and_tracker_clear_cover_diagnostic_paths() {
        let error = TmuxError::new("tmux failed");
        assert_eq!(error.to_string(), "tmux failed");

        let mut tracker = TmuxSendTracker::default();
        assert_eq!(tracker.check("%1", 1_000, false), SendThrottle::Allowed);
        assert!(tracker.get("%1").is_some());
        tracker.clear();
        assert_eq!(tracker.get("%1"), None);
    }

    #[test]
    fn send_action_empty_throttled_and_tmux_lookup_error_paths_are_safe() {
        let mut client = TmuxClient::new(FakeRunner::default());
        let mut tracker = TmuxSendTracker::default();
        let error = client
            .send_command_to_pane(
                &mut tracker,
                "%1",
                "",
                &TmuxSendCommandOptions::default(),
                1_000,
            )
            .expect_err("empty command rejected before tmux lookup");
        assert!(error.message.contains("usage: maw tmux send"));
        assert!(client.runner.calls.is_empty());

        let mut client = TmuxClient::new(FakeRunner::default());
        let mut tracker = TmuxSendTracker::default();
        tracker.set(
            "%1",
            SendTrackerEntry {
                last_ts: 1_000,
                count: 1,
                window_start: 1_000,
            },
        );
        let outcome = client
            .send_command_to_pane(
                &mut tracker,
                "%1",
                "echo two",
                &TmuxSendCommandOptions::default(),
                1_100,
            )
            .expect("cooldown reported without tmux lookup");
        assert_eq!(
            outcome,
            TmuxSendCommandOutcome::Throttled(SendThrottle::Cooldown { cooldown_ms: 500 })
        );

        let runner = FakeRunner::with_responses(vec![Err(TmuxError::new("pane gone"))]);
        let mut client = TmuxClient::new(runner);
        let mut tracker = TmuxSendTracker::default();
        let error = client
            .send_command_to_pane(
                &mut tracker,
                "%9",
                "echo safe",
                &TmuxSendCommandOptions::default(),
                2_000,
            )
            .expect_err("display-message error propagates");
        assert_eq!(error.message, "pane gone");
        assert_eq!(client.runner.calls[0].0, "display-message");
    }

    #[test]
    fn client_error_branches_preserve_context_and_do_not_require_tmux() {
        let target = TmuxKillTarget {
            resolved: "demo:1.2".to_owned(),
            source: "session:w.p".to_owned(),
        };
        let runner = FakeRunner::with_responses(vec![Err(TmuxError::new("session denied"))]);
        let mut client = TmuxClient::new(runner);
        let error = client
            .kill_target_action(
                &target,
                &BTreeSet::new(),
                &TmuxKillCommandOptions {
                    force: false,
                    session: true,
                },
            )
            .expect_err("session kill wraps runner error");
        assert_eq!(
            error.message,
            "kill failed for 'demo:1.2' (from session:w.p): session denied"
        );

        let runner =
            FakeRunner::with_responses(vec![Ok("1"), Err(TmuxError::new("not in a mode"))]);
        let mut client = TmuxClient::new(runner);
        assert!(!client
            .exit_mode_if_needed("%1")
            .expect("stale copy-mode cancellation is benign"));

        let runner = FakeRunner::with_responses(vec![Ok("1"), Err(TmuxError::new("server lost"))]);
        let mut client = TmuxClient::new(runner);
        let error = client
            .exit_mode_if_needed("%1")
            .expect_err("non-benign cancellation error propagates");
        assert_eq!(error.message, "server lost");
    }

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
