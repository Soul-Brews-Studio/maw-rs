//! Long-lived Discord Gateway service built on twilight.
//!
//! Tests inject [`GatewayEventSource`] implementations and never open a live
//! Discord websocket. Production constructors resolve the token through the
//! existing host secret abstraction in `maw-discord`.

use crate::DiscordEnv;
use std::{future::Future, pin::Pin, time::Duration};
use tokio::{
    sync::{broadcast, watch},
    time::sleep,
};
use twilight_gateway::{EventTypeFlags, Shard, StreamExt as _};
use twilight_model::gateway::{event::Event, Intents, ShardId};

const FANOUT_CAPACITY: usize = 256;
const DEFAULT_BACKOFF_MS: u64 = 500;
const MAX_BACKOFF_MS: u64 = 30_000;

/// Offline-testable event source seam.
pub trait GatewayEventSource: Send {
    fn next_event<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Event>, String>> + Send + 'a>>;
}

struct TwilightEventSource {
    shard: Shard,
}

impl TwilightEventSource {
    fn new(shard_id: ShardId, token: GatewayToken, intents: Intents) -> Self {
        Self {
            shard: Shard::new(shard_id, token.into_inner(), intents),
        }
    }
}

impl GatewayEventSource for TwilightEventSource {
    fn next_event<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Event>, String>> + Send + 'a>> {
        Box::pin(async move {
            self.shard
                .next_event(EventTypeFlags::all())
                .await
                .transpose()
                .map_err(|err| format!("gateway receive error: {err}"))
        })
    }
}

/// Start a production twilight gateway service. This connects to Discord when polled.
///
/// # Errors
///
/// Returns a redacted token-resolution error before any websocket is created.
pub fn spawn_gateway(env: &DiscordEnv, config: GatewayConfig) -> Result<GatewayHandle, String> {
    let token = resolve_gateway_token(env, &config)?;
    let source = TwilightEventSource::new(config.shard_id, token, config.intents);
    Ok(spawn_gateway_with_source(config, source))
}

/// Start a gateway service with an injected event source for mock/offline tests.
#[must_use]
pub fn spawn_gateway_with_source<S>(config: GatewayConfig, source: S) -> GatewayHandle
where
    S: GatewayEventSource + 'static,
{
    let capacity = config.fanout_capacity.max(1);
    let (events, _) = broadcast::channel(capacity);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let (status_tx, status_rx) = watch::channel(GatewayStatus::Starting {
        shard_id: config.shard_id,
    });
    let join = tokio::spawn(run_gateway_loop(
        config,
        Box::new(source),
        events.clone(),
        status_tx,
        shutdown_rx,
    ));
    GatewayHandle {
        events,
        status: status_rx,
        shutdown,
        join,
    }
}

async fn run_gateway_loop(
    config: GatewayConfig,
    mut source: Box<dyn GatewayEventSource>,
    events: broadcast::Sender<GatewayEvent>,
    status: watch::Sender<GatewayStatus>,
    mut shutdown: watch::Receiver<bool>,
) -> GatewayRunStats {
    let mut stats = GatewayRunStats::default();
    let mut attempt = 0_u32;
    let mut backoff = config.initial_backoff;
    let _ = status.send(GatewayStatus::Connected {
        shard_id: config.shard_id,
    });

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() {
                    let _ = status.send(GatewayStatus::Stopped { shard_id: config.shard_id });
                    return stats;
                }
            }
            next = source.next_event() => match next {
                Ok(Some(event)) => {
                    backoff = config.initial_backoff;
                    let sent = events.send(GatewayEvent {
                        shard_id: config.shard_id,
                        kind: event.kind(),
                        event,
                    });
                    if let Ok(subscribers) = sent {
                        if subscribers == 0 {
                            stats.lagged_subscribers = stats.lagged_subscribers.saturating_add(1);
                        }
                    }
                    stats.events = stats.events.saturating_add(1);
                }
                Ok(None) | Err(_) => {
                    attempt = attempt.saturating_add(1);
                    stats.reconnects = stats.reconnects.saturating_add(1);
                    let _ = status.send(GatewayStatus::Reconnecting {
                        shard_id: config.shard_id,
                        attempt,
                    });
                    sleep(backoff).await;
                    backoff = next_backoff(backoff, config.max_backoff);
                }
            },
        }
    }
}

fn next_backoff(current: Duration, max: Duration) -> Duration {
    current.saturating_mul(2).min(max)
}

mod gateway_contract;
mod mock_event_source;

pub use gateway_contract::{
    resolve_gateway_token, GatewayConfig, GatewayEvent, GatewayHandle, GatewayRunStats,
    GatewayStatus, GatewayToken,
};
pub use mock_event_source::MockGatewaySource;

/// Observe mock gateway events through [`GatewayHandle::subscribe`].
///
/// This is a hermetic downstream seam for CLI surfaces that need to prove they
/// consume the gateway fanout without opening a live Discord websocket.
pub async fn observe_mock_gateway_events(events: &[String]) -> usize {
    use tokio::time::{timeout, Duration};
    use twilight_model::gateway::event::Event;

    let mocked = events
        .iter()
        .map(|event| match event.as_str() {
            "heartbeat-ack" | "GatewayHeartbeatAck" => Ok(Some(Event::GatewayHeartbeatAck)),
            _ => Ok(Some(Event::GatewayHeartbeat)),
        })
        .collect::<Vec<_>>();
    let handle = spawn_gateway_with_source(
        GatewayConfig::new("mock-gateway", twilight_model::gateway::Intents::GUILDS)
            .backoff(Duration::from_millis(1), Duration::from_millis(1)),
        MockGatewaySource::new(mocked),
    );
    let mut rx = handle.subscribe();
    let mut count = 0usize;
    for _ in events {
        if timeout(Duration::from_millis(50), rx.recv()).await.is_ok() {
            count = count.saturating_add(1);
        }
    }
    let _ = handle.shutdown().await;
    count
}

#[cfg(test)]
mod gateway_tests;
