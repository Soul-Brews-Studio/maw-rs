
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
