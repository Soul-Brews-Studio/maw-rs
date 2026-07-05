    async fn servecore_auth_request(
        state: ServecoreSharedState,
        mut request: Request<Body>,
        peer: SocketAddr,
    ) -> Response {
        request.extensions_mut().insert(ConnectInfo(peer));
        request.extensions_mut().insert(Arc::new(state.clone()));
        servecore_auth_test_app(state)
            .oneshot(request)
            .await
            .expect("auth request")
    }

    #[tokio::test]
    async fn servecore_nonloopback_no_credentials_and_xff_spoof_fail_closed() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .body(Body::empty())
            .expect("request");
        let response = servecore_auth_request(ServecoreSharedState::default(), request, peer).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .header("x-forwarded-for", "127.0.0.1")
            .body(Body::empty())
            .expect("request");
        let response = servecore_auth_request(ServecoreSharedState::default(), request, peer).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn servecore_accepts_real_maw_js_stacked_fleet_hmac_v3_headers() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let body = br#"{"event":"agent-idle"}"#;
        let state = ServecoreSharedState::default()
            .servecore_with_auth(Some("fake-federation-token-393".to_owned()), None)
            .servecore_with_auth_now(1_700_000_000);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .header("x-maw-from", "nova:codex4")
            .header(
                "x-maw-signature",
                "536c867f3d9aa1f97c6c00c6b7e0337fe3d6d9c47ce1e38efe9d58d726d2c821",
            )
            .header(
                "x-maw-signature-v3",
                "19603ec4c4b9c6ad630809f50bc346066bb553b557b07d9809dfb62d4fb714a2",
            )
            .header("x-maw-timestamp", "1700000000")
            .header("x-maw-auth-version", "v3")
            .body(Body::from(body.as_slice().to_vec()))
            .expect("request");
        let response = servecore_auth_request(state, request, peer).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn servecore_rejects_wrong_fleet_token_even_with_valid_from_sign_header() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let body = br#"{"event":"agent-idle"}"#;
        let state = ServecoreSharedState::default()
            .servecore_with_auth(Some("wrong-federation-token".to_owned()), None)
            .servecore_with_auth_now(1_700_000_000);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .header("x-maw-from", "nova:codex4")
            .header(
                "x-maw-signature",
                "536c867f3d9aa1f97c6c00c6b7e0337fe3d6d9c47ce1e38efe9d58d726d2c821",
            )
            .header(
                "x-maw-signature-v3",
                "19603ec4c4b9c6ad630809f50bc346066bb553b557b07d9809dfb62d4fb714a2",
            )
            .header("x-maw-timestamp", "1700000000")
            .header("x-maw-auth-version", "v3")
            .body(Body::from(body.as_slice().to_vec()))
            .expect("request");
        let response = servecore_auth_request(state, request, peer).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn servecore_ed25519_from_sign_allows_nonloopback_and_pins_first_contact() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let body = br#"{"event":"agent-idle"}"#;
        let state = ServecoreSharedState::default().servecore_with_auth_now(1_700_000_000);
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/triggers/fire")
            .header("x-maw-from", "mawjs:m5")
            .header(
                "x-maw-ed25519-signature",
                concat!(
                    "d232e00767facc77aca0eaaf2ebc18dc3c608639430f93167679805c7e3ccf69",
                    "f15a856c7d8f4eddf64730cc61d4ccc0c28ca91b9a9df1a5016c628d737b3a0f"
                ),
            )
            .header(
                "x-maw-ed25519-pubkey",
                "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664",
            )
            .header("x-maw-timestamp", "1700000000")
            .header("x-maw-auth-version", "ed25519")
            .body(Body::from(body.as_slice().to_vec()))
            .expect("request");
        let response = servecore_auth_request(state.clone(), request, peer).await;
        assert_eq!(response.status(), StatusCode::OK);
        let pins = state
            .auth_ed25519_pins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            pins.pinned("mawjs:m5"),
            Some("79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664")
        );
    }

    #[tokio::test]
    async fn servecore_orchestration_workon_is_auth_gated_and_loopback_can_spawn_fake() {
        let peer = SocketAddr::from(([198, 51, 100, 10], 49_152));
        let payload = r#"{"repo":"demo","engine":"any-engine","target":"nova:1"}"#;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/orchestration/workon")
            .body(Body::from(payload))
            .expect("request");
        let response = servecore_auth_request(ServecoreSharedState::default(), request, peer).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let orchestrator = Arc::new(FakeOrchestrator::default());
        let state =
            ServecoreSharedState::default().servecore_with_orchestrator(orchestrator.clone());
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/orchestration/workon")
            .body(Body::from(payload))
            .expect("request");
        let response =
            servecore_auth_request(state, request, SocketAddr::from(([127, 0, 0, 1], 49_152)))
                .await;
        assert_eq!(response.status(), StatusCode::OK);
        let calls = orchestrator
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].engine.as_deref(), Some("any-engine"));
    }

    #[tokio::test]
    async fn servecore_ws_uses_engine_hook_and_loopback_auth() {
        let engine = Arc::new(TestEngine::default());
        let state = ServecoreSharedState::default().servecore_with_engine(engine.clone());
        let addr = servecore_spawn_ws_test_server(state, modules::ws::WsConfig::default()).await;
        let url = format!("ws://{addr}/ws/tmux?target=nova:1.0");
        let (mut ws, _response) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connect websocket");
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            "hello".to_owned(),
        ))
        .await
        .expect("send");
        loop {
            let received = ws.next().await.expect("frame").expect("frame ok");
            if let tokio_tungstenite::tungstenite::Message::Text(text) = received {
                assert_eq!(text, "Tmux:nova:1.0:hello");
                break;
            }
        }
        assert_eq!(
            engine
                .opened
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_slice(),
            &[(ServecoreWsKind::Tmux, Some("nova:1.0".to_owned()))]
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let protected = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .send()
            .await
            .expect("protected");
        assert_eq!(protected.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn servecore_ws_rejects_bad_tunnel_target_before_upgrade() {
        let addr = servecore_spawn_ws_test_server(
            ServecoreSharedState::default(),
            modules::ws::WsConfig::default(),
        )
        .await;
        let err = tokio_tungstenite::connect_async(format!("ws://{addr}/ws/tmux?target=-danger"))
            .await
            .expect_err("bad target must be rejected before upgrade");
        assert!(err.to_string().contains("400"));
    }

