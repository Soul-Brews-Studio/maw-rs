
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

