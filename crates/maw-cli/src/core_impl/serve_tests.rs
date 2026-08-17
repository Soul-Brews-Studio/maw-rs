// The serve suite, kept whole.
//
// These 57 tests share one fixture harness -- a fake delivery sink, a fake wake
// executor, signed/unsigned request builders and several app builders. Splitting
// them by topic would mean either duplicating that harness per file or exporting
// ~44 helpers across sibling modules, since a private item in one `mod` is not
// visible to another. One cohesive test module is the better shape; it lives here
// so serve.rs itself reads as the server, not the server plus its scaffolding.

#[cfg(test)]
#[allow(clippy::redundant_closure_for_method_calls)]
mod serve_tests {
    use super::*;

    fn non_agent_pane(target: &str) -> TmuxPane {
        TmuxPane {
            id: "%1".to_owned(),
            command: "bash".to_owned(),
            target: target.to_owned(),
            title: "nat-LOCAL (shell)".to_owned(),
            pid: Some(100),
            cwd: None,
            last_activity: None,
        }
    }

    fn agent_pane(target: &str, command: &str) -> TmuxPane {
        TmuxPane {
            id: "%2".to_owned(),
            command: command.to_owned(),
            target: target.to_owned(),
            title: "\u{2733} Claude Code".to_owned(),
            pid: Some(200),
            cwd: None,
            last_activity: None,
        }
    }

    #[test]
    fn serve_pane_looks_like_agent_matches_keywords_and_versioned_commands() {
        assert!(serve_pane_looks_like_agent("codex", "shell"));
        assert!(serve_pane_looks_like_agent("bash", "maw-rs-oracle"));
        assert!(serve_pane_looks_like_agent("2.1.219", "some task status line"));
        assert!(!serve_pane_looks_like_agent("bash", "nat-LOCAL (shell)"));
        assert!(!serve_pane_looks_like_agent("sudo", "MAWRS-REMOTE (live, read-only)"));
    }

    fn console_session() -> RouteSession {
        RouteSession {
            name: "33-maw-rs".to_owned(),
            windows: vec![RouteWindow { index: 0, name: "console".to_owned(), active: true, kind: None }],
            source: None,
        }
    }

    #[test]
    fn serve_window_name_for_resolved_target_resolves_index_to_name() {
        let sessions = vec![console_session()];
        assert_eq!(
            serve_window_name_for_resolved_target(&sessions, "33-maw-rs:0").as_deref(),
            Some("console")
        );
        assert_eq!(serve_window_name_for_resolved_target(&sessions, "33-maw-rs:9"), None);
        assert_eq!(serve_window_name_for_resolved_target(&sessions, "no-such-session:0"), None);
    }

    #[test]
    fn serve_non_agent_pane_warning_from_panes_flags_the_709_repro_shape() {
        // #709: 33-maw-rs used to be one window named maw-rs running Claude;
        // rebuilt as a two-pane bash/sudo console. hey blackmachine:33-maw-rs
        // resolves to window INDEX 0, but list_panes reports panes by window
        // NAME ("console") -- the index must be resolved through the session
        // list before a pane lookup can find anything at all.
        let sessions = vec![console_session()];
        let panes = vec![
            non_agent_pane("33-maw-rs:console.0"),
            non_agent_pane("33-maw-rs:console.1"),
        ];
        let warning = serve_non_agent_pane_warning_from_panes(&sessions, &panes, "33-maw-rs:0")
            .expect("bash pane must warn");
        assert!(warning.contains("33-maw-rs:0"), "{warning}");
        assert!(warning.contains("console"), "{warning}");
        assert!(warning.contains("bash"), "{warning}");
        assert!(warning.contains("not an agent"), "{warning}");
    }

    #[test]
    fn serve_non_agent_pane_warning_from_panes_is_silent_for_real_agents_and_unknown_targets() {
        let sessions = vec![console_session()];
        let panes = vec![agent_pane("33-maw-rs:console.0", "2.1.219")];
        assert!(serve_non_agent_pane_warning_from_panes(&sessions, &panes, "33-maw-rs:0").is_none());
        assert!(
            serve_non_agent_pane_warning_from_panes(&sessions, &panes, "no-such-session:0").is_none(),
            "target not found is a different, already-handled failure mode"
        );
    }

    #[test]
    fn serve_peer_refresh_interval_defaults_and_parses() {
        let _guard = env_test_lock();
        let _restore = EnvVarRestore::capture("MAW_PEER_REFRESH_SECS");

        std::env::remove_var("MAW_PEER_REFRESH_SECS");
        assert_eq!(
            serve_peer_refresh_interval_secs(),
            SERVE_PEER_REFRESH_DEFAULT_SECS,
            "missing env → default cadence"
        );

        std::env::set_var("MAW_PEER_REFRESH_SECS", "30");
        assert_eq!(serve_peer_refresh_interval_secs(), 30);

        std::env::set_var("MAW_PEER_REFRESH_SECS", " 0 ");
        assert_eq!(serve_peer_refresh_interval_secs(), 0, "0 disables the sweep");

        std::env::set_var("MAW_PEER_REFRESH_SECS", "not-a-number");
        assert_eq!(
            serve_peer_refresh_interval_secs(),
            SERVE_PEER_REFRESH_DEFAULT_SECS,
            "unparseable env → default, never a panic"
        );
    }

