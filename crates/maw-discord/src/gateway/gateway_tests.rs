use super::*;

fn config() -> GatewayConfig {
    GatewayConfig::new(
        "test-bot",
        Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::DIRECT_MESSAGES,
    )
    .backoff(Duration::from_millis(1), Duration::from_millis(2))
}

#[tokio::test]
async fn mock_events_fan_out_to_subscribers() {
    let source = MockGatewaySource::new(vec![
        Ok(Some(Event::GatewayHeartbeat)),
        Ok(Some(Event::GatewayHeartbeatAck)),
    ]);
    let handle = spawn_gateway_with_source(config(), source);
    let mut rx = handle.subscribe();

    let first = rx.recv().await.expect("first event");
    let second = rx.recv().await.expect("second event");
    assert_eq!(
        first.kind,
        twilight_model::gateway::event::EventType::GatewayHeartbeat
    );
    assert_eq!(
        second.kind,
        twilight_model::gateway::event::EventType::GatewayHeartbeatAck
    );

    let stats = handle.shutdown().await;
    assert!(stats.events >= 2);
}

#[tokio::test]
async fn stream_end_reports_reconnect_without_live_discord() {
    let source = MockGatewaySource::new(vec![Ok(None)]);
    let handle = spawn_gateway_with_source(config(), source);
    let mut status = handle.status();

    loop {
        status.changed().await.expect("status update");
        if let GatewayStatus::Reconnecting { attempt, .. } = *status.borrow() {
            assert_eq!(attempt, 1);
            break;
        }
    }

    let stats = handle.shutdown().await;
    assert!(stats.reconnects >= 1);
}

#[test]
fn gateway_token_debug_is_redacted() {
    let token = GatewayToken::from_mock("mock-token-never-logged");
    assert_eq!(format!("{token:?}"), "GatewayToken(<redacted>)");
}

#[test]
fn backoff_is_capped() {
    assert_eq!(
        next_backoff(Duration::from_millis(8), Duration::from_millis(10)),
        Duration::from_millis(10)
    );
}
