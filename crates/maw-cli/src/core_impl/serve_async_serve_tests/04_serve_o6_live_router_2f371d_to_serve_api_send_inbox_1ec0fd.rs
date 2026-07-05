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
            let guard = env_test_lock().lock().unwrap_or_else(|error| error.into_inner());
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

