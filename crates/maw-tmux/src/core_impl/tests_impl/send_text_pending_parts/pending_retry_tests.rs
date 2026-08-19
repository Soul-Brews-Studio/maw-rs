
    fn send_text_runner(mut responses: Vec<Result<&str, TmuxError>>) -> FakeRunner {
        responses.insert(1, Ok("❯ "));
        FakeRunner::with_responses(responses)
    }

    #[test]
    fn send_text_refuses_to_append_to_existing_pending_input() {
        for (preflight, needle) in [
            (Ok("❯ [Pasted Content 2040 chars]"), "already has pending input"),
            (Err(TmuxError::new("capture failed")), "capture failed"),
        ] {
            let mut client = TmuxClient::new(FakeRunner::with_responses(vec![Ok("0"), preflight]));
            let error = client
                .send_text_with_sleeper("sess:oracle.0", "new dispatch", |_| {})
                .expect_err("unverified composer must refuse new text");
            assert!(error.message.contains(needle));
            assert_eq!(client.runner.calls.len(), 2);
            assert_eq!(client.runner.calls[1].1, vec!["-t", "sess:oracle.0", "-e", "-p", "-J", "-S", "-80"]);
            assert!(client.runner.stdin_calls.is_empty());
        }
    }

    #[test]
    fn send_text_fails_when_confirmation_capture_fails() {
        let runner = send_text_runner(vec![Ok("0"), Ok(""), Ok(""), Err(TmuxError::new("capture failed"))]);
        let mut client = TmuxClient::new(runner);
        let error = client
            .send_text_with_sleeper("sess:oracle.0", "deploy", |_| {})
            .expect_err("unverified submission must fail");
        assert!(error.message.contains("capture failed"));
        assert!(error.message.contains("inspect the pane before retrying"));
    }

    #[test]
    fn send_text_fails_after_max_pending_retries() {
        let runner = send_text_runner(vec![
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
        let error = client
            .send_text_with_sleeper("sess:oracle.0", "deploy", |duration| sleeps.push(duration))
            .expect_err("pending input must fail delivery");
        assert!(error.message.contains("delivery could not be confirmed"));
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
    fn send_text_does_not_ignore_initial_different_pending_input() {
        let runner = send_text_runner(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("❯ different queued input"),
            Ok("❯ "),
        ]);
        let mut client = TmuxClient::new(runner);
        let mut sleeps = Vec::new();
        let error = client
            .send_text_with_sleeper("sess:oracle.0", "deploy", |duration| sleeps.push(duration))
            .expect_err("different pending input must fail delivery");

        assert!(error.message.contains("inspect the pane before retrying"));
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
    ) -> (Result<SendTextReport, TmuxError>, FakeRunner) {
        let mut responses = vec![Ok("0"), Ok(""), Ok("")];
        responses.append(&mut after_paste);
        let runner = send_text_runner(responses);
        let mut client = TmuxClient::new(runner);
        let report = client.send_text_with_sleeper("sess:oracle.0", BUFFERED_TEXT, |_| {});
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
        let report = report.expect("send text ok");

        assert_eq!((report.enter_attempts, report.warned_pending), (2, false));
        assert_eq!(runner.calls[2].0, "paste-buffer");
        assert_eq!(runner.calls[3].0, "capture-pane");
        assert_eq!(runner.calls[4].0, "send-keys");
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
        let report = report.expect("send text ok");

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

        assert!(report.expect_err("different input must fail").message.contains("not be confirmed"));
    }

    #[test]
    fn send_text_buffered_baseline_capture_failure_fails_closed() {
        let (report, _) = send_text_buffered_case(vec![
            Err(TmuxError::new("capture failed")),
            Ok(""),
            Ok(BUFFERED_PLACEHOLDER),
            Ok(BUFFERED_PLACEHOLDER),
        ]);

        assert!(report.expect_err("unknown baseline must fail").message.contains("not be confirmed"));
    }

    #[test]
    fn send_text_waits_out_matching_redraw_before_retrying() {
        let runner = send_text_runner(vec![
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
        let runner = send_text_runner(vec![
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
