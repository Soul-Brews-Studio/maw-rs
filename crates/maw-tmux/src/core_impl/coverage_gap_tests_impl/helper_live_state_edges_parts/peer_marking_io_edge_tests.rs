
    #[test]
    fn live_state_match_labels_use_oracle_and_empty_cwd_is_ignored() {
        let peers = vec![maw_peer::PeerTarget {
            name: None,
            url: "http://scratch".to_owned(),
            source: maw_peer::PeerSourceKind::Scout,
            node: None,
            oracle: Some("scratch".to_owned()),
        }];
        let result = resolve_tmux_live_state(
            &peers,
            &[TmuxPane {
                id: "%11".to_owned(),
                command: "zsh".to_owned(),
                target: "demo:1.0".to_owned(),
                title: "scratch".to_owned(),
                pid: None,
                cwd: Some("////".to_owned()),
                last_activity: None,
            }],
        );

        assert_eq!(result.live[0].matches, vec!["scratch"]);
        assert_eq!(path_basename("////"), None);
    }

    #[test]
    fn mark_peer_targets_live_reports_targets_sessions_and_cwd_matches() {
        let peers = vec![maw_peer::PeerTarget {
            name: Some("scratch".to_owned()),
            url: "http://scratch".to_owned(),
            source: maw_peer::PeerSourceKind::Config,
            node: Some("node-a".to_owned()),
            oracle: Some("oracle-a".to_owned()),
        }];
        let live = vec![
            DiscoverLivePane {
                source: "tmux".to_owned(),
                id: "%1".to_owned(),
                target: "05-scratch:1.0".to_owned(),
                session: "05-scratch".to_owned(),
                window: "main".to_owned(),
                pane: "0".to_owned(),
                command: Some("zsh".to_owned()),
                title: None,
                pid: None,
                cwd: Some("/tmp/scratch".to_owned()),
                last_activity: None,
                awake: true,
                matches: Vec::new(),
            },
            DiscoverLivePane {
                source: "tmux".to_owned(),
                id: "%2".to_owned(),
                target: "other:1.0".to_owned(),
                session: "other".to_owned(),
                window: "scratch".to_owned(),
                pane: "0".to_owned(),
                command: Some("zsh".to_owned()),
                title: None,
                pid: None,
                cwd: None,
                last_activity: None,
                awake: true,
                matches: Vec::new(),
            },
        ];

        let marked = mark_peer_targets_live(&peers, &live);

        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].name, Some("scratch".to_owned()));
        assert_eq!(marked[0].url, "http://scratch");
        assert_eq!(marked[0].source, maw_peer::PeerSourceKind::Config);
        assert_eq!(marked[0].node, Some("node-a".to_owned()));
        assert_eq!(marked[0].oracle, Some("oracle-a".to_owned()));
        assert!(marked[0].awake);
        assert_eq!(
            marked[0].live_targets,
            vec!["05-scratch:1.0".to_owned(), "other:1.0".to_owned()]
        );
        assert_eq!(
            marked[0].live_sessions,
            vec!["05-scratch".to_owned(), "other".to_owned()]
        );
    }

    #[test]
    fn io_error_formatter_includes_action_program_and_error() {
        let error = tmux_program_io_error(
            "collect output from",
            std::ffi::OsStr::new("tmux"),
            &std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe closed"),
        );

        assert!(error.message.contains("failed to collect output from tmux"));
        assert!(error.message.contains("pipe closed"));
    }

    #[test]
    fn command_runner_writes_stdin_and_collects_stdout() {
        let mut runner = CommandTmuxRunner::with_program("/bin/cat");

        let output = runner
            .run_with_stdin("-", &[], b"hello from stdin")
            .expect("cat should echo stdin");

        assert_eq!(output, "hello from stdin");
    }
