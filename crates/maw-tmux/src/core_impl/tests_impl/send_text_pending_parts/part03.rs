
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
