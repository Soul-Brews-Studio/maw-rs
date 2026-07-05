    use super::*;

    fn window(index: u32, name: &str) -> Window {
        Window {
            index,
            name: name.to_owned(),
            active: index == 0,
            kind: None,
        }
    }

    fn window_with_kind(index: u32, name: &str, kind: RepoKind) -> Window {
        Window {
            index,
            name: name.to_owned(),
            active: index == 0,
            kind: Some(kind),
        }
    }

    fn session(name: &str, windows: Vec<Window>) -> Session {
        Session {
            name: name.to_owned(),
            windows,
            source: None,
        }
    }

    fn config_with_node(node: &str) -> MawConfig {
        MawConfig {
            node: Some(node.to_owned()),
            ..MawConfig::default()
        }
    }

    #[test]
    fn sync_apply_skips_conflicts_and_stale_without_force_or_prune() {
        let diff = SyncDiff {
            add: Vec::new(),
            conflict: vec![SyncConflict {
                oracle: "mawjs".to_owned(),
                current: "mba".to_owned(),
                proposed: "white".to_owned(),
                from_peer: "white".to_owned(),
            }],
            stale: vec![StaleRoute {
                oracle: "old".to_owned(),
                peer_node: "white".to_owned(),
            }],
            unreachable: Vec::new(),
        };
        let current = HashMap::from([
            ("mawjs".to_owned(), "mba".to_owned()),
            ("old".to_owned(), "white".to_owned()),
        ]);

        let result = apply_sync_diff(&current, &diff, SyncApplyOptions::default());

        assert_eq!(result.agents, current);
        assert!(result.applied.is_empty());
    }

    #[test]
    fn invalid_node_agent_query_reports_empty_side() {
        assert_eq!(
            resolve_target(":ghost", &config_with_node("white"), &[]),
            ResolveResult::Error {
                reason: "empty_node_or_agent".to_owned(),
                detail: "invalid format: ':ghost'".to_owned(),
                hint: Some("use node:agent format (e.g. mba:homekeeper)".to_owned()),
            }
        );
    }

    #[test]
    fn self_node_alias_returns_self_node_target() {
        let sessions = vec![session("pulse", vec![window(3, "pulse")])];

        assert_eq!(
            resolve_target("white:pulse", &config_with_node("white"), &sessions),
            ResolveResult::SelfNode {
                target: "pulse:3".to_owned(),
            }
        );
    }

    #[test]
    fn self_target_alias_resolves_numbered_current_session_oracle_window() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(0, "work"),
                window(1, "maw-rs-oracle"),
                window(2, "maw-rs-codex-6"),
            ],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "ME",
                &MawConfig::default(),
                &sessions,
                Some("188-maw-rs")
            ),
            ResolveResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
    }

    #[test]
    fn self_target_alias_resolves_unnumbered_current_session_oracle_window() {
        let sessions = vec![session(
            "mawjs",
            vec![window(0, "shell"), window(4, "mawjs-oracle")],
        )];

        assert_eq!(
            resolve_target_with_current_session(
                "me",
                &MawConfig::default(),
                &sessions,
                Some("mawjs")
            ),
            ResolveResult::Local {
                target: "mawjs:4".to_owned()
            }
        );
    }

    #[test]
    fn self_target_alias_skips_declared_project_oracle_suffix_window() {
        let sessions = vec![session(
            "188-maw-rs",
            vec![
                window(0, "work"),
                window_with_kind(1, "maw-rs-oracle", RepoKind::Project),
                window_with_kind(4, "maw-rs-codex-6", RepoKind::Oracle),
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
                target: "188-maw-rs:4".to_owned()
            }
        );
    }
