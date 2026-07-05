
    #[test]
    fn http_transport_lifecycle_and_remote_session_scan_edges_are_deterministic() {
        let config = HttpTransportConfig {
            peers: vec!["http://peer".to_owned()],
            self_host: "local".to_owned(),
        };
        let mut transport = HttpFederationTransport::new(config, RemoteSessionIo);

        assert!(!transport.connected());
        transport.connect();
        assert!(transport.connected());
        assert_eq!(transport.name(), "http-federation");
        assert!(transport.can_reach(&target("mawjs")));
        assert!(!transport.can_reach(&TransportTarget {
            oracle: "mawjs".to_owned(),
            host: Some("localhost".to_owned()),
            tmux_target: None,
        }));
        transport.on_message();
        transport.on_presence();
        transport.on_feed();
        assert_eq!(transport.handler_counts(), (1, 1, 1));
        transport.publish_presence();
        assert_eq!(transport.io().timeout_for("http"), 250);
        assert!(transport.publish_feed("{}").is_empty());

        assert!(transport.send(&target("mawjs"), "hello"));
        transport.disconnect();
        assert!(!transport.connected());
    }

    #[test]
    fn transport_result_constructors_accept_owned_via_values() {
        assert_eq!(
            TransportResult::success("tmux".to_owned()),
            TransportResult {
                ok: true,
                via: "tmux".to_owned(),
                reason: None,
                retryable: false,
            }
        );
        assert_eq!(
            TransportResult::failure(
                "http-federation".to_owned(),
                TransportFailureReason::Rejected,
                false
            ),
            TransportResult {
                ok: false,
                via: "http-federation".to_owned(),
                reason: Some(TransportFailureReason::Rejected),
                retryable: false,
            }
        );
    }

    #[test]
    fn tmux_session_conversion_preserves_windows_with_no_source() {
        let local = TmuxTransportSession {
            name: "mawjs".to_owned(),
            windows: vec![TmuxTransportWindow {
                index: 2,
                name: "oracle".to_owned(),
                active: false,
            }],
        };

        let session = TransportSession::from(local.clone());

        assert_eq!(session.name, local.name);
        assert_eq!(session.source, None);
        assert_eq!(session.windows, local.windows);
    }

    #[test]
    fn tmux_transport_returns_false_when_session_listing_fails() {
        let mut transport = TmuxLocalTransport::new(FailingTmuxListIo);

        assert!(!transport.send(
            &TransportTarget {
                oracle: "mawjs".to_owned(),
                host: None,
                tmux_target: None,
            },
            "hello",
        ));
    }

    #[test]
    fn http_transport_returns_false_when_session_collection_fails() {
        let config = HttpTransportConfig {
            peers: vec!["http://peer".to_owned()],
            self_host: "local".to_owned(),
        };
        let mut list_fails = HttpFederationTransport::new(config.clone(), FailingHttpIo::default());
        assert!(!list_fails.send(&target("mawjs"), "hello"));

        let mut aggregate_fails = HttpFederationTransport::new(
            config,
            FailingHttpIo {
                fail_all_sessions: true,
            },
        );
        assert!(!aggregate_fails.send(&target("mawjs"), "hello"));
    }

    #[test]
    fn missing_remote_status_is_unknown_with_zero_status_reason() {
        let status = classify_symmetric_federation_status(
            &FederationStatus {
                local_url: "http://local:3456".to_owned(),
                peers: vec![FederationPeerStatus {
                    url: "http://peer:3456".to_owned(),
                    node: Some("peer".to_owned()),
                    reachable: true,
                    latency: None,
                    agents: vec!["mawjs".to_owned()],
                    clock_warning: true,
                }],
            },
            &[],
            "local",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Unknown);
        assert_eq!(
            status.pairs[0].reason.as_deref(),
            Some("peer /api/federation/status returned 0")
        );
        assert_eq!(status.pairs[0].agents, ["mawjs"]);
        assert!(status.pairs[0].clock_warning);
    }

    #[test]
    fn tmux_transport_explicit_target_error_and_feed_noop_are_stable() {
        let mut transport = TmuxLocalTransport::new(FailingTmuxListIo);
        transport.publish_feed();
        assert!(!transport.send(
            &TransportTarget {
                oracle: "mawjs".to_owned(),
                host: Some("remote".to_owned()),
                tmux_target: Some("ignored:0".to_owned()),
            },
            "hello",
        ));
    }
