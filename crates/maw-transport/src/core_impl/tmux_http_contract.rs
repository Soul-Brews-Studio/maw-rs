#[cfg(test)]
mod tmux_transport_tests {
    use super::*;

    #[derive(Default)]
    struct FakeTmuxIo {
        sends: Vec<(String, String)>,
        scanned: bool,
        sessions: Vec<TmuxTransportSession>,
        queries: Vec<String>,
        find_result: Option<String>,
        send_error: bool,
    }

    impl TmuxTransportIo for FakeTmuxIo {
        fn send_to_tmux(&mut self, target: &str, message: &str) -> Result<(), String> {
            if self.send_error {
                return Err("tmux rejected".to_owned());
            }
            self.sends.push((target.to_owned(), message.to_owned()));
            Ok(())
        }

        fn list_tmux_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String> {
            self.scanned = true;
            Ok(self.sessions.clone())
        }

        fn find_tmux_window(
            &mut self,
            sessions: &[TmuxTransportSession],
            query: &str,
        ) -> Option<String> {
            assert_eq!(sessions, self.sessions.as_slice());
            self.queries.push(query.to_owned());
            self.find_result.clone()
        }
    }

    fn sample_sessions() -> Vec<TmuxTransportSession> {
        vec![TmuxTransportSession {
            name: "47-mawjs".to_owned(),
            windows: vec![
                TmuxTransportWindow {
                    index: 0,
                    name: "mawjs-oracle".to_owned(),
                    active: true,
                },
                TmuxTransportWindow {
                    index: 1,
                    name: "mawjs-codex".to_owned(),
                    active: false,
                },
            ],
        }]
    }

    #[test]
    fn tmux_transport_tracks_local_lifecycle_and_reachability() {
        let mut transport = TmuxLocalTransport::new(FakeTmuxIo::default());
        assert_eq!(transport.name(), "tmux");
        assert!(!transport.connected());
        transport.connect();
        assert!(transport.connected());
        transport.disconnect();
        assert!(!transport.connected());

        assert!(transport.can_reach(&TransportTarget {
            oracle: "mawjs".to_owned(),
            host: None,
            tmux_target: None,
        }));
        assert!(transport.can_reach(&TransportTarget {
            oracle: "mawjs".to_owned(),
            host: Some("local".to_owned()),
            tmux_target: None,
        }));
        assert!(transport.can_reach(&TransportTarget {
            oracle: "mawjs".to_owned(),
            host: Some("localhost".to_owned()),
            tmux_target: None,
        }));
        assert!(!transport.can_reach(&TransportTarget {
            oracle: "mawjs".to_owned(),
            host: Some("m5".to_owned()),
            tmux_target: None,
        }));
    }

    #[test]
    fn tmux_transport_uses_explicit_target_without_scanning() {
        let mut transport = TmuxLocalTransport::new(FakeTmuxIo::default());
        assert!(transport.send(
            &TransportTarget {
                oracle: "ignored".to_owned(),
                host: None,
                tmux_target: Some("47-mawjs:1".to_owned()),
            },
            "hello",
        ));
        assert!(!transport.io().scanned);
        assert_eq!(
            transport.io().sends,
            vec![("47-mawjs:1".to_owned(), "hello".to_owned())]
        );
    }

    #[test]
    fn tmux_transport_resolves_local_oracle_through_session_scan() {
        let io = FakeTmuxIo {
            sessions: sample_sessions(),
            find_result: Some("47-mawjs:1".to_owned()),
            ..FakeTmuxIo::default()
        };
        let mut transport = TmuxLocalTransport::new(io);
        assert!(transport.send(
            &TransportTarget {
                oracle: "mawjs-codex".to_owned(),
                host: None,
                tmux_target: None,
            },
            "ping",
        ));
        assert!(transport.io().scanned);
        assert_eq!(transport.io().queries, vec!["mawjs-codex".to_owned()]);
        assert_eq!(
            transport.io().sends,
            vec![("47-mawjs:1".to_owned(), "ping".to_owned())]
        );
    }

    #[test]
    fn tmux_transport_returns_false_for_remote_unresolved_and_throwing_paths() {
        let mut remote = TmuxLocalTransport::new(FakeTmuxIo {
            sessions: sample_sessions(),
            ..FakeTmuxIo::default()
        });
        assert!(!remote.send(
            &TransportTarget {
                oracle: "mawjs".to_owned(),
                host: Some("remote".to_owned()),
                tmux_target: None,
            },
            "nope",
        ));
        assert!(remote.io().sends.is_empty());

        let mut unresolved = TmuxLocalTransport::new(FakeTmuxIo {
            sessions: sample_sessions(),
            find_result: None,
            ..FakeTmuxIo::default()
        });
        assert!(!unresolved.send(
            &TransportTarget {
                oracle: "missing".to_owned(),
                host: None,
                tmux_target: None,
            },
            "nope",
        ));

        let mut throwing = TmuxLocalTransport::new(FakeTmuxIo {
            sessions: sample_sessions(),
            find_result: Some("47-mawjs:1".to_owned()),
            send_error: true,
            ..FakeTmuxIo::default()
        });
        assert!(!throwing.send(
            &TransportTarget {
                oracle: "mawjs".to_owned(),
                host: None,
                tmux_target: None,
            },
            "nope",
        ));
        assert!(throwing.io().sends.is_empty());
    }

