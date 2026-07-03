#[cfg(test)]
mod coverage_gap_tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    struct FailingTmuxListIo;

    impl TmuxTransportIo for FailingTmuxListIo {
        fn send_to_tmux(&mut self, _target: &str, _message: &str) -> Result<(), String> {
            Ok(())
        }

        fn list_tmux_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String> {
            Err("tmux list failed".to_owned())
        }

        fn find_tmux_window(
            &mut self,
            _sessions: &[TmuxTransportSession],
            _query: &str,
        ) -> Option<String> {
            Some("ignored:0".to_owned())
        }
    }

    #[derive(Default)]
    struct FailingHttpIo {
        fail_all_sessions: bool,
    }

    impl HttpTransportIo for FailingHttpIo {
        fn list_local_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String> {
            if self.fail_all_sessions {
                Ok(Vec::new())
            } else {
                Err("local session list failed".to_owned())
            }
        }

        fn get_all_sessions(
            &mut self,
            _local_sessions: &[TmuxTransportSession],
        ) -> Result<Vec<TransportSession>, String> {
            Err("aggregate failed".to_owned())
        }

        fn find_target_window(
            &mut self,
            _sessions: &[TransportSession],
            _query: &str,
        ) -> Option<String> {
            Some("ignored:0".to_owned())
        }

        fn send_peer_keys(
            &mut self,
            _source: &str,
            _target: &str,
            _message: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        fn post_peer_feed(
            &mut self,
            _url: &str,
            _method: &str,
            _body: &str,
            _timeout_ms: u64,
        ) -> Result<HttpPostResult, String> {
            Ok(HttpPostResult {
                ok: true,
                status: 200,
            })
        }

        fn timeout_for(&self, _transport: &str) -> u64 {
            1
        }
    }

    fn target(oracle: &str) -> TransportTarget {
        TransportTarget {
            oracle: oracle.to_owned(),
            host: Some("remote".to_owned()),
            tmux_target: None,
        }
    }

    #[test]
    fn fake_ios_exercise_all_required_trait_methods() {
        let mut tmux = FailingTmuxListIo;
        assert!(tmux.send_to_tmux("target", "message").is_ok());
        assert_eq!(
            tmux.find_tmux_window(&[], "mawjs"),
            Some("ignored:0".to_owned())
        );

        let mut http = FailingHttpIo::default();
        assert_eq!(
            http.find_target_window(&[], "mawjs"),
            Some("ignored:0".to_owned())
        );
        assert_eq!(http.send_peer_keys("source", "target", "message"), Ok(true));
        assert_eq!(
            http.post_peer_feed("http://peer/api/feed", "POST", "{}", 1),
            Ok(HttpPostResult {
                ok: true,
                status: 200,
            })
        );
        assert_eq!(http.timeout_for("http"), 1);
    }

    #[test]
    fn failure_reason_and_pair_health_labels_are_stable() {
        assert_eq!(TransportFailureReason::Timeout.as_str(), "timeout");
        assert_eq!(TransportFailureReason::Unreachable.as_str(), "unreachable");
        assert_eq!(TransportFailureReason::Auth.as_str(), "auth");
        assert_eq!(TransportFailureReason::RateLimit.as_str(), "rate_limit");
        assert_eq!(TransportFailureReason::Rejected.as_str(), "rejected");
        assert_eq!(TransportFailureReason::ParseError.as_str(), "parse_error");
        assert_eq!(TransportFailureReason::Unknown.as_str(), "unknown");
        assert_eq!(PairHealth::Unknown.as_str(), "unknown");
    }

    #[test]
    fn unknown_error_strings_remain_non_retryable_unknowns() {
        assert_eq!(
            classify_error(Some("socket evaporated mysteriously")),
            ClassifiedError {
                reason: TransportFailureReason::Unknown,
                retryable: false,
            }
        );
    }

    #[test]
    fn tmux_local_host_defaults_and_unknown_result_constructors_are_stable() {
        assert!(is_local_host(None));
        assert!(is_local_host(Some("local")));
        assert!(!is_local_host(Some("remote")));
        assert_eq!(
            TransportResult::failure("none", TransportFailureReason::Unknown, false),
            TransportResult {
                ok: false,
                via: "none".to_owned(),
                reason: Some(TransportFailureReason::Unknown),
                retryable: false,
            }
        );
    }

    #[test]
    fn classifier_recognizes_alternate_needles_and_rate_limit_shapes() {
        assert_eq!(
            classify_error(None),
            ClassifiedError {
                reason: TransportFailureReason::Unknown,
                retryable: false,
            }
        );
        for (message, reason, retryable) in [
            ("ENETUNREACH while dialing", TransportFailureReason::Unreachable, true),
            ("too many requests", TransportFailureReason::RateLimit, true),
            ("rate window limit exceeded", TransportFailureReason::RateLimit, true),
            ("403 forbidden", TransportFailureReason::Auth, false),
            ("permission denied", TransportFailureReason::Rejected, false),
            ("json syntax error", TransportFailureReason::ParseError, false),
            ("socket evaporated mysteriously", TransportFailureReason::Unknown, false),
        ] {
            assert_eq!(
                classify_error(Some(message)),
                ClassifiedError { reason, retryable },
                "{message}"
            );
        }
    }

    struct ScriptedTransport {
        name: &'static str,
        connected: bool,
        reachable: bool,
        result: Result<bool, &'static str>,
        sent: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Transport for ScriptedTransport {
        fn name(&self) -> &str {
            self.name
        }

        fn connected(&self) -> bool {
            self.connected
        }

        fn can_reach(&self, _target: &TransportTarget) -> bool {
            self.reachable
        }

        fn send(
            &mut self,
            _target: &TransportTarget,
            _message: &str,
            _from: &str,
        ) -> Result<bool, String> {
            self.sent.borrow_mut().push(self.name);
            self.result.map_err(str::to_owned)
        }
    }

    fn scripted(
        name: &'static str,
        connected: bool,
        reachable: bool,
        result: Result<bool, &'static str>,
        sent: &Rc<RefCell<Vec<&'static str>>>,
    ) -> ScriptedTransport {
        ScriptedTransport {
            name,
            connected,
            reachable,
            result,
            sent: Rc::clone(sent),
        }
    }

    #[test]
    fn router_skips_unavailable_transports_and_fails_over_after_retryable_errors() {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let mut router = TransportRouter::new();
        router.register(scripted("offline", false, true, Ok(true), &sent));
        router.register(scripted("unreachable", true, false, Ok(true), &sent));
        router.register(scripted("soft-false", true, true, Ok(false), &sent));
        router.register(scripted("retryable", true, true, Err("timeout"), &sent));
        router.register(scripted("winner", true, true, Ok(true), &sent));

        let result = router.send(&target("mawjs"), "hello", "codex");

        assert_eq!(result, TransportResult::success("winner"));
        assert_eq!(
            *sent.borrow(),
            vec!["soft-false", "retryable", "winner"]
        );
    }

    struct RemoteSessionIo;

    impl HttpTransportIo for RemoteSessionIo {
        fn list_local_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String> {
            Ok(vec![TmuxTransportSession {
                name: "local".to_owned(),
                windows: Vec::new(),
            }])
        }

        fn get_all_sessions(
            &mut self,
            _local_sessions: &[TmuxTransportSession],
        ) -> Result<Vec<TransportSession>, String> {
            Ok(vec![
                TransportSession {
                    name: "without-source".to_owned(),
                    source: None,
                    windows: vec![TmuxTransportWindow {
                        index: 0,
                        name: "mawjs".to_owned(),
                        active: true,
                    }],
                },
                TransportSession {
                    name: "local-source".to_owned(),
                    source: Some("local".to_owned()),
                    windows: vec![TmuxTransportWindow {
                        index: 1,
                        name: "mawjs".to_owned(),
                        active: false,
                    }],
                },
                TransportSession {
                    name: "remote-miss".to_owned(),
                    source: Some("http://miss".to_owned()),
                    windows: vec![TmuxTransportWindow {
                        index: 2,
                        name: "other".to_owned(),
                        active: false,
                    }],
                },
                TransportSession {
                    name: "remote-hit".to_owned(),
                    source: Some("http://hit".to_owned()),
                    windows: vec![TmuxTransportWindow {
                        index: 3,
                        name: "MAWJS oracle".to_owned(),
                        active: false,
                    }],
                },
            ])
        }

        fn find_target_window(
            &mut self,
            sessions: &[TransportSession],
            query: &str,
        ) -> Option<String> {
            assert_eq!(sessions.len(), 1);
            assert_eq!(query, "mawjs");
            Some(format!("{}:3", sessions[0].name))
        }

        fn send_peer_keys(
            &mut self,
            source: &str,
            target: &str,
            message: &str,
        ) -> Result<bool, String> {
            assert_eq!(source, "http://hit");
            assert_eq!(target, "remote-hit:3");
            assert_eq!(message, "hello");
            Ok(true)
        }

        fn post_peer_feed(
            &mut self,
            _url: &str,
            _method: &str,
            _body: &str,
            _timeout_ms: u64,
        ) -> Result<HttpPostResult, String> {
            Ok(HttpPostResult {
                ok: true,
                status: 200,
            })
        }

        fn timeout_for(&self, _transport: &str) -> u64 {
            250
        }
    }

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

    #[test]
    fn http_transport_empty_peers_and_feed_warning_edges_are_stable() {
        struct WarningIo;
        impl HttpTransportIo for WarningIo {
            fn list_local_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String> {
                Ok(Vec::new())
            }

            fn get_all_sessions(
                &mut self,
                _: &[TmuxTransportSession],
            ) -> Result<Vec<TransportSession>, String> {
                Ok(vec![TransportSession {
                    name: "remote".to_owned(),
                    source: Some("http://peer".to_owned()),
                    windows: vec![TmuxTransportWindow {
                        index: 1,
                        name: "mawjs".to_owned(),
                        active: false,
                    }],
                }])
            }

            fn find_target_window(&mut self, _: &[TransportSession], _: &str) -> Option<String> {
                Some("mawjs:1".to_owned())
            }

            fn send_peer_keys(&mut self, _: &str, _: &str, _: &str) -> Result<bool, String> {
                Ok(false)
            }

            fn post_peer_feed(
                &mut self,
                url: &str,
                _: &str,
                _: &str,
                _: u64,
            ) -> Result<HttpPostResult, String> {
                Err(format!("reject {url}"))
            }

            fn timeout_for(&self, _: &str) -> u64 {
                99
            }
        }
        let mut empty = HttpFederationTransport::new(HttpTransportConfig::default(), WarningIo);
        empty.connect();
        assert!(!empty.connected());
        assert!(!empty.can_reach(&target("mawjs")));

        let config = HttpTransportConfig {
            peers: vec!["http://peer".to_owned()],
            self_host: String::new(),
        };
        let mut transport = HttpFederationTransport::new(config.clone(), WarningIo);
        transport.connect();
        assert!(!transport.send(&target("mawjs"), "hello"));

        let mut feed_transport = HttpFederationTransport::new(config, WarningIo);
        assert_eq!(
            feed_transport.publish_feed("{}")[0].reason,
            "reject http://peer/api/feed"
        );
    }

}
