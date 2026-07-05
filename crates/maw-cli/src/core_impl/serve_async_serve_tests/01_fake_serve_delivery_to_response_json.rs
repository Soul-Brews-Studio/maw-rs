    use super::*;
    use axum::body::Body;
    use futures_util::{SinkExt, StreamExt};
    use maw_auth::{build_legacy_from_sign_payload, hash_body, sign_headers_v3_at, sign_hmac_sig};
    use tokio::sync::oneshot;
    use tower::ServiceExt;

    const KEY: &str = "test-peer-key-0123456789";
    const FROM: &str = "sender-oracle:sender-node";

    #[derive(Default)]
    struct FakeServeDelivery {
        sessions: Mutex<Vec<Vec<RouteSession>>>,
        sends: Mutex<Vec<(String, String)>>,
        captures: Mutex<HashMap<String, String>>,
        send_error: Mutex<Option<String>>,
        list_error: Mutex<Option<String>>,
    }

    impl FakeServeDelivery {
        fn with_capture_agent() -> Self {
            let fake = Self::default();
            fake.set_sessions(vec![vec![
                serve_test_session("capture-agent", 0, "capture-agent"),
                serve_test_session("remote-oracle", 0, "remote-oracle"),
            ]]);
            fake.set_capture("capture-agent:0", "[capture] delivered\n");
            fake.set_capture("remote-oracle:0", "[capture] delivered\n");
            fake
        }

        fn set_sessions(&self, sessions: Vec<Vec<RouteSession>>) {
            *self.sessions.lock().expect("sessions") = sessions;
        }

        fn set_capture(&self, target: &str, capture: &str) {
            self.captures
                .lock()
                .expect("captures")
                .insert(target.to_owned(), capture.to_owned());
        }

        fn sends(&self) -> Vec<(String, String)> {
            self.sends.lock().expect("sends").clone()
        }
    }

    impl ServeDelivery for FakeServeDelivery {
        fn route_sessions(&self) -> Result<Vec<RouteSession>, String> {
            if let Some(error) = self.list_error.lock().expect("list error").clone() {
                return Err(error);
            }
            let mut sessions = self.sessions.lock().expect("sessions");
            if sessions.len() > 1 {
                return Ok(sessions.remove(0));
            }
            Ok(sessions.first().cloned().unwrap_or_default())
        }

        fn send_literal_enter(&self, target: &str, text: &str) -> Result<(), String> {
            if let Some(error) = self.send_error.lock().expect("send error").clone() {
                return Err(error);
            }
            self.sends
                .lock()
                .expect("sends")
                .push((target.to_owned(), text.to_owned()));
            Ok(())
        }

        fn capture_tail(&self, target: &str, _lines: u32) -> Result<String, String> {
            Ok(self
                .captures
                .lock()
                .expect("captures")
                .get(target)
                .cloned()
                .unwrap_or_else(|| "[capture] delivered\n".to_owned()))
        }
    }

    fn serve_test_session(name: &str, index: u32, window: &str) -> RouteSession {
        RouteSession {
            name: name.to_owned(),
            source: None,
            windows: vec![RouteWindow {
                index,
                name: window.to_owned(),
                active: true,
            }],
        }
    }

    fn serve_test_delivery() -> Arc<dyn ServeDelivery> {
        Arc::new(FakeServeDelivery::with_capture_agent())
    }

    fn serve_test_receiver_inbox() -> Arc<dyn ServeReceiverInbox> {
        Arc::new(ServeSystemReceiverInbox {
            enabled: Some(false),
            fixed_now_millis: Some(1_782_277_200_000),
            psi_root: None,
        })
    }

    fn serve_test_receiver_inbox_at(repo: &std::path::Path, now_millis: u128) -> Arc<dyn ServeReceiverInbox> {
        Arc::new(ServeSystemReceiverInbox {
            enabled: Some(true),
            fixed_now_millis: Some(now_millis),
            psi_root: Some(repo.join("ψ")),
        })
    }

    fn serve_test_receiver_inbox_from_manifest(now_millis: u128) -> Arc<dyn ServeReceiverInbox> {
        Arc::new(ServeSystemReceiverInbox {
            enabled: Some(true),
            fixed_now_millis: Some(now_millis),
            psi_root: None,
        })
    }

    fn serve_test_peer_pubkey(from: &str, pubkey: &str) -> ServePeerPubkey {
        ServePeerPubkey {
            from: from.to_owned(),
            node: node_from_identity(from).expect("peer identity node"),
            pubkey: pubkey.to_owned(),
        }
    }

    fn serve_test_trust_store_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "maw-rs-trust-live-{label}-{}-{}.json",
            std::process::id(),
            random_hex(4)
        ))
    }

    fn serve_test_app(trust_store_path: std::path::PathBuf) -> Router {
        serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: Vec::new(),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path,
        })
    }

    fn signed_trust_request(method: &str, uri: &str, auth_path: &str, body: &'static str) -> axum::http::Request<Body> {
        let headers = sign_headers_v3_at(
            KEY,
            FROM,
            method,
            auth_path,
            Some(body.as_bytes()),
            1_782_277_200,
        )
        .expect("sign trust");
        let fleet_signature = sign_hmac_sig(KEY, &format!("{method}:{uri}:1782277200"));
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("x-maw-signature", fleet_signature);
        for (name, value) in headers.to_btree_map() {
            builder = builder.header(name, value);
        }
        let mut request = builder.body(Body::from(body)).expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }

    fn unsigned_trust_request(method: &str, uri: &str, body: &'static str) -> axum::http::Request<Body> {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json")
    }

