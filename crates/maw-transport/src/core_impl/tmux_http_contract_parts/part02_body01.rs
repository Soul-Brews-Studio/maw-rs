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
