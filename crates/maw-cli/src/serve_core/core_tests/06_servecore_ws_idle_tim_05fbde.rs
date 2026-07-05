    #[tokio::test]
    async fn servecore_ws_idle_timeout_closes_dead_connection() {
        let config = modules::ws::WsConfig {
            idle_timeout: Duration::from_millis(80),
            heartbeat_interval: Duration::from_millis(20),
            send_timeout: Duration::from_millis(50),
            max_frame_bytes: 1024,
            max_connections: 8,
        };
        let addr = servecore_spawn_ws_test_server(ServecoreSharedState::default(), config).await;
        let (mut ws, _response) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .expect("connect websocket");
        let close = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) = ws.next().await
                {
                    break;
                }
            }
        })
        .await;
        assert!(close.is_ok());
    }
