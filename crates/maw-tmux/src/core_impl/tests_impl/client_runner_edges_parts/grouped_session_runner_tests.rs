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
