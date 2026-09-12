
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cold_table_refuses_socket_aliases_and_unlinked_names() {
        let dir = std::env::temp_dir().join(format!("maw-941-alias-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let path = dir.join("CaseProbe.sock");
        let alias = dir.join("alias.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).expect("bind fixture");
        std::fs::hard_link(&path, &alias).expect("hard link fixture");
        let header = "Active LOCAL (UNIX) domain sockets\nAddress Type Recv-Q Send-Q Inode Conn Refs Nextref Addr\n";
        let table = format!("{header}1 stream 0 0 0 0 0 0 {}\n", path.display());
        assert!(!macos_socket_table_is_cold(alias.to_str().unwrap(), &table));
        let case_alias = dir.join("caseprobe.sock");
        if case_alias.exists() {
            assert!(!macos_socket_table_is_cold(case_alias.to_str().unwrap(), &table));
        }
        let absent = dir.join("absent.sock");
        assert!(macos_socket_table_is_cold(absent.to_str().unwrap(), &table));
        std::fs::remove_file(&path).expect("unlink original bind name");
        assert!(!macos_socket_table_is_cold(alias.to_str().unwrap(), &table));
        assert!(!macos_socket_table_is_cold(absent.to_str().unwrap(), &table));
        drop(listener);
        assert!(macos_socket_table_is_cold(alias.to_str().unwrap(), header));
        std::fs::remove_file(alias).expect("remove alias");
        std::fs::remove_dir(dir).expect("cleanup fixture");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cold_table_refuses_incomplete_unstable_and_bound_snapshots() {
        let header = "Active LOCAL (UNIX) domain sockets\nAddress Type Recv-Q Send-Q Inode Conn Refs Nextref Addr\n";
        let socket = "/tmp/maw-941-table.sock";
        assert!(macos_socket_table_is_cold(socket, header));
        for bad in ["", "permission denied", "Active LOCAL (UNIX) domain sockets\n"] {
            assert!(!macos_socket_table_is_cold(socket, bad));
        }
        for row in [
            "Some stream sockets may have been created.",
            "broken row",
            "1 stream 0 0 0 0 0 0 /tmp/maw-941-table.sock",
            "1 stream 0 0 0 0 0 0 /private/tmp/maw-941-table.sock",
            "1 stream 0 0 0 0 0 0 relative-socket",
        ] {
            assert!(!macos_socket_table_is_cold(socket, &format!("{header}{row}\n")), "{row}");
        }
        assert!(!macos_socket_table_is_cold(socket, &format!("{header}1 stream 0 0 0 0 0 0 /tmp/maw-941-missing-parent/different.sock\n")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "isolated kernel-inventory smoke; unrelated socket churn intentionally refuses proof"]
    fn macos_cold_probe_distinguishes_bound_unlinked_from_closed() {
        let dir = std::env::temp_dir().join(format!("maw-941-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let path = dir.join("probe.sock");
        let socket = path.to_str().expect("fixture path");
        let listener = std::os::unix::net::UnixListener::bind(&path).expect("bind fixture");
        assert!(!tmux_socket_is_proven_cold(socket));
        std::fs::remove_file(&path).expect("unlink fixture");
        assert!(!tmux_socket_is_proven_cold(socket), "live unlinked must remain fatal");
        drop(listener);
        // Concurrent unrelated socket churn makes netstat report an unstable
        // snapshot; the production probe must refuse it. Retry observation only.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !tmux_socket_is_proven_cold(socket) {
            assert!(std::time::Instant::now() < deadline, "closed socket needs a stable kernel snapshot");
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        std::fs::remove_dir(&dir).expect("cleanup fixture");
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

        let enoent = TmuxError::new(
            "tmux exited with status 1: error connecting to /definitely/missing/maw-940.sock (No such file or directory)",
        );
        assert!(enoent.is_cold_start());
        assert!(TmuxError::new("tmux exited with status 1: no server running on /tmp/tmux-1/default").is_cold_start());
        for fatal in [
            "no server running on /tmp/tmux-1/default",
            "tmux exited with status 1: no server running on /tmp/evil\nsocket",
            "tmux exited with status 2: no server running on /tmp/tmux-1/default",
            "tmux exited with status 1: error connecting to /tmp/tmux-1/default (Connection refused)",
            "tmux exited with status 1: error connecting to /tmp/tmux-1/default (Permission denied)",
            "tmux exited with status 1: server exited unexpectedly",
            "failed to execute tmux: No such file or directory",
        ] {
            assert!(!TmuxError::new(fatal).is_cold_start(), "{fatal}");
        }

        let mut runner = CommandTmuxRunner {
            program: OsString::from("tmux"),
            socket: None,
            server_observed: false,
            initial_cold_start_allowed: true,
        };
        let borrowed = &mut runner;
        #[cfg(target_os = "linux")]
        assert!(TmuxRunner::is_initial_cold_start(&borrowed, &enoent));
        #[cfg(not(target_os = "linux"))]
        assert!(!TmuxRunner::is_initial_cold_start(&borrowed, &enoent));
        let stale = TmuxError::new("tmux exited with status 1: no server running on /dev/null");
        assert!(!runner.is_initial_cold_start(&stale));
        #[cfg(target_os = "linux")]
        {
            let socket = std::env::temp_dir().join(format!(
                "maw-940-unlinked-live-{}.sock",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&socket);
            drop(std::os::unix::net::UnixListener::bind(&socket).expect("bind stale socket"));
            let stale = TmuxError::new(format!("tmux exited with status 1: no server running on {}", socket.display()));
            let can_bootstrap = runner.is_initial_cold_start(&stale);
            std::fs::remove_file(&socket).expect("remove stale socket");
            assert!(can_bootstrap, "unbound socket can bootstrap");
            let listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind socket");
            std::fs::remove_file(&socket).expect("unlink live socket");
            let unlinked = TmuxError::new(format!(
                "tmux exited with status 1: error connecting to {} (No such file or directory)",
                socket.display()
            ));
            assert!(!runner.is_initial_cold_start(&unlinked), "#860 live socket");
            drop(listener);
        }
        runner.server_observed = true;
        assert!(
            !runner.is_initial_cold_start(&enoent),
            "ENOENT after a reachable server is fatal"
        );
        runner.server_observed = false;
        runner.initial_cold_start_allowed = false;
        assert!(
            !runner.is_initial_cold_start(&enoent),
            "ENOENT in an attached TMUX client is fatal (#860)"
        );

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
