
#[test]
fn transport_router_fixtures_match_maw_js_portable_spec() {
    let fixtures: Vec<Fixture> =
        serde_json::from_str(include_str!("../fixtures/transport-router.fixtures.json"))
            .expect("valid transport router fixture json");

    for fixture in fixtures {
        match fixture {
            Fixture::ClassifyError {
                name,
                error,
                expected,
            } => {
                assert_eq!(
                    classify_error(error.as_deref()),
                    expected_classified(&expected),
                    "{name}"
                );
            }
            Fixture::Send {
                name,
                target,
                message,
                from,
                transports,
                expected,
            } => {
                let sent = Rc::new(RefCell::new(Vec::new()));
                let mut router = TransportRouter::new();
                for transport in transports {
                    router.register(FixtureTransportRuntime {
                        fixture: transport,
                        sent: Rc::clone(&sent),
                    });
                }
                let target = target.map_or_else(
                    || TransportTarget {
                        oracle: "neo".to_owned(),
                        host: None,
                        tmux_target: Some("neo:1".to_owned()),
                    },
                    Into::into,
                );
                let result = router.send(
                    &target,
                    message.as_deref().unwrap_or("hello"),
                    from.as_deref().unwrap_or("codex"),
                );
                assert_eq!(result, expected_result(expected.result), "{name}");
                assert_eq!(*sent.borrow(), expected.sent, "sent order: {name}");
            }
        }
    }
}

struct UnresolvedThenHitIo;

impl HttpTransportIo for UnresolvedThenHitIo {
    fn list_local_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String> {
        Ok(Vec::new())
    }

    fn get_all_sessions(
        &mut self,
        _: &[TmuxTransportSession],
    ) -> Result<Vec<TransportSession>, String> {
        Ok(vec![
            remote_session("first", "http://first", 1),
            remote_session("second", "http://second", 2),
        ])
    }

    fn find_target_window(&mut self, sessions: &[TransportSession], _: &str) -> Option<String> {
        (sessions[0].name == "second").then(|| "second:2".to_owned())
    }

    fn send_peer_keys(&mut self, source: &str, target: &str, _: &str) -> Result<bool, String> {
        assert_eq!(source, "http://second");
        assert_eq!(target, "second:2");
        Ok(true)
    }

    fn post_peer_feed(
        &mut self,
        _: &str,
        _: &str,
        _: &str,
        _: u64,
    ) -> Result<HttpPostResult, String> {
        Ok(HttpPostResult {
            ok: true,
            status: 200,
        })
    }

    fn timeout_for(&self, _: &str) -> u64 {
        1
    }
}

fn remote_session(name: &str, source: &str, index: u32) -> TransportSession {
    TransportSession {
        name: name.to_owned(),
        source: Some(source.to_owned()),
        windows: vec![TmuxTransportWindow {
            index,
            name: "mawjs".to_owned(),
            active: false,
        }],
    }
}

#[test]
fn http_transport_continues_after_unresolved_remote_window() {
    let config = HttpTransportConfig {
        peers: vec!["http://peer".to_owned()],
        self_host: String::new(),
    };
    let mut transport = HttpFederationTransport::new(config, UnresolvedThenHitIo);

    assert!(transport.send(
        &TransportTarget {
            oracle: "mawjs".to_owned(),
            host: Some("remote".to_owned()),
            tmux_target: None,
        },
        "hello"
    ));
}

#[test]
fn remaining_transport_edges_cover_unknown_and_local_failure_paths() {
    assert_eq!(
        classify_error(Some("opaque failure")),
        ClassifiedError {
            reason: TransportFailureReason::Unknown,
            retryable: false,
        }
    );

    let local_target = TransportTarget {
        oracle: "mawjs".to_owned(),
        host: Some("localhost".to_owned()),
        tmux_target: None,
    };
    let remote_target = TransportTarget {
        oracle: "mawjs".to_owned(),
        host: Some("remote".to_owned()),
        tmux_target: Some("s:1".to_owned()),
    };

    let mut list_error = maw_transport::TmuxLocalTransport::new(LocalIo {
        list_error: true,
        ..LocalIo::default()
    });
    assert!(!list_error.send(&local_target, "hello"));

    let mut unresolved = maw_transport::TmuxLocalTransport::new(LocalIo::default());
    assert!(!unresolved.send(&local_target, "hello"));

    let mut send_error = maw_transport::TmuxLocalTransport::new(LocalIo {
        resolve: Some("local:1".to_owned()),
        send_ok: false,
        ..LocalIo::default()
    });
    assert!(!send_error.send(&local_target, "hello"));
    assert!(!send_error.send(&remote_target, "hello"));

    let mut explicit = maw_transport::TmuxLocalTransport::new(LocalIo {
        send_ok: true,
        ..LocalIo::default()
    });
    assert!(explicit.send(
        &TransportTarget {
            tmux_target: Some("local:1".to_owned()),
            ..local_target
        },
        "hello"
    ));
}
