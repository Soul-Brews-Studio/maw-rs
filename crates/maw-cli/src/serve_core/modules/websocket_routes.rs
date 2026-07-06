use super::ServecoreModuleRegistration;
use crate::serve_core::{
    servecore_mount_ws_registry_with_config, ServecoreLifecycleModule, ServecoreWsKind,
    ServecoreWsRegistry,
};
use axum::Router;
use std::time::Duration;

const WS_CAPTURE_INTERVAL_ENV: &str = "MAW_WS_CAPTURE_INTERVAL_MS";
const WS_CAPTURE_INTERVAL_MIN_MS: u64 = 100;
const WS_CAPTURE_INTERVAL_MAX_MS: u64 = 30_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WsConfig {
    pub idle_timeout: Duration,
    pub heartbeat_interval: Duration,
    pub capture_interval: Duration,
    pub send_timeout: Duration,
    pub max_frame_bytes: usize,
    pub max_connections: usize,
}

impl Default for WsConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(10),
            capture_interval: Duration::from_secs(2),
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
        if let Ok(raw) = std::env::var(WS_CAPTURE_INTERVAL_ENV) {
            if let Some(interval) = ws_parse_capture_interval(&raw) {
                config.capture_interval = interval;
            }
        }
        config
    }
}

fn ws_parse_capture_interval(raw: &str) -> Option<Duration> {
    let millis = raw.trim().parse::<u64>().ok()?;
    if (WS_CAPTURE_INTERVAL_MIN_MS..=WS_CAPTURE_INTERVAL_MAX_MS).contains(&millis) {
        Some(Duration::from_millis(millis))
    } else {
        None
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

    static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn ws_lifecycle_matches_central_module_contract() {
        let module = ws_lifecycle_module();
        assert_eq!(module.name, "ws");
        assert_eq!(module.weight, 80);
    }

    #[test]
    fn ws_capture_interval_env_defaults_to_two_seconds() {
        let _env_lock = ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _guard = EnvGuard::remove(WS_CAPTURE_INTERVAL_ENV);

        assert_eq!(
            WsConfig::ws_from_process_env().capture_interval,
            Duration::from_secs(2)
        );
    }

    #[test]
    fn ws_capture_interval_env_accepts_valid_milliseconds() {
        let _env_lock = ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _guard = EnvGuard::set(WS_CAPTURE_INTERVAL_ENV, "250");

        assert_eq!(
            WsConfig::ws_from_process_env().capture_interval,
            Duration::from_millis(250)
        );
    }

    #[test]
    fn ws_capture_interval_env_keeps_default_when_too_low() {
        let _env_lock = ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _guard = EnvGuard::set(WS_CAPTURE_INTERVAL_ENV, "99");

        assert_eq!(
            WsConfig::ws_from_process_env().capture_interval,
            Duration::from_secs(2)
        );
    }

    #[test]
    fn ws_capture_interval_env_keeps_default_when_too_high() {
        let _env_lock = ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _guard = EnvGuard::set(WS_CAPTURE_INTERVAL_ENV, "30001");

        assert_eq!(
            WsConfig::ws_from_process_env().capture_interval,
            Duration::from_secs(2)
        );
    }

    #[test]
    fn ws_capture_interval_env_keeps_default_when_non_numeric() {
        let _env_lock = ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _guard = EnvGuard::set(WS_CAPTURE_INTERVAL_ENV, "fast");

        assert_eq!(
            WsConfig::ws_from_process_env().capture_interval,
            Duration::from_secs(2)
        );
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
