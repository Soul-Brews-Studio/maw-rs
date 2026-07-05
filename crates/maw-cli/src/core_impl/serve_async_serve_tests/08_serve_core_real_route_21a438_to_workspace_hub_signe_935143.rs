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
            peer_pubkeys: Vec::new(),
            workspace_key: Some(KEY.to_owned()),
            workspaces: Mutex::new(WorkspaceStore::default()),
            requests: Mutex::new(RequestReplyStore::default()),
            delivery: serve_test_delivery(),
            receiver_inbox: serve_test_receiver_inbox(),
            feed: Mutex::new(Vec::new()),
            peer_addr_override: Some(NON_LOOPBACK_TEST_PEER),
            now_override: Some(1_782_277_200),
            serve_core_state_override: Some(fake_core),
            trust_store_path: serve_test_trust_store_path("agents"),
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
    async fn serve_real_wire_websocket_relay_echoes_text_frame() {
        let addr = spawn_test_server().await;
        let url = format!("ws://{addr}/ws");
        let (mut ws, _response) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connect websocket");

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            "relay-check".to_owned(),
        ))
        .await
        .expect("send websocket text");

        let received = ws
            .next()
            .await
            .expect("websocket should yield a frame")
            .expect("frame should be ok");
        assert_eq!(
            received,
            tokio_tungstenite::tungstenite::Message::Text("relay-check".to_owned())
        );
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