    #[tokio::test]
    async fn federation_ls_path_is_mounted_and_dead_api_ls_stays_404() {
        // #676: `maw ls --federation` GET'd /api/ls, which NO serve mounts, so it 404'd
        // and blamed healthy peers. The client now targets /api/sessions. Guard the round
        // trip: the dead path must stay 404, and a mounted route (/api/health, tmux-free)
        // must not — so the test proves the router discriminates, not that all paths 404.
        let app = serve_test_app_with_plugin_routes(Vec::new());
        let health = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .expect("health request"),
            )
            .await
            .expect("health response");
        assert_ne!(
            health.status(),
            StatusCode::NOT_FOUND,
            "control: a mounted route must not 404"
        );
        let dead = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/ls")
                    .body(Body::empty())
                    .expect("ls request"),
            )
            .await
            .expect("ls response");
        assert_eq!(
            dead.status(),
            StatusCode::NOT_FOUND,
            "#676: /api/ls is implemented by no serve; the client must not target it"
        );
    }

    use axum::body::Body;
    use futures_util::{SinkExt, StreamExt};
    use maw_auth::{build_legacy_from_sign_payload, hash_body, sign_headers_v3_at, sign_hmac_sig};
    use std::time::Duration;
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
                kind: None,
            }],
        }
    }

    fn serve_test_delivery() -> Arc<dyn ServeDelivery> {
        Arc::new(FakeServeDelivery::with_capture_agent())
    }

    #[derive(Default)]
    struct FakeServeWake {
        wakes: Mutex<Vec<(String, Option<String>)>>,
        error: Mutex<Option<String>>,
    }

    impl FakeServeWake {
        fn set_error(&self, error: &str) {
            *self.error.lock().expect("wake error") = Some(error.to_owned());
        }

        fn wakes(&self) -> Vec<(String, Option<String>)> {
            self.wakes.lock().expect("wakes").clone()
        }
    }

    impl ServeWakeExecutor for FakeServeWake {
        fn execute_wake(&self, target: &str, task: Option<&str>) -> Result<String, String> {
            if let Some(error) = self.error.lock().expect("wake error").clone() {
                return Err(error);
            }
            self.wakes
                .lock()
                .expect("wakes")
                .push((target.to_owned(), task.map(ToOwned::to_owned)));
            Ok(format!("woke {target}\n"))
        }
    }

    fn serve_test_wake() -> Arc<dyn ServeWakeExecutor> {
        Arc::new(FakeServeWake::default())
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
            peer_pubkeys: HotReload::frozen(Vec::new()),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            wake: serve_test_wake(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path,
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
            bound_port: DEFAULT_SERVE_PORT,
        })
    }

    fn serve_test_app_with_wake(
        trust_store_path: std::path::PathBuf,
        wake: Arc<dyn ServeWakeExecutor>,
    ) -> Router {
        serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: HotReload::frozen(Vec::new()),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            wake,
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path,
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
            bound_port: DEFAULT_SERVE_PORT,
        })
    }

    fn serve_test_app_with_plugin_routes(plugin_serve_routes: Vec<ServePluginRoute>) -> Router {
        serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: HotReload::frozen(Vec::new()),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            wake: serve_test_wake(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("plugins"),
            plugin_serve_routes,
            api_token_auth: ServeApiTokenAuth::open(),
            bound_port: DEFAULT_SERVE_PORT,
        })
    }

    fn serve_test_app_with_api_auth(api_token_auth: ServeApiTokenAuth) -> Router {
        serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: HotReload::frozen(Vec::new()),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            wake: serve_test_wake(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("api-token"),
            plugin_serve_routes: vec![ServePluginRoute {
                name: "testext".to_owned(),
                command: None,
                prefix: "/api/testext".to_owned(),
                health_path: "/api/testext/health".to_owned(),
                events: Vec::new(),
                event_path: None,
                dir: std::env::temp_dir(),
                process: Arc::new(Mutex::new(None)),
            }],
            api_token_auth,
            bound_port: DEFAULT_SERVE_PORT,
        })
    }

    fn serve_test_proxy_route(port: u16, child: Child) -> ServePluginRoute {
        ServePluginRoute {
            name: "testext".to_owned(),
            command: Some("sleep 60".to_owned()),
            prefix: "/api/testext".to_owned(),
            health_path: "/api/testext/health".to_owned(),
            events: Vec::new(),
            event_path: None,
            dir: std::env::temp_dir(),
            process: Arc::new(Mutex::new(Some(ServePluginProcess { port, child }))),
        }
    }

    fn signed_trust_request(method: &str, uri: &str, auth_path: &str, body: &'static str) -> axum::http::Request<Body> {
        let headers = sign_headers_v3_at(
            KEY,
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

    fn serve_test_app_with_o6_keys(
        keys: Vec<ServePeerPubkey>,
        now: i64,
        peer_addr_override: Option<SocketAddr>,
    ) -> Router {
        serve_test_app_with_o6_keys_and_delivery(keys, now, peer_addr_override, serve_test_delivery())
    }

    fn serve_test_app_with_o6_keys_and_delivery(
        keys: Vec<ServePeerPubkey>,
        now: i64,
        peer_addr_override: Option<SocketAddr>,
        delivery: Arc<dyn ServeDelivery>,
    ) -> Router {
        serve_test_app_with_o6_keys_delivery_and_inbox(
            keys,
            now,
            peer_addr_override,
            delivery,
            serve_test_receiver_inbox(),
        )
    }

    fn serve_test_app_with_o6_keys_delivery_and_inbox(
        keys: Vec<ServePeerPubkey>,
        now: i64,
        peer_addr_override: Option<SocketAddr>,
        delivery: Arc<dyn ServeDelivery>,
        receiver_inbox: Arc<dyn ServeReceiverInbox>,
    ) -> Router {
        serve_router(ServeState {
            cached_pubkey: None,
            peer_pubkeys: HotReload::frozen(keys),
            workspace_key: Some("capture-test-token-393av2".to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery,
            receiver_inbox,
            wake: serve_test_wake(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override,
            now_override: Some(now),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("o6"),
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
            bound_port: DEFAULT_SERVE_PORT,
        })
    }

    fn captured_send_fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/serve-auth/maw-js-hey-captured-api-send.json"
        ))
        .expect("captured maw-js fixture")
    }

    fn captured_send_key() -> ServePeerPubkey {
        let fixture = captured_send_fixture();
        let from = fixture["headers"]["X-Maw-From"]
            .as_str()
            .expect("from");
        serve_test_peer_pubkey(from, fixture["testPeerKey"].as_str().expect("peer key"))
    }

    fn captured_send_request() -> axum::http::Request<Body> {
        let fixture = captured_send_fixture();
        let method = fixture["method"].as_str().expect("method");
        let path = fixture["path"].as_str().expect("path");
        let body = fixture["body"].as_str().expect("body");
        let mut builder = axum::http::Request::builder().method(method).uri(path);
        for (name, value) in fixture["headers"].as_object().expect("headers") {
            builder = builder.header(name.as_str(), value.as_str().expect("header value"));
        }
        let mut request = builder.body(Body::from(body.to_owned())).expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }



    fn unsigned_json_request(method: &str, uri: &str, body: &'static str) -> axum::http::Request<Body> {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }

    fn signed_api_send_json_request(
        body: &'static str,
        key: &str,
        from: &str,
        now: i64,
    ) -> axum::http::Request<Body> {
        signed_json_request("POST", "/api/send", body, key, from, now)
    }

    fn signed_json_request(
        method: &str,
        path: &str,
        body: &'static str,
        key: &str,
        from: &str,
        now: i64,
    ) -> axum::http::Request<Body> {
        let headers = sign_headers_v3_at(key, key, from, method, path, Some(body.as_bytes()), now)
            .expect("sign v3");
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        for (name, value) in headers.to_btree_map() {
            builder = builder.header(name, value);
        }
        let mut request = builder.body(Body::from(body)).expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }


    #[tokio::test]
    async fn serve_send_accepts_signed_and_prefixes_bracket_text() {
        let body = r#"{"target":"capture-agent","text":"[fake:node] signed"}"#;
        let app = serve_test_app(serve_test_trust_store_path("signed-send"));
        let response = app.oneshot(signed_api_send_json_request(body, KEY, FROM, 1_782_277_200)).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["ok"], true);
    }

    #[tokio::test]
    async fn serve_send_flags_not_rejects_unsigned_legacy_loopback() {
        let app = serve_test_app_with_o6_keys(vec![], 1_782_277_200, Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49_152)));
        let mut unsigned_from = unsigned_json_request("POST", "/api/send", r#"{"target":"capture-agent","text":"[fake] hello"}"#);
        unsigned_from.headers_mut().insert("x-maw-from", axum::http::HeaderValue::from_static(FROM));
        let response = app.oneshot(unsigned_from).await.expect("unsigned legacy");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_send_rejects_mismatched_signature() {
        let signed_body = r#"{"target":"capture-agent","text":"hello"}"#;
        let mut request = signed_api_send_json_request(signed_body, KEY, FROM, 1_782_277_200);
        *request.body_mut() = Body::from(r#"{"target":"capture-agent","text":"tampered"}"#);
        let app = serve_test_app(serve_test_trust_store_path("v3-mismatch"));
        let response = app.oneshot(request).await.expect("mismatch");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn serve_wake_executes_receiver_side_and_reports_result() {
        let wake = Arc::new(FakeServeWake::default());
        let app = serve_test_app_with_wake(serve_test_trust_store_path("wake-exec"), wake.clone());
        let body = r#"{"target":"capture-agent","task":"fix issue"}"#;
        let response = app
            .oneshot(signed_json_request("POST", "/api/wake", body, KEY, FROM, 1_782_277_200))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["ok"], true);
        assert_eq!(json["target"], "capture-agent");
        assert_eq!(
            wake.wakes(),
            vec![("capture-agent".to_owned(), Some("fix issue".to_owned()))]
        );
    }

    #[tokio::test]
    async fn serve_wake_surfaces_receiver_failure_not_false_success() {
        let wake = Arc::new(FakeServeWake::default());
        wake.set_error("wake exited 1: wake: repo not found for bare-shell");
        let app = serve_test_app_with_wake(serve_test_trust_store_path("wake-fail"), wake.clone());
        let body = r#"{"target":"bare-shell"}"#;
        let response = app
            .oneshot(signed_json_request("POST", "/api/wake", body, KEY, FROM, 1_782_277_200))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = response_json(response).await;
        assert_eq!(json["ok"], false);
        assert_eq!(json["target"], "bare-shell");
        assert!(json["error"]
            .as_str()
            .expect("error")
            .contains("repo not found for bare-shell"));
        assert!(wake.wakes().is_empty());
    }

    // Regression test for #533: `run_wake_command` takes a VERB-STRIPPED argv
    // (the CLI dispatcher removes the `wake` verb before calling it), so the
    // receiver-side executor must not prepend the verb. The mock-executor
    // tests above never exercise the argv the real executor constructs, which
    // is exactly how `"wake"` slipped in as a second positional and every
    // receiver-side federation wake exited 1 with the usage error. This test
    // drives the REAL `ServeSystemWakeExecutor`: a nonexistent target must
    // reach real resolution ("repo not found"), never the usage guard.
    #[test]
    fn serve_system_wake_executor_passes_verb_stripped_argv() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let root = std::env::temp_dir().join(format!(
            "maw-rs-serve-wake-argv-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        std::fs::create_dir_all(root.join("config")).expect("fixture root");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let error = ServeSystemWakeExecutor
            .execute_wake("no-such-target-533", Some("issue-533"))
            .expect_err("nonexistent target must fail resolution, not succeed");

        assert!(
            !error.contains("usage: maw wake"),
            "usage error means the verb leaked into argv as a second positional: {error}"
        );
        assert!(
            error.contains("repo not found for no-such-target-533"),
            "expected real single-positional resolution failure for the target: {error}"
        );
    }

    #[tokio::test]
    async fn serve_wake_rejects_empty_target_without_executing() {
        let wake = Arc::new(FakeServeWake::default());
        let app = serve_test_app_with_wake(serve_test_trust_store_path("wake-empty"), wake.clone());
        let response = app
            .oneshot(signed_json_request("POST", "/api/wake", "{}", KEY, FROM, 1_782_277_200))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"], "empty-target");
        assert!(wake.wakes().is_empty());
    }

    #[tokio::test]
    async fn serve_wake_rejects_tampered_signature_without_executing() {
        let wake = Arc::new(FakeServeWake::default());
        let app = serve_test_app_with_wake(serve_test_trust_store_path("wake-tampered"), wake.clone());
        let signed_body = r#"{"target":"capture-agent"}"#;
        let mut request =
            signed_json_request("POST", "/api/wake", signed_body, KEY, FROM, 1_782_277_200);
        *request.body_mut() = Body::from(r#"{"target":"tampered"}"#);
        let response = app.oneshot(request).await.expect("tampered");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(wake.wakes().is_empty());
    }

    #[test]
    fn serve_peer_pubkey_collection_sets_node_for_identity_shapes() {
        let value = json!({
            "peers": {
                "nova:bigboy-vps": "node-key-a",
                "alias": {"pubkey": "node-key-b", "oracle": "seed", "node": "bigboy-vps"},
                "direct": {"pubkey": "node-key-c", "from": "gm-bo:bigboy-vps"}
            }
        });
        let mut entries = Vec::new();
        collect_peer_pubkeys(&value, None, &mut entries);
        assert!(entries.iter().any(|entry| entry.from == "nova:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == "node-key-a"));
        assert!(entries.iter().any(|entry| entry.from == "seed:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == "node-key-b"));
        assert!(entries.iter().any(|entry| entry.from == "gm-bo:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == "node-key-c"));
    }

    #[test]
    fn serve_peer_pubkey_collection_reads_maw_js_nested_identity_shape() {
        let value = json!({
            "version": 1,
            "peers": {
                "bigboy-vps": {
                    "url": "http://100.64.0.1:3456",
                    "node": "bigboy-vps",
                    "addedAt": "2026-06-28T00:00:00.000Z",
                    "lastSeen": "2026-06-28T00:01:00.000Z",
                    "pubkeyFirstSeen": "2026-06-24T00:00:00.000Z",
                    "pubkey": "node-key-bigboy-vps-401",
                    "identity": {"oracle": "mawjs", "node": "bigboy-vps"}
                }
            }
        });
        let mut entries = Vec::new();
        collect_peer_pubkeys(&value, None, &mut entries);
        assert!(entries.iter().any(|entry| entry.from == "mawjs:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == "node-key-bigboy-vps-401"));
    }

    // #789: `maw trust add <sender> <target> --peer-key <key>` (trust.rs)
    // reported "trusted" and persisted a pubkey, but `load_inbound_peer_pubkeys`
    // never read that store, so the trusted peer still 401'd on `/api/send`.
    // Round-trips through the real CLI writer (`trust_store_add`) rather than
    // hand-rolling JSON, so this also proves the shape `maw trust add` writes
    // is the shape inbound auth now reads.
    #[test]
    fn serve_load_inbound_peer_pubkeys_reads_trust_store_as_a_pubkey_source() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _xdg_config = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _xdg_state = EnvVarRestore::capture("XDG_STATE_HOME");
        let _maw_state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _maw_xdg = EnvVarRestore::capture("MAW_XDG");
        let root = std::env::temp_dir().join(format!(
            "maw-rs-serve-trust-inbound-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let state_dir = root.join("state");
        std::fs::create_dir_all(&state_dir).expect("state dir");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_STATE_HOME", &state_dir);
        std::env::set_var("MAW_STATE_DIR", &state_dir);
        std::env::remove_var("MAW_HOME");
        std::env::remove_var("MAW_XDG");

        let trust_path = trust_store_path();
        assert_eq!(trust_path, state_dir.join("trust-store.json"));
        trust_store_add(
            &trust_path,
            "hermes-agent:black",
            "m5",
            "trust-store-peer-key-789",
            1_782_277_200_000,
        )
        .expect("trust add");

        let entries = load_inbound_peer_pubkeys();
        assert!(
            entries.iter().any(|entry| entry.from == "hermes-agent:black"
                && entry.node == "black"
                && entry.pubkey == "trust-store-peer-key-789"),
            "trust-store.json entry did not surface as an inbound pubkey source: {entries:?}"
        );
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_accepts_unseeded_oracle_on_known_node() {
        let node_key = "node-key-bigboy-vps-399";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey("nova:bigboy-vps", node_key)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"hello node fallback"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("node fallback response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["state"], "delivered");
        assert_eq!(payload["target"], "capture-agent:0");
        let sends = delivery.sends();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0, "capture-agent:0");
        // #795: the receiving node's x-maw-from header ("alloy:bigboy-vps",
        // wire order oracle:node) must render human node:oracle, matching a
        // local delivery's tag order.
        assert_eq!(sends[0].1, "[bigboy-vps:alloy] hello node fallback");
    }

    #[tokio::test]
    async fn serve_api_send_dedups_cross_turn_duplicate_by_delivery_key() {
        let node_key = "node-key-bigboy-vps-399";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey("nova:bigboy-vps", node_key)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let first_body = r#"{"target":"capture-agent","text":"codex-2 DONE #87 full suite green"}"#;
        let intervening_body = r#"{"target":"capture-agent","text":"another turn between duplicate emissions"}"#;

        let first = app
            .clone()
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                first_body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("first response");
        let first_payload = response_json(first).await;
        assert_eq!(first_payload["state"], "delivered");

        let intervening = app
            .clone()
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                intervening_body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("intervening response");
        let intervening_payload = response_json(intervening).await;
        assert_eq!(intervening_payload["state"], "delivered");

        let duplicate = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                first_body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("duplicate response");
        let duplicate_status = duplicate.status();
        let duplicate_payload = response_json(duplicate).await;
        assert_eq!(duplicate_status, StatusCode::OK, "{duplicate_payload}");
        assert_eq!(duplicate_payload["state"], "delivered");
        assert_eq!(duplicate_payload["deduped"], true);
        assert_eq!(duplicate_payload["receipt"], json!(["duplicate_dropped"]));

        let sends = delivery.sends();
        assert_eq!(sends.len(), 2, "delayed replay must not reinject");
        assert_eq!(
            sends[0],
            (
                "capture-agent:0".to_owned(),
                "[bigboy-vps:alloy] codex-2 DONE #87 full suite green".to_owned()
            )
        );
        assert_eq!(
            sends[1],
            (
                "capture-agent:0".to_owned(),
                "[bigboy-vps:alloy] another turn between duplicate emissions".to_owned()
            )
        );
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_accepts_collected_maw_js_nested_identity_shape() {
        let node_key = "node-key-bigboy-vps-401";
        let value = json!({
            "version": 1,
            "peers": {
                "bigboy-vps": {
                    "url": "http://100.64.0.1:3456",
                    "node": "bigboy-vps",
                    "addedAt": "2026-06-28T00:00:00.000Z",
                    "lastSeen": "2026-06-28T00:01:00.000Z",
                    "pubkeyFirstSeen": "2026-06-24T00:00:00.000Z",
                    "pubkey": node_key,
                    "identity": {"oracle": "mawjs", "node": "bigboy-vps"}
                }
            }
        });
        let mut entries = Vec::new();
        collect_peer_pubkeys(&value, None, &mut entries);
        assert!(entries.iter().any(|entry| entry.from == "mawjs:bigboy-vps"
            && entry.node == "bigboy-vps"
            && entry.pubkey == node_key));

        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            entries,
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"hello nested identity"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("nested identity fallback response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["state"], "delivered");
        assert_eq!(payload["target"], "capture-agent:0");
        let sends = delivery.sends();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].0, "capture-agent:0");
        assert_eq!(sends[0].1, "[bigboy-vps:alloy] hello nested identity");
    }

    // #805: `/api/identity`'s port-based `user@` inference (serve_identity.rs's
    // serveidentity_infer_process_user, live whenever the daemon's port != 3456)
    // pins peers.json's `identity.node` as a compound "user@host" -- but a signed
    // request's `x-maw-from` always carries the BARE node, because
    // sender_identity.rs's signing path only ever reads config.node and has zero
    // awareness of port inference or nodeUser/serviceUser. Before the fix, the
    // collected entry kept `node == "agent@bigboy-vps"` while the incoming
    // request's derived node stayed `"bigboy-vps"` -- they never matched, so a
    // peer pinned via a non-default-port probe got refuse-missing-peer-key even
    // with the exactly correct pubkey. Auth-side normalization (stripping a
    // `user@` prefix wherever a node identity is extracted for MATCHING) fixes
    // this without touching what `/api/identity` reports for display.
    #[tokio::test]
    async fn serve_o6_node_fallback_matches_bare_from_against_compound_pinned_identity() {
        let node_key = "node-key-bigboy-vps-compound";
        let value = json!({
            "version": 1,
            "peers": {
                "bigboy-vps": {
                    "url": "http://100.64.0.1:3460",
                    "node": "bigboy-vps",
                    "addedAt": "2026-06-28T00:00:00.000Z",
                    "lastSeen": "2026-06-28T00:01:00.000Z",
                    "pubkeyFirstSeen": "2026-06-24T00:00:00.000Z",
                    "pubkey": node_key,
                    "identity": {"oracle": "alloy", "node": "agent@bigboy-vps"}
                }
            }
        });
        let mut entries = Vec::new();
        collect_peer_pubkeys(&value, None, &mut entries);
        assert!(
            entries
                .iter()
                .any(|entry| entry.node == "bigboy-vps" && entry.pubkey == node_key),
            "expected a user@-stripped bare-node entry, got {entries:?}"
        );

        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            entries,
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"hello bare after compound pin"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("compound-pinned bare-from response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["state"], "delivered");
        assert_eq!(delivery.sends().len(), 1);
    }

    #[tokio::test]
    async fn serve_o6_exact_mismatch_does_not_fallback_to_node_key() {
        let node_key = "node-key-bigboy-vps-399";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![
                serve_test_peer_pubkey("alloy:bigboy-vps", "wrong-exact-key-399"),
                serve_test_peer_pubkey("nova:bigboy-vps", node_key),
            ],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"exact must win"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("exact mismatch response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-mismatch");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_rejects_unknown_node() {
        let node_key = "node-key-bigboy-vps-399";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey("nova:bigboy-vps", node_key)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"unknown node"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "alloy:other-node",
                1_782_277_200,
            ))
            .await
            .expect("unknown node response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-missing-peer-key");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_rejects_ambiguous_node_keys() {
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![
                serve_test_peer_pubkey("nova:bigboy-vps", "node-key-a-399"),
                serve_test_peer_pubkey("seed:bigboy-vps", "node-key-b-399"),
            ],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"ambiguous node"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                "node-key-a-399",
                "alloy:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("ambiguous node response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-ambiguous-peer-key");
        assert!(delivery.sends().is_empty());
    }

    // #798: the node-only fallback used to trust "whoever holds this node's
    // signing key" for ANY claimed oracle name, regardless of whether that
    // name had ever been registered. `refuse-ambiguous-peer-key` above only
    // fired when two registered entries carried DIFFERENT pubkeys -- it did
    // nothing when two DIFFERENT oracles legitimately shared the SAME node
    // key, which is exactly the shape that let a third, never-registered
    // oracle name walk in on that shared key. These two tests prove the
    // decision this fix actually encodes: once a node has more than one
    // known oracle, the fallback stops trusting new names on that key, while
    // the oracles that ARE already registered keep working via the ordinary
    // exact-match path (unaffected -- it never went through the fallback).
    #[tokio::test]
    async fn serve_o6_node_fallback_rejects_unregistered_oracle_once_node_has_two_registered() {
        let node_key = "node-key-bigboy-vps-shared";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![
                serve_test_peer_pubkey("nova:bigboy-vps", node_key),
                serve_test_peer_pubkey("seed:bigboy-vps", node_key),
            ],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"spoofed third oracle"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "mallory:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("unregistered oracle response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-ambiguous-peer-key");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_node_fallback_still_accepts_each_registered_oracle_when_node_has_two() {
        let node_key = "node-key-bigboy-vps-shared";
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![
                serve_test_peer_pubkey("nova:bigboy-vps", node_key),
                serve_test_peer_pubkey("seed:bigboy-vps", node_key),
            ],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"registered oracle still works"}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                node_key,
                "nova:bigboy-vps",
                1_782_277_200,
            ))
            .await
            .expect("registered oracle response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["state"], "delivered");
        assert!(!delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_live_router_accepts_captured_maw_js_send_for_exact_from_key() {
        let app = serve_test_app_with_o6_keys(
            vec![
                serve_test_peer_pubkey("other-oracle:other-node", "wrong-first-peer-key"),
                captured_send_key(),
            ],
            1_782_553_858,
            Some(NON_LOOPBACK_TEST_PEER),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["state"], "delivered");
        assert_eq!(payload["target"], "capture-agent:0");
    }

    #[tokio::test]
    async fn serve_o6_send_rejects_unsigned_but_accepts_registered_maw_js_peer() {
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![captured_send_key()],
            1_782_553_858,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let unsigned = unsigned_json_request(
            "POST",
            "/api/send",
            r#"{"target":"capture-agent","text":"unsigned"}"#,
        );

        let response = app.clone().oneshot(unsigned).await.expect("unsigned send");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-unsigned");
        assert!(delivery.sends().is_empty());

        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("registered peer send");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(delivery.sends().len(), 1);
    }

    #[tokio::test]
    async fn serve_o6_live_router_rejects_captured_maw_js_send_when_exact_from_key_missing() {
        let app = serve_test_app_with_o6_keys(
            vec![serve_test_peer_pubkey("other-oracle:other-node", "wrong-first-peer-key")],
            1_782_553_858,
            Some(NON_LOOPBACK_TEST_PEER),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-missing-peer-key");
    }

    #[tokio::test]
    async fn serve_o6_live_router_rejects_captured_maw_js_send_with_wrong_from_key() {
        let mut key = captured_send_key();
        key.pubkey = "wrong-peer-key-393av2".to_owned();
        let app = serve_test_app_with_o6_keys(vec![key], 1_782_553_858, Some(NON_LOOPBACK_TEST_PEER));
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-mismatch");
    }

    #[tokio::test]
    async fn serve_o6_live_router_rejects_captured_maw_js_send_with_expired_timestamp() {
        let app = serve_test_app_with_o6_keys(
            vec![captured_send_key()],
            1_782_554_500,
            Some(NON_LOOPBACK_TEST_PEER),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{payload}");
        assert_eq!(payload["decision"], "refuse-skew");
    }

    #[tokio::test]
    async fn serve_o6_live_router_loopback_bypasses_from_key_resolution_separately() {
        let app = serve_test_app_with_o6_keys(
            Vec::new(),
            1_782_553_858,
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49_152)),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["state"], "delivered");
    }

    fn serve_test_inbox_repo(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "maw-rs-receiver-inbox-{label}-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("ψ")).expect("repo psi");
        repo
    }

    struct ServeInboxManifestEnv {
        _guard: std::sync::MutexGuard<'static, ()>,
        root: std::path::PathBuf,
        config: std::path::PathBuf,
        cache: std::path::PathBuf,
        ghq: std::path::PathBuf,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl ServeInboxManifestEnv {
        fn new(label: &str) -> Self {
            let guard = env_test_lock();
            let keys = [
                "HOME",
                "MAW_HOME",
                "MAW_CONFIG_DIR",
                "MAW_CACHE_DIR",
                "MAW_XDG",
                "XDG_CONFIG_HOME",
                "GHQ_ROOT",
            ];
            let saved = keys
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!(
                "maw-rs-receiver-inbox-manifest-{label}-{}-{}",
                std::process::id(),
                random_hex(4)
            ));
            let home = root.join("home");
            let config = root.join("config");
            let cache = root.join("cache");
            let ghq = root.join("ghq");
            std::fs::create_dir_all(config.join("fleet")).expect("fleet dir");
            std::fs::create_dir_all(&cache).expect("cache dir");
            std::fs::create_dir_all(ghq.join("github.com")).expect("ghq dir");
            std::env::set_var("HOME", &home);
            std::env::remove_var("MAW_HOME");
            std::env::remove_var("MAW_XDG");
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::set_var("MAW_CONFIG_DIR", &config);
            std::env::set_var("MAW_CACHE_DIR", &cache);
            std::env::set_var("GHQ_ROOT", ghq.join("github.com"));
            Self {
                _guard: guard,
                root,
                config,
                cache,
                ghq,
                saved,
            }
        }

        fn add_fleet_repo(
            &self,
            file: &str,
            session: &str,
            window: &str,
            repo: &str,
        ) -> std::path::PathBuf {
            let repo_path = self.ghq.join("github.com").join(repo);
            std::fs::create_dir_all(repo_path.join("ψ")).expect("repo psi");
            let fleet = json!({
                "name": session,
                "windows": [{"name": window, "repo": repo}],
            });
            std::fs::write(
                self.config.join("fleet").join(file),
                serde_json::to_string_pretty(&fleet).expect("fleet json"),
            )
            .expect("write fleet");
            repo_path
        }

        fn write_local_scanned_oracles_json(&self, name: &str, repo: &str, local_path: &std::path::Path) {
            let value = json!({
                "schema": 1,
                "oracles": [{
                    "org": "tonkmac",
                    "repo": repo,
                    "name": name,
                    "local_path": local_path.display().to_string(),
                    "has_psi": true,
                    "has_fleet_config": true,
                    "federation_node": "bigboy-vps"
                }]
            });
            std::fs::write(
                self.cache.join("oracles.json"),
                serde_json::to_string_pretty(&value).expect("oracles json"),
            )
            .expect("write oracles");
        }
    }

    impl Drop for ServeInboxManifestEnv {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_writes_receiver_inbox_without_tmux_send() {
        let repo = serve_test_inbox_repo("success");
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey("alloy:bigboy-vps", KEY)],
            1_782_623_880,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
            serve_test_receiver_inbox_at(&repo, 1_782_623_880_000),
        );
        let body = r#"{"target":"capture-agent","text":"hello nested inbox","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                KEY,
                "alloy:bigboy-vps",
                1_782_623_880,
            ))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["source"], "inbox");
        assert_eq!(payload["state"], "queued");
        assert_eq!(payload["target"], "capture-agent:0");
        assert_eq!(payload["receipt"], json!(["fallback_queued"]));
        assert_eq!(payload["reason"], "--inbox requested; pane injection skipped");
        assert!(delivery.sends().is_empty(), "inbox-only must not inject tmux");

        let expected = repo
            .join("ψ")
            .join("inbox")
            .join("2026-06-28_05-18_bigboy-vps-alloy_hello-nested-inbox.md");
        assert_eq!(payload["inbox"], expected.display().to_string());
        let written = std::fs::read_to_string(&expected).expect("inbox body");
        assert_eq!(
            written,
            "---\nfrom: bigboy-vps:alloy\nto: capture-agent\ntimestamp: 2026-06-28T05:18:00.000Z\nread: false\n---\n\nhello nested inbox\n"
        );
    }

    // #788: `maw locate <oracle> --path` finds a repo purely by scanning the
    // ghq root for a `<oracle>-oracle` directory — no fleet config or
    // registry-cache entry required. `receiver_inbox_repo_candidates` had no
    // equivalent scan, so `maw hey --inbox <oracle>` reported the receiver as
    // unroutable on the exact same name `maw locate` resolves fine. The two
    // resolvers must agree.
    #[test]
    fn receiver_inbox_falls_back_to_the_same_resolver_maw_locate_uses() {
        let env = ServeInboxManifestEnv::new("locate-fallback");
        let repo = env.ghq.join("github.com").join("tonkmac").join("widget-oracle");
        std::fs::create_dir_all(repo.join("ψ")).expect("bare ghq-scanned repo");
        // Deliberately no add_fleet_repo / write_local_scanned_oracles_json —
        // none of receiver_inbox_repo_candidates' existing sources (psi_root,
        // live-target cwd, fleet manifest) can see this repo.
        let config = HeyConfig { node: None, oracle: None, route: RouteConfig::default() };

        let result = persist_receiver_inbox(
            ReceiverInboxInput {
                query: "widget",
                target: None,
                to: Some("widget"),
                from: "bigboy-vps:alloy",
                message: "hello widget inbox",
                config: &config,
            },
            1_782_623_880_000,
            None,
        );

        let ReceiverInboxResult::Ok(ok) = result else {
            panic!("expected the ghq-scan fallback to resolve the repo: {result:?}");
        };
        assert_eq!(ok.oracle, "widget");
        assert_eq!(ok.inbox_dir, repo.join("ψ").join("inbox"));
        let written = std::fs::read_to_string(ok.path).expect("inbox body");
        assert!(written.contains("to: widget\n"));
    }

    #[test]
    fn receiver_inbox_manifest_phase_a_keeps_numbered_oracle_name_match() {
        let env = ServeInboxManifestEnv::new("phase-a");
        let repo = env.add_fleet_repo(
            "01-wish.json",
            "01-wish",
            "wish-oracle",
            "tonkmac/wish-oracle",
        );
        let config = HeyConfig {
            node: None,
            oracle: None,
            route: RouteConfig::default(),
        };
        let result = persist_receiver_inbox(
            ReceiverInboxInput {
                query: "wish",
                target: Some("wish"),
                to: Some("wish"),
                from: "bigboy-vps:alloy",
                message: "hello wish inbox",
                config: &config,
            },
            1_782_623_880_000,
            None,
        );
        let ReceiverInboxResult::Ok(ok) = result else {
            panic!("phase-a inbox write failed: {result:?}");
        };
        assert_eq!(ok.oracle, "wish");
        assert_eq!(ok.inbox_dir, repo.join("ψ").join("inbox"));
        let written = std::fs::read_to_string(ok.path).expect("inbox body");
        assert!(written.contains("to: wish\n"));
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_resolves_fleet_target_cwd_without_relabeling_oracle() {
        let env = ServeInboxManifestEnv::new("bigboylocal");
        let repo = env.add_fleet_repo(
            "02-bigboy.json",
            "02-bigboy",
            "bigboylocal-oracle",
            "tonkmac/bigboylocal-oracle",
        );
        env.write_local_scanned_oracles_json("bigboylocal", "bigboylocal-oracle", &repo);
        let delivery = Arc::new(FakeServeDelivery::default());
        delivery.set_sessions(vec![vec![serve_test_session(
            "02-bigboy",
            0,
            "bigboylocal-oracle",
        )]]);
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey("alloy:bigboy-vps", KEY)],
            1_782_623_880,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
            serve_test_receiver_inbox_from_manifest(1_782_623_880_000),
        );
        let body = r#"{"target":"02-bigboy","text":"hello bigboy inbox","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                KEY,
                "alloy:bigboy-vps",
                1_782_623_880,
            ))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["target"], "02-bigboy:0");
        assert_eq!(payload["source"], "inbox");
        assert!(delivery.sends().is_empty(), "inbox-only must not inject tmux");

        let expected = repo
            .join("ψ")
            .join("inbox")
            .join("2026-06-28_05-18_bigboy-vps-alloy_hello-bigboy-inbox.md");
        assert_eq!(payload["inbox"], expected.display().to_string());
        let written = std::fs::read_to_string(&expected).expect("inbox body");
        assert_eq!(
            written,
            concat!(
                "---\n",
                "from: bigboy-vps:alloy\n",
                "to: bigboy\n",
                "timestamp: 2026-06-28T05:18:00.000Z\n",
                "read: false\n",
                "---\n\n",
                "hello bigboy inbox\n"
            )
        );
    }

    #[test]
    fn receiver_inbox_target_cwd_matches_maw_js_window_selection_rules() {
        let env = ServeInboxManifestEnv::new("target-cwd");
        let repo = env.add_fleet_repo(
            "02-bigboy.json",
            "02-bigboy",
            "bigboylocal-oracle",
            "tonkmac/bigboylocal-oracle",
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("02-bigboy").expect("session"),
            Some(repo.clone())
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("02-bigboy:0").expect("index"),
            Some(repo.clone())
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("02-bigboy:bigboylocal-oracle").expect("window"),
            Some(repo.clone())
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("node:02-bigboy:bigboylocal-oracle")
                .expect("node window"),
            Some(repo)
        );
        assert_eq!(
            receiver_inbox_resolve_target_cwd("bigboy").expect("wrong owner"),
            None
        );
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_refuses_ambiguous_fleet_session_owner() {
        let env = ServeInboxManifestEnv::new("ambiguous");
        let repo_one = env.add_fleet_repo(
            "02-bigboy-a.json",
            "02-bigboy",
            "bigboylocal-oracle",
            "tonkmac/bigboylocal-oracle",
        );
        let repo_two = env.add_fleet_repo(
            "02-bigboy-b.json",
            "02-bigboy",
            "bigboylocal-alt-oracle",
            "tonkmac/bigboylocal-alt-oracle",
        );
        let delivery = Arc::new(FakeServeDelivery::default());
        delivery.set_sessions(vec![vec![serve_test_session(
            "02-bigboy",
            0,
            "bigboylocal-oracle",
        )]]);
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey("alloy:bigboy-vps", KEY)],
            1_782_623_880,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
            serve_test_receiver_inbox_from_manifest(1_782_623_880_000),
        );
        let body = r#"{"target":"02-bigboy","text":"hello ambiguous inbox","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                KEY,
                "alloy:bigboy-vps",
                1_782_623_880,
            ))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{payload}");
        assert_eq!(payload["error"], "receiver-inbox-unavailable");
        assert!(payload["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("receiver repo ambiguous"));
        assert!(delivery.sends().is_empty());
        assert!(!repo_one.join("ψ").join("inbox").exists());
        assert!(!repo_two.join("ψ").join("inbox").exists());
    }

    #[test]
    fn receiver_inbox_target_lookup_refuses_numeric_strip_wrong_owner() {
        let env = ServeInboxManifestEnv::new("wrong-owner");
        let _repo = env.add_fleet_repo(
            "02-bigboy.json",
            "02-bigboy",
            "bigboylocal-oracle",
            "tonkmac/bigboylocal-oracle",
        );
        let config = HeyConfig {
            node: None,
            oracle: None,
            route: RouteConfig::default(),
        };
        let result = persist_receiver_inbox(
            ReceiverInboxInput {
                query: "bigboy",
                target: Some("bigboy"),
                to: Some("bigboy"),
                from: "bigboy-vps:alloy",
                message: "hello wrong owner",
                config: &config,
            },
            1_782_623_880_000,
            None,
        );
        match result {
            ReceiverInboxResult::Err { oracle, reason } => {
                assert_eq!(oracle.as_deref(), Some("bigboy"));
                assert_eq!(reason, "receiver repo not found for bigboy");
            }
            ReceiverInboxResult::Ok(ok) => panic!("unexpected inbox write: {ok:?}"),
        }
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_disabled_fails_closed_without_fake_queue() {
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey(FROM, KEY)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let body = r#"{"target":"capture-agent","text":"hello","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request("POST", "/api/send", body, KEY, FROM, 1_782_277_200))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{payload}");
        assert_eq!(payload["state"], "failed");
        assert_eq!(payload["error"], "receiver-inbox-unavailable");
        assert!(payload["detail"].as_str().unwrap_or_default().contains("disabled"));
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_write_error_fails_closed_without_tmux_send() {
        let repo = serve_test_inbox_repo("write-error");
        std::fs::write(repo.join("ψ").join("inbox"), "not a dir").expect("block inbox dir");
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey(FROM, KEY)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
            serve_test_receiver_inbox_at(&repo, 1_782_277_200_000),
        );
        let body = r#"{"target":"capture-agent","text":"hello","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request("POST", "/api/send", body, KEY, FROM, 1_782_277_200))
            .await
            .expect("inbox response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{payload}");
        assert_eq!(payload["state"], "failed");
        assert_eq!(payload["error"], "receiver-inbox-unavailable");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_api_send_inbox_true_uses_exclusive_collision_suffix() {
        let repo = serve_test_inbox_repo("collision");
        let inbox_dir = repo.join("ψ").join("inbox");
        std::fs::create_dir_all(&inbox_dir).expect("inbox dir");
        let base = inbox_dir.join("2026-06-28_05-18_bigboy-vps-alloy_hello-nested-inbox.md");
        std::fs::write(&base, "existing").expect("existing base");
        let app = serve_test_app_with_o6_keys_delivery_and_inbox(
            vec![serve_test_peer_pubkey("alloy:bigboy-vps", KEY)],
            1_782_623_880,
            Some(NON_LOOPBACK_TEST_PEER),
            Arc::new(FakeServeDelivery::with_capture_agent()),
            serve_test_receiver_inbox_at(&repo, 1_782_623_880_000),
        );
        let body = r#"{"target":"capture-agent","text":"hello nested inbox","inbox":true}"#;
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/send",
                body,
                KEY,
                "alloy:bigboy-vps",
                1_782_623_880,
            ))
            .await
            .expect("inbox response");
        let payload = response_json(response).await;
        let suffixed = inbox_dir.join("2026-06-28_05-18_bigboy-vps-alloy_hello-nested-inbox-2.md");
        assert_eq!(payload["inbox"], suffixed.display().to_string());
        assert_eq!(std::fs::read_to_string(&base).expect("base"), "existing");
        assert!(suffixed.is_file());
    }

    #[tokio::test]
    async fn serve_api_send_toctou_refuses_disappeared_target_before_send() {
        let delivery = Arc::new(FakeServeDelivery::default());
        delivery.set_sessions(vec![
            vec![serve_test_session("capture-agent", 0, "capture-agent")],
            Vec::new(),
        ]);
        let app = serve_test_app_with_o6_keys_and_delivery(
            Vec::new(),
            1_782_553_858,
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49_152)),
            delivery.clone(),
        );
        let response = app
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{payload}");
        assert_eq!(payload["error"], "target-disappeared");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_api_send_auth_reject_is_logged_without_delivery() {
        let delivery = Arc::new(FakeServeDelivery::with_capture_agent());
        let app = serve_test_app_with_o6_keys_and_delivery(
            vec![serve_test_peer_pubkey("other-oracle:other-node", "wrong-first-peer-key")],
            1_782_553_858,
            Some(NON_LOOPBACK_TEST_PEER),
            delivery.clone(),
        );
        let rejected = app
            .clone()
            .oneshot(captured_send_request())
            .await
            .expect("captured send response");
        assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
        let feed = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/feed")
                    .body(Body::empty())
                    .expect("feed request"),
            )
            .await
            .expect("feed");
        let payload = response_json(feed).await;
        assert_eq!(payload["events"][0]["state"], "failed");
        assert_eq!(payload["events"][0]["decision"], "refuse-missing-peer-key");
        assert!(delivery.sends().is_empty());
    }

    #[tokio::test]
    async fn serve_o6_from_aware_key_resolution_also_unblocks_api_feed() {
        let app = serve_test_app_with_o6_keys(
            vec![serve_test_peer_pubkey(FROM, KEY)],
            1_782_277_200,
            Some(NON_LOOPBACK_TEST_PEER),
        );
        let response = app
            .oneshot(signed_json_request(
                "POST",
                "/api/feed",
                r#"{"event":"hello"}"#,
                KEY,
                FROM,
                1_782_277_200,
            ))
            .await
            .expect("feed response");
        let status = response.status();
        let payload = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert_eq!(payload["ok"], true);
    }

    async fn spawn_test_server() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let app = serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: HotReload::frozen(Vec::new()),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            wake: serve_test_wake(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("server"),
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
            bound_port: addr.port(),
        });
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("serve test server");
        });
        std::mem::forget(tx);
        addr
    }

    async fn spawn_plugin_proxy_server(route: ServePluginRoute) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind proxy");
        let addr = listener.local_addr().expect("proxy addr");
        let app = serve_test_app_with_plugin_routes(vec![route]);
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.expect("proxy server");
        });
        addr
    }

    #[tokio::test]
    async fn serve_real_wire_accepts_v3_rejects_unsigned_and_accepts_legacy() {
        let addr = spawn_test_server().await;
        let client = reqwest::Client::builder().build().expect("client");
        let url = format!("http://{addr}/api/send");
        let body = r#"{"target":"remote-oracle","text":"hello"}"#;
        let timestamp = 1_782_277_200_i64;
        let headers = sign_headers_v3_at(
            KEY,
            KEY,
            FROM,
            "POST",
            "/api/send",
            Some(body.as_bytes()),
            timestamp,
        )
        .expect("sign v3");
        let mut request = client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_owned());
        for (name, value) in headers.to_btree_map() {
            request = request.header(name, value);
        }
        let response = request.send().await.expect("send signed");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<Value>().await.expect("json");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["state"], "delivered");

        let response = client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("x-forwarded-for", "127.0.0.1")
            .body(body.to_owned())
            .send()
            .await
            .expect("send unsigned");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let signed_at = "2026-06-24T05:00:00.000Z";
        let now = 1_782_277_200_i64;
        let body_hash = hash_body(Some(body.as_bytes()));
        let payload = build_legacy_from_sign_payload(FROM, signed_at, "POST", "/api/send", &body_hash);
        let legacy_sig = sign_hmac_sig(KEY, &payload);
        let response = client
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("x-maw-from", FROM)
            .header("x-maw-signature", legacy_sig)
            .header("x-maw-signed-at", signed_at)
            .header("x-maw-auth-version", "v3")
            .header("x-maw-timestamp", now.to_string())
            .body(body.to_owned())
            .send()
            .await
            .expect("send legacy");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_plugin_proxy_websocket_passthrough() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind ws upstream");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept ws upstream");
            let mut ws = tokio_tungstenite::accept_async(stream).await.expect("accept websocket");
            assert_eq!(ws.next().await.expect("frame").expect("ok").into_text().expect("text"), "ping");
            ws.send(tokio_tungstenite::tungstenite::Message::Text("pong".to_owned())).await.expect("send pong");
        });
        let child = Command::new("/bin/sleep").arg("5").spawn().expect("sleep child");
        let addr = spawn_plugin_proxy_server(serve_test_proxy_route(port, child)).await;
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/testext/ws?room=1")).await.expect("connect proxy ws");
        ws.send(tokio_tungstenite::tungstenite::Message::Text("ping".to_owned())).await.expect("send ping");
        let reply = ws.next().await.expect("reply").expect("reply ok").into_text().expect("text");
        assert_eq!(reply, "pong");
    }

    #[tokio::test]
    async fn serve_plugin_proxy_spa_index_fallback_on_extensionless_404() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind upstream");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            for response in [b"HTTP/1.1 404 Not Found\r\nconnection: close\r\ncontent-length: 0\r\n\r\n".as_slice(), b"HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: 13\r\n\r\n<main></main>".as_slice()] {
                let (mut stream, _) = listener.accept().await.expect("accept upstream");
                let mut buf = [0_u8; 1024];
                let n = stream.read(&mut buf).await.expect("read request");
                let request = String::from_utf8_lossy(&buf[..n]);
                assert!(request.starts_with(if response[9] == b'4' { "GET /api/testext/board/42 " } else { "GET /api/testext/index.html " }));
                stream.write_all(response).await.expect("write response");
            }
        });
        let child = Command::new("/bin/sleep").arg("5").spawn().expect("sleep child");
        let app = serve_test_app_with_plugin_routes(vec![serve_test_proxy_route(port, child)]);
        let response = app.oneshot(axum::http::Request::get("/api/testext/board/42").body(Body::empty()).unwrap()).await.expect("proxy response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024).await.expect("body");
        assert_eq!(&body[..], b"<main></main>");
    }

    #[tokio::test]
    async fn serve_plugin_engine_command_prefix_http_proxies_when_process_is_up() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind upstream");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept upstream");
            let mut buf = [0_u8; 1024];
            let n = stream.read(&mut buf).await.expect("read request");
            let request = String::from_utf8_lossy(&buf[..n]);
            assert!(request.starts_with("GET /api/testext/assets/app.js?x=1 "));
            stream.write_all(b"HTTP/1.1 202 Accepted\r\ncontent-type: text/plain\r\ncontent-length: 7\r\n\r\nproxied").await.expect("write response");
        });
        let child = Command::new("/bin/sleep").arg("60").spawn().expect("sleep child");
        let app = serve_test_app_with_plugin_routes(vec![serve_test_proxy_route(port, child)]);
        let response = app.oneshot(axum::http::Request::get("/api/testext/assets/app.js?x=1").body(Body::empty()).unwrap()).await.expect("proxy response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024).await.expect("body");
        assert_eq!(&body[..], b"proxied");
    }

    #[tokio::test]
    async fn serve_plugin_health_falls_back_when_command_process_is_down() {
        let route = ServePluginRoute {
            name: "testext".to_owned(),
            command: Some("sleep 60".to_owned()),
            prefix: "/api/testext".to_owned(),
            health_path: "/api/testext/health".to_owned(),
            events: Vec::new(),
            event_path: None,
            dir: std::env::temp_dir(),
            process: Arc::new(Mutex::new(None)),
        };
        let app = serve_test_app_with_plugin_routes(vec![route]);
        let response = app.oneshot(axum::http::Request::get("/api/testext/health").body(Body::empty()).unwrap()).await.expect("health");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await;
        assert_eq!(payload["plugin"], "testext");
        assert_eq!(payload["command"], "sleep 60");
    }

    #[tokio::test]
    async fn serve_api_token_auth_gates_api_but_leaves_health_open() {
        let app = serve_test_app_with_api_auth(ServeApiTokenAuth {
            token: Some("secret-token".to_owned()),
            loopback_exempt: false,
            forced_open: false,
        });
        let denied = app.clone().oneshot(axum::http::Request::get("/api/feed").body(Body::empty()).unwrap()).await.expect("denied");
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

        let health = app.clone().oneshot(axum::http::Request::get("/api/health").body(Body::empty()).unwrap()).await.expect("health");
        assert_eq!(health.status(), StatusCode::OK);

        let bearer = app.clone().oneshot(
            axum::http::Request::get("/api/feed")
                .header("authorization", "Bearer secret-token")
                .body(Body::empty())
                .unwrap(),
        ).await.expect("bearer");
        assert_eq!(bearer.status(), StatusCode::OK);

        let plugin = app.oneshot(
            axum::http::Request::get("/api/testext/health")
                .header("x-maw-token", "secret-token")
                .body(Body::empty())
                .unwrap(),
        ).await.expect("plugin x token");
        assert_eq!(plugin.status(), StatusCode::OK);
    }

    fn serve_ws_upgrade_request(uri: &str) -> axum::http::request::Builder {
        axum::http::Request::get(uri)
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
    }

    // #828: /ws reaches `tmux send-keys`, but the gate only looked at `/api/`,
    // so an unauthenticated upgrade got past it (verified live: 101 from the LAN
    // while /api/feed answered 401 on the same daemon).
    #[tokio::test]
    async fn serve_ws_upgrade_is_gated_by_the_api_token() {
        let app = serve_test_app_with_api_auth(ServeApiTokenAuth {
            token: Some("secret-token".to_owned()),
            loopback_exempt: false,
            forced_open: false,
        });

        for uri in ["/ws?target=nova:1.0", "/ws/tmux", "/ws/pty"] {
            let denied = app
                .clone()
                .oneshot(serve_ws_upgrade_request(uri).body(Body::empty()).unwrap())
                .await
                .expect("ws denied");
            assert_eq!(denied.status(), StatusCode::UNAUTHORIZED, "{uri} unauthenticated");
        }

        // The credentialed upgrade still reaches the handler; oneshot has no
        // connection to hand over, so it stops at the extractor, not the gate.
        let allowed = app
            .oneshot(
                serve_ws_upgrade_request("/ws?target=nova:1.0")
                    .header("x-maw-token", "secret-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("ws token");
        assert_ne!(allowed.status(), StatusCode::UNAUTHORIZED);
    }

    // Open mode leaves the token unset, which is exactly when the old gate fell
    // open. A keystroke-injecting socket has to fail closed instead.
    #[tokio::test]
    async fn serve_ws_upgrade_refuses_when_no_token_is_configured() {
        let app = serve_test_app_with_api_auth(ServeApiTokenAuth::open());

        let feed = app
            .clone()
            .oneshot(axum::http::Request::get("/api/feed").body(Body::empty()).unwrap())
            .await
            .expect("open mode feed");
        assert_eq!(feed.status(), StatusCode::OK);

        let ws = app
            .oneshot(
                serve_ws_upgrade_request("/ws?target=nova:1.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("open mode ws");
        assert_eq!(ws.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn serve_api_token_auth_open_mode_is_backward_compatible() {
        let app = serve_test_app_with_api_auth(ServeApiTokenAuth::open());
        let response = app.oneshot(axum::http::Request::get("/api/feed").body(Body::empty()).unwrap()).await.expect("open mode");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_api_health_reports_configured_non_default_port() {
        let non_default_port = 3457_u16;
        assert_ne!(non_default_port, DEFAULT_SERVE_PORT);
        let app = serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: HotReload::frozen(Vec::new()),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            wake: serve_test_wake(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("health-port"),
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
            bound_port: non_default_port,
        });
        let response = app
            .oneshot(axum::http::Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .expect("health");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await;
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["port"], u64::from(non_default_port));
    }

    #[tokio::test]
    async fn serve_api_health_real_wire_reports_actually_bound_port() {
        let addr = spawn_test_server().await;
        assert_ne!(addr.port(), DEFAULT_SERVE_PORT, "ephemeral bind should not land on the default port");
        let client = reqwest::Client::builder().build().expect("client");
        let response = client
            .get(format!("http://{addr}/api/health"))
            .send()
            .await
            .expect("health");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<Value>().await.expect("json");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["port"], u64::from(addr.port()));
    }


    #[tokio::test]
    async fn serve_mounts_discovered_plugin_engine_serve_health_and_skips_bad_manifest() {
        let (root, plugin_routes) = {
            let _guard = env_test_lock();
            let _plugins_restore = EnvVarRestore::capture("MAW_PLUGINS_DIR");
            let root = std::env::temp_dir().join(format!(
                "maw-serve-plugin-{}-{}",
                std::process::id(),
                random_hex(4)
            ));
            let plugins = root.join("plugins");
            serve_write_plugin(
                &plugins,
                "testext",
                &json!({"prefix": "/api/testext", "health": "/health", "events": ["ready"], "eventPath": "/events"}),
            );
            serve_write_plugin(&plugins, "badext", &json!({"prefix": "/not-api/bad"}));
            std::env::set_var("MAW_PLUGINS_DIR", &plugins);
            (root, serve_discover_plugin_routes())
        };

        let app = serve_test_app_with_plugin_routes(plugin_routes);
        let health = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/testext/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("plugin health");
        assert_eq!(health.status(), StatusCode::OK);
        let payload = response_json(health).await;
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["plugin"], "testext");
        assert_eq!(payload["prefix"], "/api/testext");

        let missing = app
            .clone()
            .oneshot(
                axum::http::Request::get("/not-api/bad/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("bad plugin skipped");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let core = app
            .oneshot(axum::http::Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .expect("core health");
        assert_eq!(core.status(), StatusCode::OK);
        let _ = std::fs::remove_dir_all(root);
    }

    fn serve_write_plugin(root: &std::path::Path, name: &str, serve: &Value) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("plugin dir");
        std::fs::write(dir.join("index.ts"), "export default async function run() {}\n").expect("entry");
        std::fs::write(
            dir.join("plugin.json"),
            serde_json::to_vec_pretty(&json!({
                "name": name,
                "version": "1.0.0",
                "sdk": "*",
                "target": "js",
                "entry": "index.ts",
                "engine": {"serve": serve}
            }))
            .expect("manifest json"),
        )
        .expect("manifest");
    }

    #[tokio::test]
    async fn serve_trust_live_is_auth_gated_atomic_redacted_and_tofu_safe() {
        let path = serve_test_trust_store_path("route");
        let app = serve_test_app(path.clone());
        assert!(maw_auth::is_protected("/api/trust", "POST"));
        assert!(maw_auth::is_protected("/api/trust/revoke", "POST"));
        assert!(maw_auth::is_protected("/api/trust", "GET"));

        let secret_key = "ed25519:alpha-peer-key-secret";
        let body = r#"{"sender":"alpha","target":"beta","peerKey":"ed25519:alpha-peer-key-secret"}"#;
        let denied = app
            .clone()
            .oneshot(unsigned_trust_request("POST", "/api/trust", body))
            .await
            .expect("denied");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let trusted = app
            .clone()
            .oneshot(signed_trust_request("POST", "/api/trust", "/trust", body))
            .await
            .expect("trust");
        let trusted_status = trusted.status();
        let payload = response_json(trusted).await;
        assert_eq!(trusted_status, StatusCode::OK, "{payload}");
        let rendered = payload.to_string();
        assert_eq!(payload["peerKey"], "received (redacted)");
        assert!(!rendered.contains(secret_key), "{rendered}");
        let stored = std::fs::read_to_string(&path).expect("stored");
        assert!(stored.contains(secret_key));
        assert!(!path.with_extension("json.tmp").exists());

        let mismatch = r#"{"sender":"beta","target":"alpha","peerKey":"ed25519:different-peer-key"}"#;
        let rejected = app
            .clone()
            .oneshot(signed_trust_request("POST", "/api/trust", "/trust", mismatch))
            .await
            .expect("mismatch");
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        let rejected_payload = response_json(rejected).await.to_string();
        assert!(rejected_payload.contains("peer-key mismatch"));
        assert!(!rejected_payload.contains("different-peer-key"));

        let listed = app
            .clone()
            .oneshot(signed_trust_request("GET", "/api/trust", "/trust", ""))
            .await
            .expect("list");
        assert_eq!(listed.status(), StatusCode::OK);
        let listed_payload = response_json(listed).await.to_string();
        assert!(listed_payload.contains("received (redacted)"));
        assert!(!listed_payload.contains(secret_key));

        let missing_yes = r#"{"sender":"alpha","target":"beta"}"#;
        let refused = app
            .clone()
            .oneshot(signed_trust_request(
                "POST",
                "/api/trust/revoke",
                "/trust/revoke",
                missing_yes,
            ))
            .await
            .expect("missing yes");
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);

        let revoke = r#"{"sender":"alpha","target":"beta","yes":true}"#;
        let revoked = app
            .oneshot(signed_trust_request(
                "POST",
                "/api/trust/revoke",
                "/trust/revoke",
                revoke,
            ))
            .await
            .expect("revoke");
        assert_eq!(revoked.status(), StatusCode::OK);
        let entries = trust_read_store(&path).expect("read after revoke");
        assert!(entries.is_empty());
        let _ = std::fs::remove_file(path);
    }

    // #866: `/api/sessions` and `/api/capture` returned 200 to any
    // unauthenticated peer that could reach the bound port (default
    // `0.0.0.0`). Unlike `/api/trust`, they were never added to
    // `is_protected`'s allowlist, so Mechanism B (the peer-signature
    // default-deny gate) never covered them -- and Mechanism A (the
    // bearer-token gate, `serve_api_token_gate`) falls open for all of
    // `/api/*` whenever no `serve.token` is configured, which is the default
    // state exercised by `serve_test_app` (`ServeApiTokenAuth::open()`).
    /// `serve_test_app`, but with the auth clock set to `now` instead of the
    /// fixture's pinned 1_782_277_200. Needed whenever the REAL client-side
    /// signer produces the headers: it stamps the real wall clock, which is
    /// ~54 days outside the pinned fixture's 300s window (`WINDOW_SEC`), so a
    /// correctly signed request would 403 for a reason that has nothing to do
    /// with what the test is checking.
    fn serve_test_app_at(trust_store_path: std::path::PathBuf, now: i64) -> Router {
        serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: HotReload::frozen(Vec::new()),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            wake: serve_test_wake(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(now),
            serve_core_state_override: None,
            trust_store_path,
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
            bound_port: DEFAULT_SERVE_PORT,
        })
    }

    /// A signed cross-node GET, built the way a real federated client builds one:
    /// `sign_headers_v3_at` emits BOTH the v3 from-signature and the fleet-token
    /// `X-Maw-Signature` over the same `auth_path`, so this takes one path rather
    /// than letting the two signatures disagree (which is exactly the peek bug).
    fn signed_peer_get(uri: &str, auth_path: &str) -> axum::http::Request<Body> {
        let headers = sign_headers_v3_at(KEY, KEY, FROM, "GET", auth_path, Some(b""), 1_782_277_200)
            .expect("sign peer get");
        let mut builder = axum::http::Request::builder().method("GET").uri(uri);
        for (name, value) in headers.to_btree_map() {
            builder = builder.header(name, value);
        }
        let mut request = builder.body(Body::empty()).expect("request");
        request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        request
    }

    fn loopback_get(uri: &str) -> axum::http::Request<Body> {
        let mut request = axum::http::Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("loopback request");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 51_000)));
        request
    }

    #[tokio::test]
    async fn serve_sessions_requires_signed_peer_auth_but_stays_loopback_open() {
        let app = serve_test_app(serve_test_trust_store_path("sessions-auth"));

        // The hole itself, asserted on the wire: an unsigned GET from a
        // NON-loopback peer must not be served. Pre-fix this is a 200 carrying
        // every session/window/cwd/PID on the box.
        let denied = app
            .clone()
            .oneshot(unsigned_trust_request("GET", "/api/sessions", ""))
            .await
            .expect("denied sessions");
        assert_eq!(
            denied.status(),
            StatusCode::FORBIDDEN,
            "#866: unsigned GET /api/sessions from a non-loopback peer must be refused, not served"
        );

        // A validly signed peer request is accepted -- the route is gated, not
        // closed. Signed over the path the server verifies against (`/sessions`),
        // which is `servecore_api_auth_path`: no query, no `/api` prefix.
        let signed = app
            .clone()
            .oneshot(signed_peer_get("/api/sessions", "/sessions"))
            .await
            .expect("signed sessions");
        assert_eq!(
            signed.status(),
            StatusCode::OK,
            "#866: a correctly signed peer must still be able to read /api/sessions"
        );

        // Loopback stays open: `verify_serve_request` (maw-auth) exempts a
        // loopback peer before it ever checks for a signature -- independent of
        // the bearer-token gate's own (separate) `loopback_exempt` flag. A local
        // dashboard/CLI on 127.0.0.1 must keep working unsigned.
        let loopback = app
            .oneshot(loopback_get("/api/sessions"))
            .await
            .expect("loopback sessions");
        assert_eq!(loopback.status(), StatusCode::OK);

        assert!(maw_auth::is_protected("/sessions", "GET"));
    }

    #[tokio::test]
    async fn serve_capture_requires_signed_peer_auth_but_stays_loopback_open() {
        let app = serve_test_app(serve_test_trust_store_path("capture-auth"));

        // Pre-fix this reaches the handler and answers on tmux's behalf (200 with
        // verbatim pane content, or 400 when the target does not resolve in the
        // test env); either way it is NOT refused, which is the defect.
        let denied = app
            .clone()
            .oneshot(unsigned_trust_request("GET", "/api/capture?target=nova:1.0", ""))
            .await
            .expect("denied capture");
        assert_eq!(
            denied.status(),
            StatusCode::FORBIDDEN,
            "#866: unsigned GET /api/capture from a non-loopback peer must be refused, not reach tmux"
        );

        // Signed peers still get through the gate. The handler itself may 400 on
        // an unresolvable target with no tmux server in the test env -- what is
        // asserted here is that auth no longer refuses it.
        let signed = app
            .clone()
            .oneshot(signed_peer_get("/api/capture?target=nova:1.0", "/capture"))
            .await
            .expect("signed capture");
        assert_ne!(
            signed.status(),
            StatusCode::FORBIDDEN,
            "#866: a correctly signed peer must not be refused on /api/capture"
        );

        let loopback = app
            .oneshot(loopback_get("/api/capture?target=nova:1.0"))
            .await
            .expect("loopback capture");
        assert_ne!(loopback.status(), StatusCode::FORBIDDEN);
        assert_ne!(loopback.status(), StatusCode::UNAUTHORIZED);

        assert!(maw_auth::is_protected("/capture", "GET"));
    }

    // The caller half of #866. The two cross-node readers of `/api/sessions`
    // (`ls_fetch_peer_sessions` for `maw ls --federation`, and the serve-side
    // `federation_fetch_peer_sessions` sweep) both went out BARE before this
    // change, so protecting the route would have 403'd them against every
    // patched peer. Both now sign via `federation_signed_get_headers`; this
    // drives that real client-side helper and feeds its output to the real
    // router, so a signing change that stops authenticating fails here rather
    // than in production between two nodes.
    #[tokio::test]
    async fn serve_federation_signed_get_headers_authenticate_against_the_real_router() {
        let _guard = env_test_lock();
        let _token = EnvVarRestore::capture("MAW_FEDERATION_TOKEN");
        let _peer_key = EnvVarRestore::capture("MAW_PEER_KEY");
        let _sender = EnvVarRestore::capture("MAW_SENDER");
        // serve_test_app trusts KEY as both the workspace key and the cached
        // pubkey, and FROM ("sender-oracle:sender-node") as the identity.
        // MAW_SENDER is human-ordered node:oracle.
        std::env::set_var("MAW_FEDERATION_TOKEN", KEY);
        std::env::set_var("MAW_PEER_KEY", KEY);
        std::env::set_var("MAW_SENDER", "sender-node:sender-oracle");

        let headers =
            federation_signed_get_headers("/api/sessions").expect("client-side signing must work");
        let map = headers.to_btree_map();

        // Assert the ACTUAL wire string, not just that the request succeeds.
        // `validate_wire_from` accepts any two non-empty colon-separated parts
        // WITHOUT checking their order, and the fallback builds the value as
        // `{oracle}:{node}` while MAW_SENDER is human-ordered `{node}:{oracle}`
        // -- so a swap is silently accepted and the peer just files us under the
        // wrong identity. A status-only assertion would pass with the halves
        // swapped; this one does not.
        let wire_from = map
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("x-maw-from"))
            .map(|(_, value)| value.clone())
            .expect("signed headers must carry x-maw-from");
        assert_eq!(
            wire_from, FROM,
            "wire identity must be oracle:node ({FROM}), not the human node:oracle ordering"
        );

        let mut builder = axum::http::Request::builder()
            .method("GET")
            .uri("/api/sessions");
        for (name, value) in map {
            builder = builder.header(name, value);
        }
        let mut request = builder.body(Body::empty()).expect("request");
        request
            .extensions_mut()
            .insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));

        let now = i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX);
        let app = serve_test_app_at(serve_test_trust_store_path("caller-signing"), now);
        let response = app
            .clone()
            .oneshot(request)
            .await
            .expect("signed caller response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "#866: the headers our federated callers now send must authenticate, not 403"
        );

        // Forward compatibility with peers on an OLD build. There, `/sessions`
        // is not on the protected allowlist, so `servecore_auth_default_deny`
        // returns `next.run(req)` before reading a single header -- the added
        // signing headers are inert. `/api/feed` is unprotected on THIS build
        // too, so sending the same signed headers at it reproduces exactly that
        // old-peer path: extra headers must not turn a served route into a
        // refusal. (Mechanism A is unaffected either way: `token_matches` only
        // ever reads `Authorization: Bearer` / `x-maw-token`.)
        let mut old_peer_builder = axum::http::Request::builder().method("GET").uri("/api/feed");
        for (name, value) in headers.to_btree_map() {
            old_peer_builder = old_peer_builder.header(name, value);
        }
        let mut old_peer_request = old_peer_builder.body(Body::empty()).expect("request");
        old_peer_request
            .extensions_mut()
            .insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        let old_peer = app
            .oneshot(old_peer_request)
            .await
            .expect("old-peer response");
        assert_eq!(
            old_peer.status(),
            StatusCode::OK,
            "#866: signing headers must be inert on an unprotected route -- that is what an old peer sees"
        );
    }

    // The peek half of the caller audit. `maw peek <peer>:<target>` (#820) is
    // the ONLY cross-node caller of `/api/capture`, and it signed the path WITH
    // the query string -- headers that were attached and never verified while
    // the route was unprotected. Drives peek's real signing helper and feeds its
    // output to the real router, so reverting `PEEK_CAPTURE_AUTH_PATH` to the
    // query form turns this red instead of breaking peek in production.
    #[tokio::test]
    async fn serve_peek_signed_capture_headers_authenticate_against_the_real_router() {
        let _guard = env_test_lock();
        let _token = EnvVarRestore::capture("MAW_FEDERATION_TOKEN");
        let _peer_key = EnvVarRestore::capture("MAW_PEER_KEY");
        std::env::set_var("MAW_FEDERATION_TOKEN", KEY);
        std::env::set_var("MAW_PEER_KEY", KEY);

        let now = i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX);
        let headers = peek_signed_capture_headers(FROM, now).expect("peek signing must work");

        let mut builder = axum::http::Request::builder()
            .method("GET")
            // The real peek URL: query present here, absent from what was signed.
            .uri("/api/capture?target=nova%3A1.0");
        for (name, value) in headers.to_btree_map() {
            builder = builder.header(name, value);
        }
        let mut request = builder.body(Body::empty()).expect("request");
        request
            .extensions_mut()
            .insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));

        let app = serve_test_app_at(serve_test_trust_store_path("peek-signing"), now);
        let response = app.oneshot(request).await.expect("peek signed response");
        assert_ne!(
            response.status(),
            StatusCode::FORBIDDEN,
            "#866: maw peek's signed capture must authenticate, not 403 -- it signs the query-less path"
        );
    }

    // #866 fallout, found while auditing callers: the server verifies a signature
    // over `servecore_api_auth_path(uri.path())` -- QUERY STRIPPED, `/api`
    // stripped (candidates: `/capture`, `/api/capture`). `maw peek` signed
    // `/api/capture?target=...` *including the query*, which matches neither.
    // That never surfaced because `/capture` was unprotected, so the signature
    // was attached and never checked. Protecting the route turns that latent
    // mismatch into a live 403 on a just-shipped feature (#820), so peek now
    // signs the query-less path. This pins both halves of that decision.
    #[tokio::test]
    async fn serve_capture_signature_covers_path_without_query_like_peek_now_sends() {
        let app = serve_test_app(serve_test_trust_store_path("capture-sigpath"));

        // What peek sent before this fix: signed over the path WITH the query.
        let with_query = app
            .clone()
            .oneshot(signed_peer_get(
                "/api/capture?target=nova:1.0",
                "/api/capture?target=nova:1.0",
            ))
            .await
            .expect("query-signed capture");
        assert_eq!(
            with_query.status(),
            StatusCode::FORBIDDEN,
            "signing the query string does not match what the server verifies -- this is why peek had to change"
        );

        // What peek sends now: signed over the query-less path.
        let without_query = app
            .oneshot(signed_peer_get("/api/capture?target=nova:1.0", "/capture"))
            .await
            .expect("path-signed capture");
        assert_ne!(
            without_query.status(),
            StatusCode::FORBIDDEN,
            "the query-less signed path is what the server accepts"
        );
    }

    #[test]
    fn serve_default_bind_matches_maw_js_parity_and_ignores_maw_host() {
        let _guard = env_test_lock();
        let _restore = EnvVarRestore::capture("MAW_HOST");
        std::env::set_var("MAW_HOST", "127.0.0.1");
        let args = parse_serve_args(&[]).expect("default serve args");
        assert_eq!(args.host, "0.0.0.0");
        assert_eq!(args.port, 3456);
        assert_eq!(
            resolve_serve_socket_addr(&args).expect("default bind"),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 3456)
        );
    }

    #[tokio::test]
    async fn serve_host_port_override_resolves_and_binds_throwaway_loopback() {
        let args = parse_serve_args(&[
            "--host".to_owned(),
            "127.0.0.1".to_owned(),
            "--port".to_owned(),
            "0".to_owned(),
        ])
        .expect("override serve args");
        let addr = resolve_serve_socket_addr(&args).expect("override bind");
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(addr.port(), 0);
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("throwaway loopback bind");
        assert_eq!(
            listener.local_addr().expect("local addr").ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
    }

    #[test]
    fn serve_host_validation_rejects_injection_before_bind() {
        for host in ["", "-0.0.0.0", "127.0.0.1\nx", "localhost"] {
            let args = ServeArgs {
                host: host.to_owned(),
                port: 3456,
                cached_pubkey: None,
            };
            assert_eq!(
                resolve_serve_socket_addr(&args),
                Err("serve: --host must be an IP address".to_owned()),
                "host={host:?}"
            );
        }
    }

    #[tokio::test]
    async fn serve_core_real_router_allows_loopback_protected_paths() {
        let addr = spawn_test_server().await;
        let client = reqwest::Client::builder().build().expect("client");
        let trigger = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .json(&json!({"event":"agent-idle","context":{"repo":"maw-rs"}}))
            .send()
            .await
            .expect("protected request");
        assert_eq!(trigger.status(), StatusCode::OK, "/api/triggers/fire");
        let plugins = client
            .post(format!("http://{addr}/api/plugins/reload"))
            .send()
            .await
            .expect("protected request");
        assert_eq!(plugins.status(), StatusCode::OK, "/api/plugins/reload");
        let cleanup = client
            .post(format!("http://{addr}/api/worktrees/cleanup"))
            .send()
            .await
            .expect("protected request");
        assert_eq!(
            cleanup.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "/api/worktrees/cleanup is live JSON route, not core stub"
        );
        let public = client
            .get(format!("http://{addr}/api/agents"))
            .send()
            .await
            .expect("public request");
        assert_eq!(public.status(), StatusCode::OK);
        let costs = client
            .get(format!("http://{addr}/api/costs"))
            .header("origin", "https://god.buildwithoracle.com")
            .send()
            .await
            .expect("costs request");
        assert_eq!(costs.status(), StatusCode::OK, "/api/costs");
        assert_eq!(
            costs
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://god.buildwithoracle.com")
        );
        let missing = client
            .get(format!("http://{addr}/api/missing-god-ui-route"))
            .header("origin", "https://god.buildwithoracle.com")
            .send()
            .await
            .expect("missing request");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            missing
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://god.buildwithoracle.com")
        );
    }

    #[tokio::test]
    async fn serve_agents_real_router_is_public_and_uses_fake_state() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let fake_core = crate::serve_core::ServecoreSharedState::default()
            .servecore_with_agents_node(Some("node-a".to_owned()))
            .servecore_with_agents_snapshot(vec![crate::serve_core::ServecoreAgentPane {
                id: "%86".to_owned(),
                command: "codex".to_owned(),
                target: "nova:1.0".to_owned(),
                title: "nova-agent".to_owned(),
                cwd: Some("/tmp/maw-rs".to_owned()),
                pid: Some(8600),
                last_activity: Some(86),
            }]);
        let app = serve_router(ServeState {
            cached_pubkey: Some(KEY.to_owned()),
            peer_pubkeys: HotReload::frozen(Vec::new()),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            wake: serve_test_wake(),
            delivery_idempotency: Mutex::new(DeliveryIdempotencyStore::default()),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: Some(fake_core),
            trust_store_path: serve_test_trust_store_path("agents"),
            plugin_serve_routes: Vec::new(),
            api_token_auth: ServeApiTokenAuth::open(),
            bound_port: addr.port(),
        });
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("serve test server");
        });
        std::mem::forget(tx);

        let client = reqwest::Client::builder().build().expect("client");
        let response = client
            .get(format!("http://{addr}/api/agents"))
            .send()
            .await
            .expect("agents");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<Value>().await.expect("json");
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["node"], "node-a");
        assert_eq!(payload["agents"][0]["target"], "nova:1.0");

        let protected = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .json(&json!({"event":"agent-idle","context":{"repo":"maw-rs"}}))
            .send()
            .await
            .expect("protected");
        assert_eq!(protected.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_real_wire_websocket_subscribe_returns_native_ack_not_echo() {
        let addr = spawn_test_server().await;
        let url = format!("ws://{addr}/ws");
        let (mut ws, _response) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connect websocket");

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"type":"subscribe","target":"demo:1"}"#.to_owned(),
        ))
        .await
        .expect("send websocket text");

        let ack = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let received = ws
                    .next()
                    .await
                    .expect("websocket should yield a frame")
                    .expect("frame should be ok");
                if let tokio_tungstenite::tungstenite::Message::Text(text) = received {
                    let value = serde_json::from_str::<Value>(&text).expect("json");
                    if value["type"] == "subscribed" {
                        assert_eq!(value["target"], "demo:1");
                        break;
                    }
                }
            }
        })
        .await;
        assert!(ack.is_ok(), "websocket should ack subscribe after stream frames");
    }

    #[tokio::test]
    async fn workspace_hub_signed_routes_accept_and_unsigned_rejects() {
        let addr = spawn_test_server().await;
        let client = reqwest::Client::builder().build().expect("client");
        let create_url = format!("http://{addr}/api/workspace/create");
        let create_response = client
            .post(create_url)
            .json(&json!({"name": "nova", "nodeId": "node-a"}))
            .send()
            .await
            .expect("create workspace");
        assert_eq!(create_response.status(), StatusCode::OK);
        let create_payload = create_response.json::<Value>().await.expect("create json");
        let workspace_id = create_payload["id"].as_str().expect("workspace id");
        let token = create_payload["token"].as_str().expect("workspace token");
        assert_eq!(token.len(), 64);

        let agents_path = format!("/api/workspace/{workspace_id}/agents");
        let agents_url = format!("http://{addr}{agents_path}");
        let unsigned = client
            .post(&agents_url)
            .json(&json!({"name": "nova-codex-1", "nodeId": "node-a"}))
            .send()
            .await
            .expect("unsigned agents request");
        assert_eq!(unsigned.status(), StatusCode::UNAUTHORIZED);

        let timestamp = "1782277200";
        let signature = sign_hmac_sig(token, &format!("POST:{agents_path}:{timestamp}"));
        let signed = client
            .post(&agents_url)
            .header("x-maw-timestamp", timestamp)
            .header("x-maw-signature", signature)
            .json(&json!({
                "name": "nova-codex-1",
                "nodeId": "node-a",
                "status": "online",
                "capabilities": ["relay"]
            }))
            .send()
            .await
            .expect("signed agents request");
        assert_eq!(signed.status(), StatusCode::OK);
        let signed_payload = signed.json::<Value>().await.expect("signed json");
        assert_eq!(signed_payload["ok"], true);
        assert_eq!(signed_payload["agents"], 1);
    }
}
