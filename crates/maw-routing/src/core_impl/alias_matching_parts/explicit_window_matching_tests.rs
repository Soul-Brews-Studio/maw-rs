
    #[test]
    fn find_window_prefers_colon_index_over_numeric_window_name_suffix() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(1, "maw-rs-oracle"),
                window(2, "maw-rs-codex-1"),
            ],
        )];

        assert_eq!(
            resolve_target("188-maw-rs:1", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
        assert_eq!(
            resolve_target("maw-rs:1", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
    }

    #[test]
    fn find_window_prefers_exact_full_window_name_over_substring() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(2, "maw-rs-codex-10"),
                window(1, "maw-rs-codex-1"),
            ],
        )];

        assert_eq!(
            resolve_target(
                "188-maw-rs:maw-rs-codex-1",
                &MawConfig::default(),
                &sessions
            ),
            ResolveResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
    }

    #[test]
    fn explicit_session_window_pins_duplicate_names_to_named_session() {
        let sessions = vec![
            session("webhook-relay-v3", vec![window(2, "codex-1")]),
            session("arra-oracle-v3", vec![window(4, "codex-1")]),
        ];

        assert_eq!(
            resolve_target("webhook-relay-v3:codex-1", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "webhook-relay-v3:2".to_owned()
            }
        );
    }

    #[test]
    fn explicit_session_window_miss_is_loud_and_never_cross_session() {
        let sessions = vec![
            session("webhook-relay-v3", vec![window(0, "oracle")]),
            session("arra-oracle-v3", vec![window(4, "codex-1")]),
        ];

        assert_eq!(
            resolve_target("webhook-relay-v3:codex-1", &MawConfig::default(), &sessions),
            ResolveResult::Error {
                reason: "session_window_not_found".to_owned(),
                detail: "no window 'codex-1' in session 'webhook-relay-v3'".to_owned(),
                hint: Some("windows: webhook-relay-v3:0 (oracle)".to_owned()),
            }
        );
    }

    #[test]
    fn explicit_session_window_does_not_substring_match_inside_session() {
        let sessions = vec![session(
            "webhook-relay-v3",
            vec![window(4, "webhook-codex-10")],
        )];

        assert_eq!(
            resolve_target("webhook-relay-v3:codex-1", &MawConfig::default(), &sessions),
            ResolveResult::Error {
                reason: "session_window_not_found".to_owned(),
                detail: "no window 'codex-1' in session 'webhook-relay-v3'".to_owned(),
                hint: Some("windows: webhook-relay-v3:4 (webhook-codex-10)".to_owned()),
            }
        );
    }

    #[test]
    fn find_window_refuses_ambiguous_exact_session_or_window_matches() {
        let duplicate_sessions = vec![
            session("47-mawjs", vec![window(0, "left")]),
            session("99-mawjs", vec![window(1, "right")]),
        ];
        assert_eq!(find_window(&duplicate_sessions, "mawjs"), None);

        let duplicate_windows = vec![
            session("alpha", vec![window(0, "oracle")]),
            session("bravo", vec![window(0, "oracle")]),
        ];
        assert_eq!(find_window(&duplicate_windows, "oracle"), None);
    }

    #[test]
    fn find_window_uses_unique_substring_window_or_session_match() {
        let window_match = vec![session("alpha", vec![window(9, "mawjs-codex")])];
        assert_eq!(
            find_window(&window_match, "codex"),
            Some("alpha:9".to_owned())
        );

        let session_match = vec![session("mawjs-session", vec![window(4, "main")])];
        assert_eq!(
            find_window(&session_match, "session"),
            Some("mawjs-session:4".to_owned())
        );
        assert_eq!(
            find_window(&[session("empty-session", Vec::new())], "empty"),
            None
        );

        let ambiguous = vec![
            session("alpha", vec![window(0, "mawjs-left")]),
            session("bravo-mawjs", vec![window(1, "main")]),
        ];
        assert_eq!(find_window(&ambiguous, "mawjs"), None);
    }

    #[test]
    fn find_window_direct_paths_cover_unique_exact_and_strict_fallbacks() {
        let sessions = vec![session("alpha", vec![window(7, "main")])];
        assert_eq!(find_window(&sessions, "alpha"), Some("alpha:7".to_owned()));
        assert_eq!(
            find_window(&sessions, "alpha:9"),
            Some("alpha:9".to_owned())
        );
        assert_eq!(
            match_session(&sessions, "alp", false).map(|session| session.name.as_str()),
            Some("alpha")
        );
    }

    #[test]
    fn helper_functions_cover_non_matching_edges() {
        assert_eq!(match_session(&[], "", true), None);
        assert_eq!(split_pane_suffix("main."), ("main.", String::new()));
        assert_eq!(split_pane_suffix("main.x"), ("main.x", String::new()));
        assert!(!numeric_window_or_pane(""));
        assert!(!numeric_window_or_pane("1."));
        assert!(!numeric_window_or_pane("x.1"));
        assert_eq!(strip_numeric_fleet_prefix("mawjs"), "mawjs");
        assert_eq!(strip_numeric_fleet_prefix("dev-mawjs"), "dev-mawjs");
    }
