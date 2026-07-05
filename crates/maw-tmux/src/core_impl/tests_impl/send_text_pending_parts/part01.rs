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
