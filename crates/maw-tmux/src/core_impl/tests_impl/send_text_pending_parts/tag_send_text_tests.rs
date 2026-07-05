
    #[test]
    fn split_window_locked_builds_maw_js_args() {
        let runner = FakeRunner::with_responses(vec![Ok(""), Ok(""), Ok("")]);
        let mut client = TmuxClient::new(runner);
        client
            .split_window_locked("main:0", &SplitWindowLockedOptions::default())
            .expect("default split ok");
        client
            .split_window_locked(
                "main:1",
                &SplitWindowLockedOptions {
                    vertical: Some(true),
                    pct: Some(33),
                    shell_command: Some("zsh".to_owned()),
                },
            )
            .expect("vertical split ok");
        client
            .split_window_locked(
                "main:2",
                &SplitWindowLockedOptions {
                    vertical: Some(false),
                    pct: Some(20),
                    shell_command: None,
                },
            )
            .expect("horizontal split ok");

        assert_eq!(
            client.runner.calls,
            vec![
                (
                    "split-window".to_owned(),
                    vec!["-t", "main:0"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "split-window".to_owned(),
                    vec!["-t", "main:1", "-v", "-l", "33%", "zsh"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "split-window".to_owned(),
                    vec!["-t", "main:2", "-h", "-l", "20%"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
            ]
        );
    }

    #[test]
    fn tag_pane_sets_title_and_meta_with_auto_at_prefix() {
        let runner = FakeRunner::with_responses(vec![Ok(""), Ok(""), Ok("")]);
        let mut client = TmuxClient::new(runner);
        let meta = vec![
            ("agent-name".to_owned(), "scout".to_owned()),
            ("@role".to_owned(), "teammate".to_owned()),
        ];
        client
            .tag_pane("s:0.1", Some("oracle main"), &meta)
            .expect("tag pane ok");

        assert_eq!(
            client.runner.calls,
            vec![
                (
                    "select-pane".to_owned(),
                    vec!["-t", "s:0.1", "-T", "oracle main"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "set-option".to_owned(),
                    vec!["-p", "-t", "s:0.1", "@agent-name", "scout"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
                (
                    "set-option".to_owned(),
                    vec!["-p", "-t", "s:0.1", "@role", "teammate"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                ),
            ]
        );
    }

    #[test]
    fn read_pane_tags_parses_quoted_meta_options() {
        let runner = FakeRunner::with_responses(vec![
            Ok("oracle\n"),
            Ok("@agent-name \"scout\"\n@role teammate\n@quote \"say \\\"hi\\\"\"\nwindow-style default\n"),
        ]);
        let mut client = TmuxClient::new(runner);
        let tags = client.read_pane_tags("s:0.1").expect("read tags ok");
        assert_eq!(tags.title, "oracle");
        assert_eq!(
            tags.meta,
            BTreeMap::from([
                ("@agent-name".to_owned(), "scout".to_owned()),
                ("@quote".to_owned(), "say \"hi\"".to_owned()),
                ("@role".to_owned(), "teammate".to_owned()),
            ])
        );
        assert_eq!(client.runner.calls[0].0, "display-message");
        assert_eq!(client.runner.calls[1].0, "show-options");
    }

    #[test]
    fn send_text_uses_literal_path_and_retries_until_capture_clears() {
        let runner = FakeRunner::with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("\u{1b}[32m❯\u{1b}[0m deploy now\r"),
            Ok("\u{1b}[32m❯\u{1b}[0m deploy now\r"),
            Ok(""),
            Ok("\u{1b}[32m❯\u{1b}[0m \r"),
            Ok("\u{1b}[32m❯\u{1b}[0m \r"),
        ]);
        let mut client = TmuxClient::new(runner);
        let mut sleeps = Vec::new();
        let report = client
            .send_text_with_sleeper("sess:oracle.0", "deploy now", |duration| sleeps.push(duration))
            .expect("send text ok");
        assert_eq!(
            report,
            SendTextReport {
                used_buffer: false,
                enter_attempts: 2,
                warned_pending: false,
            }
        );
        assert_eq!(
            sleeps,
            vec![
                std::time::Duration::from_millis(SEND_SETTLE_MS),
                std::time::Duration::from_millis(SUBMIT_CONFIRM_MS),
                std::time::Duration::from_millis(SUBMIT_GRACE_MS),
                std::time::Duration::from_millis(SUBMIT_CONFIRM_MS),
                std::time::Duration::from_millis(SUBMIT_GRACE_MS),
            ]
        );
        assert_eq!(client.runner.calls[0].0, "display-message");
        assert_eq!(
            client.runner.calls[1].1,
            vec!["-t", "sess:oracle.0", "-l", "deploy now"]
        );
        assert_eq!(
            client.runner.calls[2].1,
            vec!["-t", "sess:oracle.0", "Enter"]
        );
        assert_eq!(client.runner.calls[3].0, "capture-pane");
        assert_eq!(client.runner.calls[4].0, "capture-pane");
        assert_eq!(
            client.runner.calls[5].1,
            vec!["-t", "sess:oracle.0", "Enter"]
        );
        assert_eq!(client.runner.stdin_calls.len(), 0);
    }

    #[test]
    fn send_text_uses_buffer_path_for_multiline_or_long_payloads() {
        let long_text = "x".repeat(501);
        let runner =
            FakeRunner::with_responses(vec![Ok("0"), Ok(""), Ok(""), Ok(""), Ok("$ \r"), Ok("$ \r")]);
        let mut client = TmuxClient::new(runner);
        let mut sleeps = Vec::new();
        let report = client
            .send_text_with_sleeper("sess:oracle.0", &long_text, |duration| sleeps.push(duration))
            .expect("send text ok");
        assert!(report.used_buffer);
        assert_eq!(report.enter_attempts, 1);
        assert_eq!(
            sleeps,
            vec![
                std::time::Duration::from_millis(SEND_SETTLE_MS),
                std::time::Duration::from_millis(SUBMIT_CONFIRM_MS),
                std::time::Duration::from_millis(SUBMIT_GRACE_MS),
            ]
        );
        assert_eq!(
            client.runner.stdin_calls,
            vec![("load-buffer".to_owned(), vec!["-".to_owned()], long_text,)]
        );
        assert_eq!(client.runner.calls[1].0, "paste-buffer");
    }
