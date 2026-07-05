
    #[test]
    fn self_target_alias_reports_no_oracle_when_suffix_window_is_declared_project() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(0, "work"),
                window_with_kind(1, "maw-rs-oracle", RepoKind::Project),
            ],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Error {
                reason: "me_oracle_window_not_found".to_owned(),
                detail: "'me' resolved current tmux session '188-maw-rs', but no *-oracle window was found".to_owned(),
                hint: Some(
                    "windows: 188-maw-rs:0 (work), 188-maw-rs:1 (maw-rs-oracle)"
                        .to_owned()
                ),
            }
        );
    }

    #[test]
    fn self_target_alias_matches_declared_oracle_window_without_suffix() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(0, "work"),
                window_with_kind(3, "maw-rs-codex-6", RepoKind::Oracle),
            ],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Local {
                target: "188-maw-rs:3".to_owned()
            }
        );
    }

    #[test]
    fn self_target_alias_reports_no_oracle_window_with_session_windows() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![window(0, "work"), window(2, "maw-rs-codex-6")],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Error {
                reason: "me_oracle_window_not_found".to_owned(),
                detail: "'me' resolved current tmux session '188-maw-rs', but no *-oracle window was found".to_owned(),
                hint: Some(
                    "windows: 188-maw-rs:0 (work), 188-maw-rs:2 (maw-rs-codex-6)"
                        .to_owned()
                ),
            }
        );
    }

    #[test]
    fn self_target_alias_reports_outside_tmux_context() {
        assert_eq!(
            resolve_target_with_current_session("me", &MawConfig::default(), &[], None),
            ResolveResult::Error {
                reason: "me_needs_tmux".to_owned(),
                detail: "'me' needs a tmux context".to_owned(),
                hint: Some(
                    "run inside tmux so maw can resolve the current session".to_owned()
                ),
            }
        );
    }

    #[test]
    fn literal_me_window_is_reachable_with_full_session_form() {
        let sessions = vec![session(
            "scratch",
            vec![window(0, "shell"), window(3, "me")],
        )];

        assert_eq!(
            resolve_target("scratch:me", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "scratch:3".to_owned()
            }
        );
    }

    #[test]
    fn exact_unnumbered_session_breaks_alias_tie() {
        let sessions = vec![
            session("47-mawjs", vec![window(0, "mawjs")]),
            session("mawjs-oracle", vec![window(2, "mawjs")]),
        ];

        assert_eq!(
            resolve_target("mawjs", &MawConfig::default(), &sessions),
            ResolveResult::Local {
                target: "47-mawjs:0".to_owned(),
            }
        );
    }

    #[test]
    fn blank_alias_and_numeric_prefixed_candidates_are_defensive() {
        assert!(resolve_session_alias_window_target("   ", &[], RouteType::Local).is_none());
        assert_eq!(fleet_window_candidate_names(""), Vec::<String>::new());
        assert_eq!(
            fleet_window_candidate_names("mawjs"),
            vec!["mawjs", "mawjs-oracle"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fleet_window_candidate_names("47-mawjs-oracle"),
            vec!["47-mawjs-oracle", "47-mawjs", "mawjs-oracle", "mawjs"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn find_window_covers_colon_fallthrough_edges() {
        let sessions = vec![session("dev", Vec::new())];
        assert_eq!(find_window(&sessions, "dev:"), Some("dev:".to_owned()));
        assert_eq!(find_window(&sessions, "dev:nope"), None);
    }

    #[test]
    fn find_window_supports_colon_first_window_and_numeric_fallbacks() {
        let sessions = vec![session("dev", vec![window(5, "main")])];

        assert_eq!(find_window(&sessions, "dev:"), Some("dev:5".to_owned()));
        assert_eq!(find_window(&sessions, "dev:4"), Some("dev:4".to_owned()));
        assert_eq!(
            find_window(&sessions, "dev:4.2"),
            Some("dev:4.2".to_owned())
        );
    }
