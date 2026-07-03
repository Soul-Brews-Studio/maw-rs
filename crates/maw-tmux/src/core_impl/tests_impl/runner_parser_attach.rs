
    use super::*;

    #[derive(Default)]
    struct FakeRunner {
        calls: Vec<(String, Vec<String>)>,
        stdin_calls: Vec<(String, Vec<String>, String)>,
        responses: Vec<Result<String, TmuxError>>,
    }

    impl FakeRunner {
        fn with_responses(responses: Vec<Result<&str, TmuxError>>) -> Self {
            Self {
                calls: Vec::new(),
                stdin_calls: Vec::new(),
                responses: responses
                    .into_iter()
                    .map(|response| response.map(str::to_owned))
                    .collect(),
            }
        }
    }

    impl FakeRunner {
        fn next_response(&mut self) -> Result<String, TmuxError> {
            if self.responses.is_empty() {
                return Err(TmuxError::new("no response"));
            }
            self.responses.remove(0)
        }
    }

    impl TmuxRunner for FakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            self.next_response()
        }

        fn run_with_stdin(
            &mut self,
            subcommand: &str,
            args: &[String],
            stdin: &[u8],
        ) -> Result<String, TmuxError> {
            self.stdin_calls.push((
                subcommand.to_owned(),
                args.to_vec(),
                String::from_utf8_lossy(stdin).into_owned(),
            ));
            self.next_response()
        }
    }

    #[test]
    fn shell_quote_matches_maw_js_safe_chars_and_single_quote_escape() {
        assert_eq!(
            shell_quote("alpha_1:/tmp/repo.wt-main"),
            "alpha_1:/tmp/repo.wt-main"
        );
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn command_runner_argv_matches_tmux_socket_order_without_executing() {
        let runner = CommandTmuxRunner::with_program("/usr/bin/tmux").with_socket("/tmp/maw sock");
        let argv = runner.argv(
            "list-panes",
            &["-a".to_owned(), "-F".to_owned(), "#{pane_id}".to_owned()],
        );
        assert_eq!(
            argv,
            vec![
                OsString::from("/usr/bin/tmux"),
                OsString::from("-S"),
                OsString::from("/tmp/maw sock"),
                OsString::from("list-panes"),
                OsString::from("-a"),
                OsString::from("-F"),
                OsString::from("#{pane_id}"),
            ]
        );
    }

    #[test]
    fn tmux_shell_command_includes_optional_socket() {
        assert_eq!(
            tmux_shell_command(
                Some("/tmp/maw sock"),
                "list-windows",
                &[
                    "-a".to_owned(),
                    "-F".to_owned(),
                    "#{window_name}".to_owned()
                ],
            ),
            "tmux -S '/tmp/maw sock' list-windows -a -F '#{window_name}'",
        );
    }

    #[test]
    fn parse_list_all_groups_windows_by_session_in_order() {
        let sessions = parse_list_all_windows(
            "s1|||1|||alpha|||1|||/tmp/a\ns1|||2|||beta|||0|||\ns2|||1|||gamma|||0|||/tmp/g\n",
        );
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "s1");
        assert_eq!(sessions[0].windows[0].cwd.as_deref(), Some("/tmp/a"));
        assert_eq!(sessions[0].windows[1].cwd, None);
        assert!(sessions[0].windows[0].active);
        assert_eq!(sessions[1].windows[0].name, "gamma");
    }

    #[test]
    fn parse_list_windows_matches_maw_js_colon_format() {
        assert_eq!(
            parse_list_windows("1:oracle:1\n2:notes:0\n"),
            vec![
                TmuxWindow {
                    index: 1,
                    name: "oracle".to_owned(),
                    active: true,
                    cwd: None
                },
                TmuxWindow {
                    index: 2,
                    name: "notes".to_owned(),
                    active: false,
                    cwd: None
                },
            ],
        );
    }

    #[test]
    fn parse_list_panes_handles_optional_numeric_fields() {
        let panes = parse_list_panes(
            "%1|||claude|||s:oracle.0|||title|||123|||/repo|||456\n%2|||zsh|||s:logs.0|||||||||\n",
        );
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].pid, Some(123));
        assert_eq!(panes[0].cwd.as_deref(), Some("/repo"));
        assert_eq!(panes[0].last_activity, Some(456));
        assert_eq!(panes[1].pid, None);
    }

    #[test]
    fn client_session_mutators_match_maw_js_arg_order() {
        let runner = FakeRunner::with_responses(vec![
            Ok("%1\n"),
            Err(TmuxError::new("set-option ignored")),
            Ok(""),
            Ok(""),
        ]);
        let mut client = TmuxClient::new(runner);
        let out = client
            .new_session(
                "maw",
                &NewSessionOptions {
                    window: Some("agent".to_owned()),
                    cwd: Some("/repo".to_owned()),
                    command: Some("exec zsh -li".to_owned()),
                    print_format: Some("#{pane_id}".to_owned()),
                    ..NewSessionOptions::default()
                },
            )
            .expect("new session ok");
        assert_eq!(out, "%1\n");
        client
            .new_window("maw", "logs", Some("/tmp"))
            .expect("new window ok");
        client.kill_session("old");

        assert_eq!(client.runner.calls[0].0, "new-session");
        assert_eq!(
            client.runner.calls[0].1,
            vec![
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
                "-s",
                "maw",
                "-n",
                "agent",
                "-c",
                "/repo",
                "exec zsh -li",
            ]
        );
        assert_eq!(client.runner.calls[1].0, "set-option");
        assert_eq!(
            client.runner.calls[2],
            (
                "new-window".to_owned(),
                vec!["-t", "maw:", "-n", "logs", "-c", "/tmp"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            )
        );
        assert_eq!(client.runner.calls[3].0, "kill-session");
    }

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

    #[test]
    fn tmux_attach_action_branches_match_maw_js_cases() {
        let alive = BTreeSet::from(["some-session".to_owned()]);
        assert_eq!(
            decide_tmux_attach_action(
                "%999",
                &BTreeSet::from(["%999".to_owned()]),
                true,
                true,
                false
            ),
            TmuxAttachAction::Print {
                session: "%999".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("some-session:0.1", &alive, true, true, false),
            TmuxAttachAction::Print {
                session: "some-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("some-session:0.1", &alive, false, false, false),
            TmuxAttachAction::Print {
                session: "some-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("some-session:0.1", &alive, false, true, true),
            TmuxAttachAction::SwitchClient {
                session: "some-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("some-session:0.1", &alive, false, true, false),
            TmuxAttachAction::Attach {
                session: "some-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("ghost-session", &alive, false, true, false),
            TmuxAttachAction::Recover {
                session: "ghost-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action(
                "pulse",
                &BTreeSet::from(["01-pulse".to_owned(), "02-pulse".to_owned()]),
                false,
                true,
                false
            ),
            TmuxAttachAction::Recover {
                session: "pulse".to_owned()
            }
        );

        assert_eq!(
            tmux_attach_spawn_command(&TmuxAttachAction::SwitchClient {
                session: "some-session".to_owned()
            }),
            Some(SpawnCommand {
                program: "tmux".to_owned(),
                args: vec![
                    "switch-client".to_owned(),
                    "-t".to_owned(),
                    "some-session".to_owned()
                ],
            })
        );
        assert_eq!(
            tmux_attach_spawn_command(&TmuxAttachAction::Attach {
                session: "some-session".to_owned()
            }),
            Some(SpawnCommand {
                program: "tmux".to_owned(),
                args: vec![
                    "attach".to_owned(),
                    "-t".to_owned(),
                    "some-session".to_owned()
                ],
            })
        );
        assert_eq!(
            tmux_attach_spawn_command(&TmuxAttachAction::Print {
                session: "some-session".to_owned()
            }),
            None
        );
    }

    #[test]
    fn tmux_attach_session_resolution_prefers_exact_then_fuzzy() {
        let alive = BTreeSet::from([
            "05-volt".to_owned(),
            "mawjs-codex".to_owned(),
            "50-mawjs-codex".to_owned(),
            "volt".to_owned(),
        ]);
        assert_eq!(
            resolve_tmux_attach_session("volt", &alive),
            TmuxAttachSessionResolution::Match {
                session: "volt".to_owned()
            }
        );
        assert_eq!(
            resolve_tmux_attach_session("mawjscodex", &alive),
            TmuxAttachSessionResolution::Match {
                session: "50-mawjs-codex".to_owned()
            }
        );

        let only_numbered = BTreeSet::from(["05-volt".to_owned()]);
        assert_eq!(
            resolve_tmux_attach_session("volt", &only_numbered),
            TmuxAttachSessionResolution::Match {
                session: "05-volt".to_owned()
            }
        );

        let numbered_preferred = BTreeSet::from(["05-volt".to_owned(), "volt-oracle".to_owned()]);
        assert_eq!(
            resolve_tmux_attach_session("volt", &numbered_preferred),
            TmuxAttachSessionResolution::Match {
                session: "05-volt".to_owned()
            }
        );
    }

    #[test]
    fn tmux_attach_session_resolution_matches_numeric_fleet_prefix() {
        let alive = BTreeSet::from([
            "187-sentinel".to_owned(),
            "188-maw-rs".to_owned(),
            "189-crew-master".to_owned(),
        ]);
        assert_eq!(
            resolve_tmux_attach_session("188", &alive),
            TmuxAttachSessionResolution::Match {
                session: "188-maw-rs".to_owned()
            }
        );
        assert_eq!(
            resolve_tmux_attach_session("188-maw", &alive),
            TmuxAttachSessionResolution::Match {
                session: "188-maw-rs".to_owned()
            }
        );
        assert_eq!(
            resolve_tmux_attach_session("18", &alive),
            TmuxAttachSessionResolution::Ambiguous {
                query: "18".to_owned(),
                candidates: vec![
                    "187-sentinel".to_owned(),
                    "188-maw-rs".to_owned(),
                    "189-crew-master".to_owned()
                ]
            }
        );

        let duplicate_prefix = BTreeSet::from(["188-alpha".to_owned(), "188-beta".to_owned()]);
        assert_eq!(
            resolve_tmux_attach_session("188", &duplicate_prefix),
            TmuxAttachSessionResolution::Ambiguous {
                query: "188".to_owned(),
                candidates: vec!["188-alpha".to_owned(), "188-beta".to_owned()]
            }
        );
    }

    #[test]
    fn tmux_attach_session_resolution_refuses_loose_ambiguity() {
        let alive = BTreeSet::from(["05-calliope".to_owned(), "06-caller".to_owned()]);
        assert_eq!(
            resolve_tmux_attach_session("call", &alive),
            TmuxAttachSessionResolution::Ambiguous {
                query: "call".to_owned(),
                candidates: vec!["05-calliope".to_owned(), "06-caller".to_owned()]
            }
        );
        assert_eq!(
            resolve_tmux_attach_session("ghost", &alive),
            TmuxAttachSessionResolution::Missing {
                session: "ghost".to_owned()
            }
        );
        assert_eq!(
            resolve_tmux_attach_session(" :0.1", &alive),
            TmuxAttachSessionResolution::Missing {
                session: String::new()
            }
        );
    }
