
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
