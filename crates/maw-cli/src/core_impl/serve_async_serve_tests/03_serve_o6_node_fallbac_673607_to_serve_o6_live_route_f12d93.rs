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
        assert_eq!(sends[0].1, "[alloy:bigboy-vps] hello node fallback");
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
        assert_eq!(sends[0].1, "[alloy:bigboy-vps] hello nested identity");
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

