
    #[test]
    fn tmux_attach_action_branches_match_maw_js_cases() {
        let alive = BTreeSet::from(["some-session".to_owned()]);
        assert_eq!(
            decide_tmux_attach_action(
                "%999",
                &BTreeSet::from(["%999".to_owned()]),
                true,
                true,
                false
            ),
            TmuxAttachAction::Print {
                session: "%999".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("some-session:0.1", &alive, true, true, false),
            TmuxAttachAction::Print {
                session: "some-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("some-session:0.1", &alive, false, false, false),
            TmuxAttachAction::Print {
                session: "some-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("some-session:0.1", &alive, false, true, true),
            TmuxAttachAction::SwitchClient {
                session: "some-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("some-session:0.1", &alive, false, true, false),
            TmuxAttachAction::Attach {
                session: "some-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action("ghost-session", &alive, false, true, false),
            TmuxAttachAction::Recover {
                session: "ghost-session".to_owned()
            }
        );
        assert_eq!(
            decide_tmux_attach_action(
                "pulse",
                &BTreeSet::from(["01-pulse".to_owned(), "02-pulse".to_owned()]),
                false,
                true,
                false
            ),
            TmuxAttachAction::Recover {
                session: "pulse".to_owned()
            }
        );

        assert_eq!(
            tmux_attach_spawn_command(&TmuxAttachAction::SwitchClient {
                session: "some-session".to_owned()
            }),
            Some(SpawnCommand {
                program: "tmux".to_owned(),
                args: vec![
                    "switch-client".to_owned(),
                    "-t".to_owned(),
                    "some-session".to_owned()
                ],
            })
        );
        assert_eq!(
            tmux_attach_spawn_command(&TmuxAttachAction::Attach {
                session: "some-session".to_owned()
            }),
            Some(SpawnCommand {
                program: "tmux".to_owned(),
                args: vec![
                    "attach".to_owned(),
                    "-t".to_owned(),
                    "some-session".to_owned()
                ],
            })
        );
        assert_eq!(
            tmux_attach_spawn_command(&TmuxAttachAction::Print {
                session: "some-session".to_owned()
            }),
            None
        );
    }

    #[test]
    fn tmux_attach_session_resolution_prefers_exact_then_fuzzy() {
        let alive = BTreeSet::from([
            "05-volt".to_owned(),
            "mawjs-codex".to_owned(),
            "50-mawjs-codex".to_owned(),
            "volt".to_owned(),
        ]);
        assert_eq!(
            resolve_tmux_attach_session("volt", &alive),
            TmuxAttachSessionResolution::Match {
                session: "volt".to_owned()
            }
        );
        assert_eq!(
            resolve_tmux_attach_session("mawjscodex", &alive),
            TmuxAttachSessionResolution::Match {
                session: "50-mawjs-codex".to_owned()
            }
        );

        let only_numbered = BTreeSet::from(["05-volt".to_owned()]);
        assert_eq!(
            resolve_tmux_attach_session("volt", &only_numbered),
            TmuxAttachSessionResolution::Match {
                session: "05-volt".to_owned()
            }
        );

        let numbered_preferred = BTreeSet::from(["05-volt".to_owned(), "volt-oracle".to_owned()]);
        assert_eq!(
            resolve_tmux_attach_session("volt", &numbered_preferred),
            TmuxAttachSessionResolution::Match {
                session: "05-volt".to_owned()
            }
        );
    }

    #[test]
    fn tmux_attach_session_resolution_matches_numeric_fleet_prefix() {
        let alive = BTreeSet::from([
            "187-sentinel".to_owned(),
            "188-maw-rs".to_owned(),
            "189-crew-master".to_owned(),
        ]);
        assert_eq!(
            resolve_tmux_attach_session("188", &alive),
            TmuxAttachSessionResolution::Match {
                session: "188-maw-rs".to_owned()
            }
        );
        assert_eq!(
            resolve_tmux_attach_session("188-maw", &alive),
            TmuxAttachSessionResolution::Match {
                session: "188-maw-rs".to_owned()
            }
        );
        assert_eq!(
            resolve_tmux_attach_session("18", &alive),
            TmuxAttachSessionResolution::Ambiguous {
                query: "18".to_owned(),
                candidates: vec![
                    "187-sentinel".to_owned(),
                    "188-maw-rs".to_owned(),
                    "189-crew-master".to_owned()
                ]
            }
        );

        let duplicate_prefix = BTreeSet::from(["188-alpha".to_owned(), "188-beta".to_owned()]);
        assert_eq!(
            resolve_tmux_attach_session("188", &duplicate_prefix),
            TmuxAttachSessionResolution::Ambiguous {
                query: "188".to_owned(),
                candidates: vec!["188-alpha".to_owned(), "188-beta".to_owned()]
            }
        );
    }

    #[test]
    fn tmux_attach_session_resolution_refuses_loose_ambiguity() {
        let alive = BTreeSet::from(["05-calliope".to_owned(), "06-caller".to_owned()]);
        assert_eq!(
            resolve_tmux_attach_session("call", &alive),
            TmuxAttachSessionResolution::Ambiguous {
                query: "call".to_owned(),
                candidates: vec!["05-calliope".to_owned(), "06-caller".to_owned()]
            }
        );
        assert_eq!(
            resolve_tmux_attach_session("ghost", &alive),
            TmuxAttachSessionResolution::Missing {
                session: "ghost".to_owned()
            }
        );
        assert_eq!(
            resolve_tmux_attach_session(" :0.1", &alive),
            TmuxAttachSessionResolution::Missing {
                session: String::new()
            }
        );
    }
