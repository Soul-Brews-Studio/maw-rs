use crate::{decrypt_token, list_pass_tokens, DiscordEnv};
use std::time::Duration;
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
};
use twilight_model::gateway::{event::Event, Intents, ShardId};

use super::{DEFAULT_BACKOFF_MS, FANOUT_CAPACITY, MAX_BACKOFF_MS};

/// Gateway service configuration. The token value is intentionally absent.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub bot: String,
    pub token_name: Option<String>,
    pub intents: Intents,
    pub shard_id: ShardId,
    pub fanout_capacity: usize,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl GatewayConfig {
    #[must_use]
    pub fn new(bot: impl Into<String>, intents: Intents) -> Self {
        Self {
            bot: bot.into(),
            token_name: None,
            intents,
            shard_id: ShardId::ONE,
            fanout_capacity: FANOUT_CAPACITY,
            initial_backoff: Duration::from_millis(DEFAULT_BACKOFF_MS),
            max_backoff: Duration::from_millis(MAX_BACKOFF_MS),
        }
    }

    #[must_use]
    pub fn token_name(mut self, token_name: impl Into<String>) -> Self {
        self.token_name = Some(token_name.into());
        self
    }

    #[must_use]
    pub const fn shard_id(mut self, shard_id: ShardId) -> Self {
        self.shard_id = shard_id;
        self
    }

    #[must_use]
    pub const fn backoff(mut self, initial: Duration, max: Duration) -> Self {
        self.initial_backoff = initial;
        self.max_backoff = max;
        self
    }
}

/// A redacted token resolved from the host secret store.
pub struct GatewayToken {
    value: String,
}

impl GatewayToken {
    #[must_use]
    pub fn from_mock(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.value
    }
}

impl std::fmt::Debug for GatewayToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GatewayToken(<redacted>)")
    }
}

/// Resolve a bot token through the same env/pass abstraction used by REST.
///
/// The returned token is never logged by this module.
///
/// # Errors
///
/// Returns a redacted error if no matching token entry exists or decrypt fails.
pub fn resolve_gateway_token(
    env: &DiscordEnv,
    config: &GatewayConfig,
) -> Result<GatewayToken, String> {
    let token_name = if let Some(name) = &config.token_name {
        name.clone()
    } else {
        list_pass_tokens(env)
            .into_iter()
            .find(|entry| entry.bot == config.bot)
            .map(|entry| entry.name)
            .ok_or_else(|| format!("no Discord token entry for bot '{}'", config.bot))?
    };
    decrypt_token(&token_name)
        .map(GatewayToken::from_mock)
        .ok_or_else(|| format!("failed to decrypt Discord token entry '{token_name}'"))
}

/// Event delivered to subscribers.
#[derive(Clone, Debug)]
pub struct GatewayEvent {
    pub shard_id: ShardId,
    pub kind: twilight_model::gateway::event::EventType,
    pub event: Event,
}

/// Lifecycle status emitted for observability and tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatewayStatus {
    Starting { shard_id: ShardId },
    Connected { shard_id: ShardId },
    Reconnecting { shard_id: ShardId, attempt: u32 },
    Stopped { shard_id: ShardId },
}

/// Handle for a running gateway service.
pub struct GatewayHandle {
    pub(super) events: broadcast::Sender<GatewayEvent>,
    pub(super) status: watch::Receiver<GatewayStatus>,
    pub(super) shutdown: watch::Sender<bool>,
    pub(super) join: JoinHandle<GatewayRunStats>,
}

impl GatewayHandle {
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<GatewayEvent> {
        self.events.subscribe()
    }

    #[must_use]
    pub fn status(&self) -> watch::Receiver<GatewayStatus> {
        self.status.clone()
    }

    pub async fn shutdown(self) -> GatewayRunStats {
        let _ = self.shutdown.send(true);
        self.join.await.unwrap_or_default()
    }
}

/// Final counters for a service run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GatewayRunStats {
    pub events: u64,
    pub reconnects: u32,
    pub lagged_subscribers: u64,
}
