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

