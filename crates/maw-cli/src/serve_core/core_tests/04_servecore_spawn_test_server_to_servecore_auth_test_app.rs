    async fn servecore_spawn_test_server() -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = servecore_apply_pipeline(servecore_mount_core_routes(Router::new()));
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("server");
        });
        std::mem::forget(tx);
        addr
    }

    async fn servecore_spawn_ws_test_server(
        state: ServecoreSharedState,
        config: modules::ws::WsConfig,
    ) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_mount_ws_routes_with_config(router, config);
        let router = servecore_with_shared_state(router, state);
        let app = servecore_apply_pipeline_with_views_config(
            router,
            modules::views::ViewsConfig::views_with_paths(
                std::env::temp_dir().join("maw-rs-ws-no-ui"),
                std::env::temp_dir().join("maw-rs-ws-no-door.html"),
                std::env::temp_dir().join("maw-rs-ws-no-topology.html"),
            ),
        );
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("server");
        });
        std::mem::forget(tx);
        addr
    }

    #[derive(Debug, Default)]
    struct TestEngine {
        opened: Mutex<Vec<(ServecoreWsKind, Option<String>)>>,
    }

    impl ServecoreEngine for TestEngine {
        fn servecore_engine_name(&self) -> &'static str {
            "test"
        }

        fn servecore_ws_open(
            &self,
            kind: ServecoreWsKind,
            target: Option<&str>,
        ) -> Result<(), String> {
            let mut guard = self
                .opened
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.push((kind, target.map(ToOwned::to_owned)));
            Ok(())
        }

        fn servecore_ws_text(
            &self,
            kind: ServecoreWsKind,
            text: &str,
            target: Option<&str>,
        ) -> Option<String> {
            Some(format!("{kind:?}:{}:{text}", target.unwrap_or("none")))
        }
    }

    #[test]
    fn servecore_route_registry_rejects_duplicates_and_accepts_params() {
        let mut registry = ServecoreRouteRegistry::default();
        registry
            .servecore_register(Method::GET, "/api/agent/:id")
            .expect("first");
        let duplicate = registry.servecore_register(Method::GET, "/api/agent/:id");
        assert!(duplicate
            .expect_err("duplicate")
            .contains("duplicate route"));
        registry
            .servecore_register(Method::POST, "/api/agent/:id")
            .expect("method distinct");
        assert_eq!(registry.servecore_routes().len(), 2);
    }

    #[test]
    fn servecore_ws_registry_rejects_duplicates() {
        let mut registry = ServecoreWsRegistry::default();
        registry.servecore_register_ws("/ws").expect("ws");
        registry
            .servecore_register_ws_kind("/ws/pty", ServecoreWsKind::Pty)
            .expect("pty");
        registry
            .servecore_register_ws_kind("/ws/tmux", ServecoreWsKind::Tmux)
            .expect("tmux");
        assert!(registry
            .servecore_register_ws("/ws")
            .expect_err("dup")
            .contains("duplicate ws"));
        assert_eq!(
            registry.servecore_paths(),
            vec!["/ws", "/ws/pty", "/ws/tmux"]
        );
        assert_eq!(
            registry.servecore_handlers(),
            vec![
                ("/ws".to_owned(), ServecoreWsKind::Engine),
                ("/ws/pty".to_owned(), ServecoreWsKind::Pty),
                ("/ws/tmux".to_owned(), ServecoreWsKind::Tmux),
            ]
        );
    }

    #[test]
    fn servecore_lifecycle_sorts_by_weight_then_name_and_whitelists() {
        let modules = vec![
            ServecoreLifecycleModule {
                name: "triggers".to_owned(),
                weight: 20,
            },
            ServecoreLifecycleModule {
                name: "agents".to_owned(),
                weight: 10,
            },
            ServecoreLifecycleModule {
                name: "debug".to_owned(),
                weight: 10,
            },
        ];
        let enabled = ServecoreLifecycle::servecore_from_profile(
            modules,
            &["debug".to_owned(), "triggers".to_owned()],
        );
        assert_eq!(
            enabled.servecore_enabled_modules(),
            vec!["debug", "triggers"]
        );
    }

    #[test]
    fn servecore_pipeline_order_matches_maw_js_surface() {
        assert_eq!(
            servecore_pipeline_order(),
            [
                "cors-preflight",
                "ws-upgrade",
                "engine-proxy",
                "api-protected-auth",
                "registry",
                "api-public",
                "registry",
                "fallback-views",
            ]
        );
    }

    #[tokio::test]
    async fn servecore_loopback_allows_protected_paths_and_public_still_passes() {
        let addr = servecore_spawn_test_server().await;
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
        let plugins = client
            .post(format!("http://{addr}/api/plugins/reload"))
            .send()
            .await
            .expect("plugins");
        assert_eq!(plugins.status(), StatusCode::OK);
        let public = client
            .get(format!("http://{addr}/api/serve-core/pipeline"))
            .send()
            .await
            .expect("public");
        assert_eq!(public.status(), StatusCode::OK);
    }

    fn servecore_auth_test_app(state: ServecoreSharedState) -> Router {
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_with_shared_state(router, state);
        servecore_apply_pipeline(router)
    }

