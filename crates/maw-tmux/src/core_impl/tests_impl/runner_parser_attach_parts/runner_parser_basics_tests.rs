
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
