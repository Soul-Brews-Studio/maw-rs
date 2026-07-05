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

