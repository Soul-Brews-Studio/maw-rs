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
            trust_store_path: serve_test_trust_store_path("server"),
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

