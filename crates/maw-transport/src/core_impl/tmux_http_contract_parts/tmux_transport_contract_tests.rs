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
