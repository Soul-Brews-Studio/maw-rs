
    #[test]
    fn peer_url_lookup_prefers_named_peer_then_peer_substring() {
        let config = MawConfig {
            named_peers: vec![NamedPeer {
                name: "white".to_owned(),
                url: "http://white".to_owned(),
            }],
            peers: vec!["http://mba:3456".to_owned()],
            ..MawConfig::default()
        };

        assert_eq!(
            find_peer_url("white", &config),
            Some("http://white".to_owned())
        );
        assert_eq!(
            find_peer_url("mba", &config),
            Some("http://mba:3456".to_owned())
        );
        assert_eq!(find_peer_url("ghost", &config), None);
    }

    #[test]
    fn oracle_suffixed_alias_is_left_for_agent_routing() {
        let sessions = vec![session("mawjs", vec![window(0, "mawjs")])];
        assert!(
            resolve_session_alias_window_target("mawjs-oracle", &sessions, RouteType::Local)
                .is_none()
        );
    }

    #[test]
    fn declared_project_window_overrides_oracle_suffix_alias_guard() {
        let sessions = vec![session(
            "bar-oracle",
            vec![window_with_kind(0, "bar-oracle", RepoKind::Project)],
        )];

        assert_eq!(
            resolve_session_alias_window_target("bar-oracle", &sessions, RouteType::Local),
            Some(ResolveResult::Local {
                target: "bar-oracle:0".to_owned(),
            })
        );
    }

    #[test]
    fn declared_oracle_window_without_suffix_keeps_oracle_suffix_guard() {
        let sessions = vec![session(
            "foo",
            vec![window_with_kind(0, "foo", RepoKind::Oracle)],
        )];

        assert!(
            resolve_session_alias_window_target("foo-oracle", &sessions, RouteType::Local)
                .is_none()
        );
    }

    #[test]
    fn find_window_preserves_pane_suffix_for_named_window() {
        let sessions = vec![session("dev", vec![window(5, "main")])];

        assert_eq!(
            find_window(&sessions, "dev:main.2"),
            Some("dev:5.2".to_owned())
        );
    }

