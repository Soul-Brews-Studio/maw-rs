    #[test]
    fn tmux_kill_fallback_reports_ambiguous_pane_aliases() {
        let raw = [
            "%71|||demo:2.0|||codex||||||/repos/a",
            "%72|||demo:3.0|||codex||||||/repos/b",
        ]
        .join("\n");
        let error =
            resolve_kill_target_with_pane_fallback("codex", "codex", "session-name", false, &raw)
                .expect_err("ambiguous alias refused");
        assert!(error
            .message
            .contains("'codex' is ambiguous — matches 2 panes:"));
        assert!(error
            .message
            .contains("• codex → %71 (demo:2.0) [pane-title]"));
        assert!(error
            .message
            .contains("• codex → %72 (demo:3.0) [pane-title]"));

        let preserved =
            resolve_kill_target_with_pane_fallback("codex", "codex", "session-name", true, &raw)
                .expect("session kill does not fallback");
        assert_eq!(
            preserved,
            TmuxKillTarget {
                resolved: "codex".to_owned(),
                source: "session-name".to_owned(),
            }
        );
    }

    #[test]
    fn tmux_ls_recent_pure_helpers_match_maw_js_tests() {
        let raw =
            "old-session\t100\nnew-session\t300\nmid-session\t200\nzero\t0\nbad\tnope\nmissing\n";
        assert_eq!(
            parse_session_created_list(raw),
            BTreeMap::from([
                ("mid-session".to_owned(), 200),
                ("new-session".to_owned(), 300),
                ("old-session".to_owned(), 100),
            ])
        );
        assert_eq!(format_session_created(None), "—");
        assert_eq!(format_session_created(Some(0)), "—");
        assert_eq!(format_session_created(Some(300)), "1970-01-01 00:05:00");
        assert_eq!(parse_active_duration_seconds(Some("30m")), Some(1800));
        assert_eq!(parse_active_duration_seconds(Some("1h")), Some(3600));
        assert_eq!(parse_active_duration_seconds(Some("2d")), Some(172_800));
        assert_eq!(parse_active_duration_seconds(Some("45")), Some(2700));
        assert_eq!(parse_active_duration_seconds(Some("0m")), None);
        assert_eq!(
            active_duration_arg(&["--active".to_owned(), "1h".to_owned()], "--active"),
            Some("1h".to_owned())
        );
        assert_eq!(
            active_duration_arg(&["--active=2d".to_owned()], "--active"),
            Some("2d".to_owned())
        );
        assert_eq!(
            active_duration_arg(
                &["--active".to_owned(), "session-filter".to_owned()],
                "--active"
            ),
            None
        );
    }

    #[test]
    fn annotate_pane_matches_maw_js_precedence() {
        let fleet = BTreeSet::from([
            "101-mawjs".to_owned(),
            "112-fusion".to_owned(),
            "114-mawjs-no2".to_owned(),
        ]);
        let teams = BTreeMap::from([("%300".to_owned(), "scout @ iter-triage".to_owned())]);
        assert_eq!(
            annotate_pane(
                &TmuxLsPaneRef {
                    id: "%100".to_owned(),
                    target: "101-mawjs:0.0".to_owned(),
                    command: Some("claude".to_owned())
                },
                &fleet,
                &BTreeMap::new(),
            ),
            "fleet: mawjs"
        );
        assert_eq!(
            annotate_pane(
                &TmuxLsPaneRef {
                    id: "%101".to_owned(),
                    target: "114-mawjs-no2:0.0".to_owned(),
                    command: Some("claude".to_owned())
                },
                &fleet,
                &BTreeMap::new(),
            ),
            "fleet: mawjs-no2"
        );
        assert_eq!(
            annotate_pane(
                &TmuxLsPaneRef {
                    id: "%200".to_owned(),
                    target: "maw-view:0.0".to_owned(),
                    command: Some("claude".to_owned())
                },
                &fleet,
                &BTreeMap::new(),
            ),
            "view: maw-view"
        );
        assert_eq!(
            annotate_pane(
                &TmuxLsPaneRef {
                    id: "%201".to_owned(),
                    target: "mawjs-view:0.0".to_owned(),
                    command: Some("claude".to_owned())
                },
                &fleet,
                &BTreeMap::new(),
            ),
            "view: mawjs-view"
        );
        assert_eq!(
            annotate_pane(
                &TmuxLsPaneRef {
                    id: "%300".to_owned(),
                    target: "101-mawjs:0.1".to_owned(),
                    command: Some("bun".to_owned())
                },
                &fleet,
                &teams,
            ),
            "team: scout @ iter-triage"
        );
        assert_eq!(
            annotate_pane(
                &TmuxLsPaneRef {
                    id: "%600".to_owned(),
                    target: "view-foo:0.0".to_owned(),
                    command: Some("claude".to_owned())
                },
                &fleet,
                &BTreeMap::new(),
            ),
            "orphan"
        );
        assert_eq!(
            annotate_pane(
                &TmuxLsPaneRef {
                    id: "%700".to_owned(),
                    target: "any:0.0".to_owned(),
                    command: Some("bash".to_owned())
                },
                &BTreeSet::new(),
                &BTreeMap::new(),
            ),
            ""
        );
    }

    #[test]
    fn similar_oracle_candidates_preserve_org_slug_ambiguity() {
        let repos = vec![
            "/opt/Code/github.com/laris-co/pulse-oracle".to_owned(),
            "/opt/Code/github.com/Soul-Brews-Studio/pulse-oracle".to_owned(),
            "/opt/Code/github.com/Soul-Brews-Studio/pulse-oracle".to_owned(),
            "/opt/Code/github.com/Soul-Brews-Studio/other".to_owned(),
        ];
        assert_eq!(
            similar_oracle_candidates_from_repos("pulse", &repos),
            vec![
                "laris-co/pulse-oracle".to_owned(),
                "Soul-Brews-Studio/pulse-oracle".to_owned(),
            ]
        );
        assert!(similar_oracle_candidates_from_repos("x", &[]).is_empty());
    }

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

    #[test]
    fn pending_input_detection_matches_codex_capture_states() {
        let idle_empty = [
            "─ Worked for 20m 43s ─────────────────────────",
            "",
            "› ",
            "",
            "  gpt-5.5 xhigh · agents/godui-streaming · Context left",
        ]
        .join("\n");
        assert_eq!(pane_pending_input_from_capture(&idle_empty), None);

        let pending = [
            "─ Worked for 20m 43s ─────────────────────────",
            "",
            "\u{1b}[1m›\u{1b}[0m Explain this codebase",
            "",
            "  gpt-5.5 xhigh · agents/godui-streaming · Context left",
        ]
        .join("\n");
        assert_eq!(
            pane_pending_input_from_capture(&pending),
            Some("Explain this codebase".to_owned())
        );

        let just_submitted = [
            "› Explain this codebase",
            "• Working (1m 21s • esc to interrupt)",
            "",
            "  gpt-5.5 xhigh · agents/godui-streaming · Context left",
        ]
        .join("\n");
        assert_eq!(pane_pending_input_from_capture(&just_submitted), None);

        let busy_queued = [
            "• Working (1m 21s • esc to interrupt)",
            "",
            "› Find and fix a bug in @filename",
            "",
            "  gpt-5.5 xhigh · agents/send-enter-fix · Context left",
        ]
        .join("\n");
        assert_eq!(
            pane_pending_input_from_capture(&busy_queued),
            Some("Find and fix a bug in @filename".to_owned())
        );
    }

    #[test]
    fn pending_input_detection_matches_claude_capture_states() {
        let idle_empty = [
            "✻ Twisting… (4m 34s)",
            "────────────────────────────────────────",
            "❯\u{a0}",
            "────────────────────────────────────────",
            "  🖥 m5 · 3.5 Sonnet · 60%",
            "  📡 188-maw-rs:1.0",
        ]
        .join("\n");
        assert_eq!(pane_pending_input_from_capture(&idle_empty), None);

        let pending = [
            "field report duplicate delivery 2x",
            "────────────────────────────────────────",
            "❯\u{a0}dispatch broadcast ledger ให้ทีม gen-2 เลยครับ",
            "────────────────────────────────────────",
            "  🖥 m5 · 3.5 Sonnet · 60%",
            "  📡 183-crew-master:1.0",
        ]
        .join("\n");
        assert_eq!(
            pane_pending_input_from_capture(&pending),
            Some("dispatch broadcast ledger ให้ทีม gen-2 เลยครับ".to_owned())
        );

        let just_submitted = [
            "❯ REBOOT CANCELLED",
            "✻ Twisting… (0s)",
            "────────────────────────────────────────",
            "  🖥 m5 · 3.5 Sonnet · 60%",
        ]
        .join("\n");
        assert_eq!(pane_pending_input_from_capture(&just_submitted), None);

        let busy_queued = [
            "✻ Twisting… (4m 34s)",
            "────────────────────────────────────────",
            "❯\u{a0}ok แจกงานให้ทีมได้เลย #121",
            "────────────────────────────────────────",
            "  🖥 m5 · 3.5 Sonnet · 60%",
            "  📡 142-athena:1.0",
        ]
        .join("\n");
        assert_eq!(
            pane_pending_input_from_capture(&busy_queued),
            Some("ok แจกงานให้ทีมได้เลย #121".to_owned())
        );
    }

    #[test]
    fn pending_input_detection_ignores_historical_prompt_lines() {
        let history_with_current_empty_prompt = [
            "❯ ทฟไ ะนาำ",
            "⏺ Error: command not found",
            "❯ maw token use nh2",
            "⏺ Token usage printed",
            "────────────────────────────────────────",
            "❯\u{a0}",
            "────────────────────────────────────────",
            "  🖥 m5 · 3.5 Sonnet · 60%",
        ]
        .join("\n");
        assert_eq!(
            pane_pending_input_from_capture(&history_with_current_empty_prompt),
            None
        );
    }

    #[test]
    fn pending_input_matching_is_duplicate_safe() {
        assert!(pending_input_matches_sent("deploy now", "deploy now"));
        assert!(pending_input_matches_sent(
            "first line",
            "first line\nsecond line"
        ));
        assert!(pending_input_matches_sent("deploy now", "\u{a0}deploy now\r"));
        assert!(!pending_input_matches_sent("different queued input", "deploy now"));
        assert_eq!(
            pending_input_state_from_capture("❯ deploy now", "deploy now"),
            PendingInputState::MatchesSent
        );
        assert_eq!(
            pending_input_state_from_capture("❯ different queued input", "deploy now"),
            PendingInputState::DifferentInput
        );
    }

    #[test]
    fn client_fail_soft_lists_and_records_runner_args() {
        let runner =
            FakeRunner::with_responses(vec![Ok("s1\ns2\n"), Err(TmuxError::new("no server"))]);
        let mut client = TmuxClient::new(runner);
        assert_eq!(client.list_session_names(), vec!["s1", "s2"]);
        assert!(client.list_all().is_empty());
        assert_eq!(client.runner.calls[0].0, "list-sessions");
        assert_eq!(client.runner.calls[1].0, "list-windows");
    }

    #[test]
    fn client_listing_helpers_parse_outputs_and_fail_soft_where_expected() {
        let runner = FakeRunner::with_responses(vec![
            Ok("0:agent:1\n1:logs:0\n"),
            Ok("%1\n\n%2\n"),
            Err(TmuxError::new("no panes")),
            Ok("%1|||zsh|||s:agent.0|||main|||42|||/repo|||900\n"),
            Ok(""),
            Err(TmuxError::new("missing")),
        ]);
        let mut client = TmuxClient::new(runner);

        assert_eq!(
            client.list_windows("s").expect("windows parse"),
            vec![
                TmuxWindow {
                    index: 0,
                    name: "agent".to_owned(),
                    active: true,
                    cwd: None,
                },
                TmuxWindow {
                    index: 1,
                    name: "logs".to_owned(),
                    active: false,
                    cwd: None,
                },
            ]
        );
        assert_eq!(
            client.list_pane_ids(),
            BTreeSet::from(["%1".to_owned(), "%2".to_owned()])
        );
        assert!(client.list_pane_ids().is_empty());
        assert_eq!(
            client.list_panes(),
            vec![TmuxPane {
                id: "%1".to_owned(),
                command: "zsh".to_owned(),
                target: "s:agent.0".to_owned(),
                title: "main".to_owned(),
                pid: Some(42),
                cwd: Some("/repo".to_owned()),
                last_activity: Some(900),
            }]
        );
        assert!(client.has_session("s"));
        assert!(!client.has_session("ghost"));

        assert_eq!(client.runner.calls[0].0, "list-windows");
        assert_eq!(client.runner.calls[1].0, "list-panes");
        assert_eq!(client.runner.calls[2].0, "list-panes");
        assert_eq!(client.runner.calls[3].0, "list-panes");
        assert_eq!(client.runner.calls[4].0, "has-session");
        assert_eq!(client.runner.calls[5].0, "has-session");
    }
