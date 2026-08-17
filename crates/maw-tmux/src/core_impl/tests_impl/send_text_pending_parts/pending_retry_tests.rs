
    #[test]
    fn send_text_reports_warning_after_max_pending_retries() {
        let runner = FakeRunner::with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ deploy"),
        ]);
        let mut client = TmuxClient::new(runner);
        let mut sleeps = Vec::new();
        let report = client
            .send_text_with_sleeper("sess:oracle.0", "deploy", |duration| sleeps.push(duration))
            .expect("send text ok");
        assert_eq!(report.enter_attempts, 4);
        assert!(report.warned_pending);
        assert_eq!(sleeps.len(), 9);
        assert_eq!(sleeps[0], std::time::Duration::from_millis(SEND_SETTLE_MS));
        for pair in sleeps[1..].chunks_exact(2) {
            assert_eq!(pair[0], std::time::Duration::from_millis(SUBMIT_CONFIRM_MS));
            assert_eq!(pair[1], std::time::Duration::from_millis(SUBMIT_GRACE_MS));
        }
        assert_eq!(
            client
                .runner
                .calls
                .iter()
                .filter(|(subcommand, args)| subcommand == "send-keys"
                    && args
                        == &vec![
                            "-t".to_owned(),
                            "sess:oracle.0".to_owned(),
                            "Enter".to_owned()
                        ])
                .count(),
            4
        );
    }

    #[test]
    fn send_text_does_not_retry_non_matching_pending_input() {
        let runner = FakeRunner::with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("❯ deploy"),
            Ok("❯ different queued input"),
        ]);
        let mut client = TmuxClient::new(runner);
        let mut sleeps = Vec::new();
        let report = client
            .send_text_with_sleeper("sess:oracle.0", "deploy", |duration| sleeps.push(duration))
            .expect("send text ok");

        assert_eq!(report.enter_attempts, 1);
        assert!(report.warned_pending);
        assert_eq!(
            sleeps,
            vec![
                std::time::Duration::from_millis(SEND_SETTLE_MS),
                std::time::Duration::from_millis(SUBMIT_CONFIRM_MS),
                std::time::Duration::from_millis(SUBMIT_GRACE_MS),
            ]
        );
        assert_eq!(
            client
                .runner
                .calls
                .iter()
                .filter(|(subcommand, args)| subcommand == "send-keys"
                    && args
                        == &vec![
                            "-t".to_owned(),
                            "sess:oracle.0".to_owned(),
                            "Enter".to_owned()
                        ])
                .count(),
            1
        );
    }

    const BUFFERED_TEXT: &str = "deploy\nnow";
    const BUFFERED_PLACEHOLDER: &str = "❯ [Pasted Content 10 chars]";

    fn send_text_buffered_case(
        mut after_paste: Vec<Result<&str, TmuxError>>,
    ) -> (SendTextReport, FakeRunner) {
        let mut responses = vec![Ok("0"), Ok(""), Ok("")];
        responses.append(&mut after_paste);
        let runner = FakeRunner::with_responses(responses);
        let mut client = TmuxClient::new(runner);
        let report = client
            .send_text_with_sleeper("sess:oracle.0", BUFFERED_TEXT, |_| {})
            .expect("send text ok");
        let enter_count = client
            .runner
            .calls
            .iter()
            .filter(|(subcommand, args)| {
                subcommand == "send-keys" && args.last().is_some_and(|arg| arg == "Enter")
            })
            .count();
        assert!(report.used_buffer);
        assert_eq!(
            enter_count,
            usize::try_from(report.enter_attempts).expect("attempt count fits usize")
        );
        (report, client.runner)
    }

    #[test]
    fn send_text_retries_buffered_placeholder_until_capture_clears() {
        let (report, runner) = send_text_buffered_case(vec![
            Ok(BUFFERED_PLACEHOLDER),
            Ok(""),
            Ok("❯ "),
            Ok(BUFFERED_PLACEHOLDER),
            Ok(""),
            Ok("❯ "),
            Ok("❯ "),
        ]);

        assert_eq!((report.enter_attempts, report.warned_pending), (2, false));
        assert_eq!(runner.calls[1].0, "paste-buffer");
        assert_eq!(runner.calls[2].0, "capture-pane");
        assert_eq!(runner.calls[3].0, "send-keys");
    }

    #[test]
    fn send_text_retries_buffered_literal_echo_until_capture_clears() {
        let (report, _) = send_text_buffered_case(vec![
            Ok("❯ deploy"),
            Ok(""),
            Ok("❯ deploy"),
            Ok("❯ deploy"),
            Ok(""),
            Ok("❯ "),
            Ok("❯ "),
        ]);

        assert_eq!((report.enter_attempts, report.warned_pending), (2, false));
    }

    #[test]
    fn send_text_buffered_baseline_does_not_retry_different_input() {
        let (report, _) = send_text_buffered_case(vec![
            Ok(BUFFERED_PLACEHOLDER),
            Ok(""),
            Ok("❯ different queued input"),
            Ok("❯ different queued input"),
        ]);

        assert_eq!((report.enter_attempts, report.warned_pending), (1, true));
    }

    #[test]
    fn send_text_buffered_baseline_capture_failure_fails_closed() {
        let (report, _) = send_text_buffered_case(vec![
            Err(TmuxError::new("capture failed")),
            Ok(""),
            Ok(BUFFERED_PLACEHOLDER),
            Ok(BUFFERED_PLACEHOLDER),
        ]);

        assert_eq!((report.enter_attempts, report.warned_pending), (1, true));
    }

    #[test]
    fn send_text_waits_out_matching_redraw_before_retrying() {
        let runner = FakeRunner::with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("❯ deploy"),
            Ok("❯ "),
        ]);
        let mut client = TmuxClient::new(runner);
        let mut sleeps = Vec::new();
        let report = client
            .send_text_with_sleeper("sess:oracle.0", "deploy", |duration| sleeps.push(duration))
            .expect("send text ok");

        assert_eq!(report.enter_attempts, 1);
        assert!(!report.warned_pending);
        assert_eq!(
            client
                .runner
                .calls
                .iter()
                .filter(|(subcommand, args)| subcommand == "send-keys"
                    && args
                        == &vec![
                            "-t".to_owned(),
                            "sess:oracle.0".to_owned(),
                            "Enter".to_owned()
                        ])
                .count(),
            1
        );
        assert_eq!(
            sleeps,
            vec![
                std::time::Duration::from_millis(SEND_SETTLE_MS),
                std::time::Duration::from_millis(SUBMIT_CONFIRM_MS),
                std::time::Duration::from_millis(SUBMIT_GRACE_MS),
            ]
        );
    }

    #[test]
    fn send_text_grace_recheck_catches_false_negative_before_success() {
        let runner = FakeRunner::with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("❯ "),
            Ok("❯ deploy"),
            Ok(""),
            Ok("❯ "),
            Ok("❯ "),
        ]);
        let mut client = TmuxClient::new(runner);
        let mut sleeps = Vec::new();
        let report = client
            .send_text_with_sleeper("sess:oracle.0", "deploy", |duration| sleeps.push(duration))
            .expect("send text ok");

        assert_eq!(report.enter_attempts, 2);
        assert!(!report.warned_pending);
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
    }

    #[test]
    fn capture_resize_and_exit_mode_match_maw_js_runtime_helpers() {
        let runner = FakeRunner::with_responses(vec![
            Ok("captured"),
            Err(TmuxError::new("ignored")),
            Ok("1"),
            Ok(""),
        ]);
        let mut client = TmuxClient::new(runner);
        assert_eq!(client.capture("%1", Some(5)).expect("capture"), "captured");
        client.resize_pane("%1", 0, 999);
        assert!(client.exit_mode_if_needed("%1").expect("exit mode"));

        assert_eq!(client.runner.calls[0].0, "capture-pane");
        assert_eq!(
            client.runner.calls[0].1,
            vec!["-t", "%1", "-e", "-p", "-S", "-5"]
        );
        assert_eq!(client.runner.calls[1].0, "resize-pane");
        assert_eq!(
            client.runner.calls[1].1,
            vec!["-t", "%1", "-x", "1", "-y", "200"]
        );
        assert_eq!(client.runner.calls[2].0, "display-message");
        assert_eq!(client.runner.calls[3].1, vec!["-t", "%1", "-X", "cancel"]);
    }

    #[test]
    fn pending_input_detection_matches_maw_js_prompt_heuristic() {
        assert!(pane_input_pending_from_capture("old\n$ maw hey oracle"));
        assert!(pane_input_pending_from_capture(
            "\u{1b}[32m❯\u{1b}[0m cargo test"
        ));
        assert!(pane_input_pending_from_capture("› Explain this codebase"));
        assert!(!pane_input_pending_from_capture("old\n$ "));
        assert!(!pane_input_pending_from_capture("command output only"));
        assert_eq!(strip_tmux_ansi("a\u{1b}[31mred\u{1b}[0m"), "ared");
    }