    #[test]
    fn tmux_transport_accepts_handlers_and_ignores_publish_hooks() {
        let mut transport = TmuxLocalTransport::new(FakeTmuxIo::default());
        transport.on_message();
        transport.on_presence();
        transport.on_feed();
        assert_eq!(transport.handler_counts(), (1, 1, 1));
        transport.publish_presence();
        transport.publish_feed();
    }
}

#[cfg(test)]
mod http_transport_tests {
    use super::*;

    #[derive(Default)]
    struct FakeHttpIo {
        local_sessions: Vec<TmuxTransportSession>,
        all_sessions: Vec<TransportSession>,
        sent: Vec<(String, String, String)>,
        posts: Vec<(String, String, String, u64)>,
        queries: Vec<String>,
        find_result: Option<String>,
        fail_post_url: Option<String>,
        peer_send_result: Option<Result<bool, String>>,
    }

    impl HttpTransportIo for FakeHttpIo {
        fn list_local_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String> {
            Ok(self.local_sessions.clone())
        }

        fn get_all_sessions(
            &mut self,
            local_sessions: &[TmuxTransportSession],
        ) -> Result<Vec<TransportSession>, String> {
            assert_eq!(local_sessions, self.local_sessions.as_slice());
            Ok(self.all_sessions.clone())
        }

        fn find_target_window(
            &mut self,
            sessions: &[TransportSession],
            query: &str,
        ) -> Option<String> {
            assert_eq!(sessions.len(), 1);
            self.queries.push(query.to_owned());
            self.find_result.clone()
        }

        fn send_peer_keys(
            &mut self,
            source: &str,
            target: &str,
            message: &str,
        ) -> Result<bool, String> {
            self.sent
                .push((source.to_owned(), target.to_owned(), message.to_owned()));
            self.peer_send_result
                .clone()
                .unwrap_or(Ok(true))
        }

        fn post_peer_feed(
            &mut self,
            url: &str,
            method: &str,
            body: &str,
            timeout_ms: u64,
        ) -> Result<HttpPostResult, String> {
            self.posts.push((
                url.to_owned(),
                method.to_owned(),
                body.to_owned(),
                timeout_ms,
            ));
            if self.fail_post_url.as_deref() == Some(url) {
                Err("boom".to_owned())
            } else {
                Ok(HttpPostResult {
                    ok: true,
                    status: 200,
                })
            }
        }

        fn timeout_for(&self, transport: &str) -> u64 {
            assert_eq!(transport, "http");
            1234
        }
    }

    fn window(name: &str) -> TmuxTransportWindow {
        TmuxTransportWindow {
            index: 0,
            name: name.to_owned(),
            active: true,
        }
    }

    fn local_session(name: &str, window_name: &str) -> TmuxTransportSession {
        TmuxTransportSession {
            name: name.to_owned(),
            windows: vec![window(window_name)],
        }
    }

    fn sourced_session(name: &str, window_name: &str, source: Option<&str>) -> TransportSession {
        TransportSession {
            name: name.to_owned(),
            source: source.map(str::to_owned),
            windows: vec![window(window_name)],
        }
    }

    #[test]
    fn http_transport_connects_only_when_peers_are_configured() {
        let mut offline = HttpFederationTransport::new(
            HttpTransportConfig {
                peers: Vec::new(),
                self_host: "local".to_owned(),
            },
            FakeHttpIo::default(),
        );
        assert_eq!(offline.name(), "http-federation");
        assert!(!offline.connected());
        offline.connect();
        assert!(!offline.connected());

        let mut online = HttpFederationTransport::new(
            HttpTransportConfig {
                peers: vec!["http://peer".to_owned()],
                self_host: "local".to_owned(),
            },
            FakeHttpIo::default(),
        );
        online.connect();
        assert!(online.connected());
        online.disconnect();
        assert!(!online.connected());
    }

    #[test]
    fn http_transport_can_reach_only_remote_targets_when_peers_exist() {
        let no_peers = HttpFederationTransport::new(
            HttpTransportConfig {
                peers: Vec::new(),
                self_host: "local".to_owned(),
            },
            FakeHttpIo::default(),
        );
        assert!(!no_peers.can_reach(&TransportTarget {
            oracle: "mawjs".to_owned(),
            host: Some("m5".to_owned()),
            tmux_target: None,
        }));

        let transport = HttpFederationTransport::new(
            HttpTransportConfig {
                peers: vec!["http://peer".to_owned()],
                self_host: "local".to_owned(),
            },
            FakeHttpIo::default(),
        );
        for host in [None, Some("local"), Some("localhost")] {
            assert!(!transport.can_reach(&TransportTarget {
                oracle: "mawjs".to_owned(),
                host: host.map(str::to_owned),
                tmux_target: None,
            }));
        }
        assert!(transport.can_reach(&TransportTarget {
            oracle: "mawjs".to_owned(),
            host: Some("m5".to_owned()),
            tmux_target: None,
        }));
    }

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
}
