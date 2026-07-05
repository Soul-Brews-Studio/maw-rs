    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn serve_peers_info_routes_return_public_metadata_for_peer_probe() {
        let _guard = env_test_lock().lock().unwrap_or_else(|error| error.into_inner());
        let _restore_home = EnvVarRestore::capture("HOME");
        let _restore_maw_home = EnvVarRestore::capture("MAW_HOME");
        let _restore_maw_state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _restore_maw_config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _restore_peer = EnvVarRestore::capture("MAW_PEER_KEY");
        let root = std::env::temp_dir().join(format!(
            "maw-rs-peers-info-{}-{}",
            std::process::id(),
            random_hex(4)
        ));
        let home = root.join("home");
        let state = root.join("state");
        let config = root.join("config");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&state).expect("state");
        std::fs::create_dir_all(&config).expect("config");
        std::fs::write(
            config.join("maw.config.json"),
            r#"{"node":"node-a","oracle":"oracle-a"}"#,
        )
        .expect("config");
        std::env::set_var("HOME", &home);
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_STATE_DIR", &state);
        std::env::set_var("MAW_CONFIG_DIR", &config);
        std::env::set_var("MAW_PEER_KEY", "pub-peers-info-test");

        assert!(!maw_auth::is_protected("/info", "GET"));
        assert!(!maw_auth::is_protected("/api/peers/info", "GET"));

        let app = serve_test_app_with_o6_keys(Vec::new(), 1_782_277_200, Some(NON_LOOPBACK_TEST_PEER));
        for path in ["/info", "/api/peers/info"] {
            let mut request = axum::http::Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .expect("request");
            request.extensions_mut().insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let payload = response_json(response).await;
            assert_eq!(payload["node"], "node-a");
            assert_eq!(payload["host"], "node-a");
            assert!(payload.get("oracle").is_none(), "/info must stay maw-js-safe fields only: {payload}");
            assert!(payload.get("identity").is_none(), "/info must not embed full identity payload: {payload}");
            assert!(payload.get("pubkey").is_none(), "/info must not expose peer_key: {payload}");
            assert!(!payload.to_string().contains("pub-peers-info-test"), "/info leaked peer_key: {payload}");
            assert_eq!(payload["reachability"]["status"], "reachable");
            assert_eq!(payload["maw"]["schema"], "1");
            assert!(payload["endpoints"]
                .as_array()
                .expect("endpoints")
                .iter()
                .any(|value| value == "/api/peers/info"));
        }

        let mut protected = axum::http::Request::builder()
            .method("GET")
            .uri("/api/trust")
            .body(Body::empty())
            .expect("request");
        protected
            .extensions_mut()
            .insert(ConnectInfo(NON_LOOPBACK_TEST_PEER));
        let response = app.oneshot(protected).await.expect("protected response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let _ = std::fs::remove_dir_all(root);
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
            peer_pubkeys: keys,
            workspace_key: Some("capture-test-token-393av2".to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery,
            receiver_inbox,
            feed: Mutex::new(Vec::new()),
            peer_addr_override,
            now_override: Some(now),
            serve_core_state_override: None,
            trust_store_path: serve_test_trust_store_path("o6"),
        })
    }

    fn captured_send_fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/serve-auth/maw-js-hey-captured-api-send.json"
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

    fn signed_json_request(
        method: &str,
        path: &str,
        body: &'static str,
        key: &str,
        from: &str,
        now: i64,
    ) -> axum::http::Request<Body> {
        let headers = sign_headers_v3_at(key, from, method, path, Some(body.as_bytes()), now)
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

