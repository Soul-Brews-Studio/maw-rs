    #[tokio::test]
    async fn serve_real_wire_accepts_v3_rejects_unsigned_and_accepts_legacy() {
        let addr = spawn_test_server().await;
        let client = reqwest::Client::builder().build().expect("client");
        let url = format!("http://{addr}/api/send");
        let body = r#"{"target":"remote-oracle","text":"hello"}"#;
        let timestamp = 1_782_277_200_i64;
        let headers = sign_headers_v3_at(
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

    #[test]
    fn serve_default_bind_matches_maw_js_parity_and_ignores_maw_host() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
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

