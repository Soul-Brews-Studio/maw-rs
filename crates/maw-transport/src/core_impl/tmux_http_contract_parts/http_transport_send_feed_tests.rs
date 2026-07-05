
    #[test]
    fn http_transport_sends_through_peer_that_owns_matching_window() {
        let local_sessions = vec![local_session("local", "local-oracle")];
        let all_sessions = vec![
            sourced_session("local", "local-oracle", Some("local")),
            sourced_session("remote-a", "other-oracle", Some("http://peer-a")),
            sourced_session("remote-b", "target-oracle", Some("http://peer-b")),
        ];
        let io = FakeHttpIo {
            local_sessions,
            all_sessions,
            find_result: Some("remote-b:0".to_owned()),
            ..FakeHttpIo::default()
        };
        let mut transport = HttpFederationTransport::new(
            HttpTransportConfig {
                peers: vec!["http://peer-a".to_owned(), "http://peer-b".to_owned()],
                self_host: "local".to_owned(),
            },
            io,
        );
        assert!(transport.send(
            &TransportTarget {
                oracle: "target".to_owned(),
                host: Some("remote".to_owned()),
                tmux_target: None,
            },
            "hello",
        ));
        assert_eq!(transport.io().queries, vec!["target".to_owned()]);
        assert_eq!(
            transport.io().sent,
            vec![(
                "http://peer-b".to_owned(),
                "remote-b:0".to_owned(),
                "hello".to_owned(),
            ),]
        );
    }

    #[test]
    fn http_transport_returns_false_when_no_remote_session_resolves() {
        let io = FakeHttpIo {
            all_sessions: vec![
                sourced_session("local", "target-oracle", None),
                sourced_session("remote-a", "other-oracle", Some("http://peer-a")),
                sourced_session("remote-b", "target-oracle", Some("http://peer-b")),
            ],
            find_result: None,
            ..FakeHttpIo::default()
        };
        let mut transport = HttpFederationTransport::new(
            HttpTransportConfig {
                peers: vec!["http://peer".to_owned()],
                self_host: "local".to_owned(),
            },
            io,
        );
        assert!(!transport.send(
            &TransportTarget {
                oracle: "target".to_owned(),
                host: Some("remote".to_owned()),
                tmux_target: None,
            },
            "hello",
        ));
        assert!(transport.io().sent.is_empty());
    }

    #[test]
    fn http_transport_returns_false_when_peer_send_declines_or_errors() {
        for peer_send_result in [Ok(false), Err("peer refused".to_owned())] {
            let io = FakeHttpIo {
                all_sessions: vec![sourced_session(
                    "remote",
                    "target-oracle",
                    Some("http://peer"),
                )],
                find_result: Some("remote:0".to_owned()),
                peer_send_result: Some(peer_send_result),
                ..FakeHttpIo::default()
            };
            let mut transport = HttpFederationTransport::new(
                HttpTransportConfig {
                    peers: vec!["http://peer".to_owned()],
                    self_host: "local".to_owned(),
                },
                io,
            );

            assert!(!transport.send(
                &TransportTarget {
                    oracle: "target".to_owned(),
                    host: Some("remote".to_owned()),
                    tmux_target: None,
                },
                "hello",
            ));
            assert_eq!(transport.io().sent.len(), 1);
        }
    }

    #[test]
    fn http_transport_publishes_feed_events_to_every_peer_and_warns_on_rejections() {
        let mut transport = HttpFederationTransport::new(
            HttpTransportConfig {
                peers: vec![
                    "http://a".to_owned(),
                    "http://b".to_owned(),
                    "http://c".to_owned(),
                ],
                self_host: "local".to_owned(),
            },
            FakeHttpIo {
                fail_post_url: Some("http://b/api/feed".to_owned()),
                ..FakeHttpIo::default()
            },
        );
        let warnings = transport.publish_feed("{\"message\":\"hello\"}");
        assert_eq!(
            transport.io().posts,
            vec![
                (
                    "http://a/api/feed".to_owned(),
                    "POST".to_owned(),
                    "{\"message\":\"hello\"}".to_owned(),
                    1234,
                ),
                (
                    "http://b/api/feed".to_owned(),
                    "POST".to_owned(),
                    "{\"message\":\"hello\"}".to_owned(),
                    1234,
                ),
                (
                    "http://c/api/feed".to_owned(),
                    "POST".to_owned(),
                    "{\"message\":\"hello\"}".to_owned(),
                    1234,
                ),
            ]
        );
        assert_eq!(
            warnings,
            vec![HttpFeedWarning {
                peer: "http://b".to_owned(),
                reason: "boom".to_owned(),
            }]
        );
    }

    #[test]
    fn http_transport_accepts_handlers_and_ignores_presence() {
        let mut transport = HttpFederationTransport::new(
            HttpTransportConfig {
                peers: Vec::new(),
                self_host: "local".to_owned(),
            },
            FakeHttpIo::default(),
        );
        transport.on_message();
        transport.on_presence();
        transport.on_feed();
        assert_eq!(transport.handler_counts(), (1, 1, 1));
        transport.publish_presence();
    }
