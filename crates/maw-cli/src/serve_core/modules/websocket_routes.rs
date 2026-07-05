use super::ServecoreModuleRegistration;
use crate::serve_core::{
    servecore_mount_ws_registry_with_config, ServecoreLifecycleModule, ServecoreWsKind,
    ServecoreWsRegistry,
};
use axum::Router;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WsConfig {
    pub idle_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub send_timeout: Duration,
    pub max_frame_bytes: usize,
    pub max_connections: usize,
}

impl Default for WsConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(10),
            send_timeout: Duration::from_secs(2),
            max_frame_bytes: 64 * 1024,
            max_connections: 128,
        }
    }
}

impl WsConfig {
    #[must_use]
    pub fn ws_from_process_env() -> Self {
        let mut config = Self::default();
        if let Ok(raw) = std::env::var("MAW_WS_IDLE_SEC") {
            if let Ok(seconds) = raw.parse::<u64>() {
                if (1..=3600).contains(&seconds) {
                    config.idle_timeout = Duration::from_secs(seconds);
                }
            }
        }
        config
    }
}

#[must_use]
pub fn ws_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "ws".to_owned(),
        weight: 80,
    }
}

#[must_use]
pub fn ws_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: ws_lifecycle_module(),
        mount: ws_mount,
    }
}

pub fn ws_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    ws_mount_with_config(router, WsConfig::ws_from_process_env())
}

pub fn ws_mount_with_config<S>(router: Router<S>, config: WsConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    servecore_mount_ws_registry_with_config(router, &ws_registry(), config)
}

fn ws_registry() -> ServecoreWsRegistry {
    let mut registry = ServecoreWsRegistry::default();
    registry
        .servecore_register_ws_kind("/ws/pty", ServecoreWsKind::Pty)
        .expect("pty ws route");
    registry
        .servecore_register_ws_kind("/ws/tmux", ServecoreWsKind::Tmux)
        .expect("tmux ws route");
    registry
}

/// Validates an optional tmux/pty target before any transport spawn/attach work.
///
/// # Errors
///
/// Returns an error when the target has shell/tmux flag or control-character shapes.
pub fn ws_validate_target(target: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(target) = target else {
        return Ok(None);
    };
    if ws_valid_target(target) {
        Ok(Some(target.to_owned()))
    } else {
        Err("target must be a safe tmux target")
    }
}

fn ws_valid_target(target: &str) -> bool {
    !target.is_empty()
        && target.len() <= 128
        && target.trim() == target
        && target != "--"
        && !target.starts_with('-')
        && !target.chars().any(char::is_control)
        && target
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_lifecycle_matches_central_module_contract() {
        let module = ws_lifecycle_module();
        assert_eq!(module.name, "ws");
        assert_eq!(module.weight, 80);
    }

    #[test]
    fn ws_validate_target_rejects_injection_shapes() {
        assert_eq!(
            ws_validate_target(Some("nova:1.0")).unwrap().as_deref(),
            Some("nova:1.0")
        );
        assert!(ws_validate_target(None).unwrap().is_none());
        assert!(ws_validate_target(Some("-bad")).is_err());
        assert!(ws_validate_target(Some("--")).is_err());
        assert!(ws_validate_target(Some("bad\nname")).is_err());
        assert!(ws_validate_target(Some("bad;name")).is_err());
    }

    #[test]
    fn ws_registry_owns_non_ui_websocket_routes() {
        let registry = ws_registry();
        assert_eq!(registry.servecore_paths(), vec!["/ws/pty", "/ws/tmux"]);
        assert_eq!(
            registry.servecore_handlers(),
            vec![
                ("/ws/pty".to_owned(), ServecoreWsKind::Pty),
                ("/ws/tmux".to_owned(), ServecoreWsKind::Tmux),
            ]
        );
    }
}
