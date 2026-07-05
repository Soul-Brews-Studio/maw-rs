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
