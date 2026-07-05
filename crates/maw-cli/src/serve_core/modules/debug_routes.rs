use super::ServecoreModuleRegistration;
use crate::serve_core::ServecoreLifecycleModule;
use axum::{
    extract::State,
    http::header,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

#[must_use]
pub fn debug_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "debug".to_owned(),
        weight: 6,
    }
}

#[must_use]
pub fn debug_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: debug_lifecycle_module(),
        mount: debug_mount,
    }
}

pub fn debug_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .merge(debug_router().with_state(Arc::new(DebugPluginState::default())))
        .route("/api/plugins/reload", post(debug_plugins_reload))
}

fn debug_router() -> Router<Arc<DebugPluginState>> {
    Router::new()
        .route("/api/plugins", get(debug_plugins_json))
        .route("/plugins", get(debug_plugins_html))
}

#[derive(Clone, Default)]
struct DebugPluginState {
    stats: Arc<Vec<DebugPluginStats>>,
}

impl DebugPluginState {
    #[cfg(test)]
    fn debug_with_stats(stats: Vec<DebugPluginStats>) -> Self {
        Self {
            stats: Arc::new(stats),
        }
    }

    fn debug_stats(&self) -> Arc<Vec<DebugPluginStats>> {
        Arc::clone(&self.stats)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DebugPluginStats {
    name: String,
    version: String,
    enabled: bool,
    kind: String,
    api_path: Option<String>,
}

async fn debug_plugins_json(State(state): State<Arc<DebugPluginState>>) -> impl IntoResponse {
    let plugin_stats = state.debug_stats();
    let plugins = debug_plugins_payload(&plugin_stats);
    Json(json!({
        "plugins": plugins,
        "count": plugins.len(),
        "enabled": plugins.iter().filter(|plugin| plugin.enabled).count(),
    }))
}

async fn debug_plugins_html(State(state): State<Arc<DebugPluginState>>) -> impl IntoResponse {
    let plugin_stats = state.debug_stats();
    let plugins = debug_plugins_payload(&plugin_stats);
    let body = debug_render_html(&plugins);
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(body),
    )
}

async fn debug_plugins_reload() -> impl IntoResponse {
    Json(json!({"ok": true, "state": "reload-pending-auth"}))
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct DebugPluginEntry {
    name: String,
    version: String,
    enabled: bool,
    kind: String,
    api_path: Option<String>,
}

fn debug_plugins_payload(stats: &[DebugPluginStats]) -> Vec<DebugPluginEntry> {
    let mut plugins = stats.iter().map(debug_plugin_entry).collect::<Vec<_>>();
    plugins.sort_by(|left, right| left.name.cmp(&right.name));
    plugins
}

fn debug_plugin_entry(stat: &DebugPluginStats) -> DebugPluginEntry {
    DebugPluginEntry {
        name: stat.name.clone(),
        version: stat.version.clone(),
        enabled: stat.enabled,
        kind: stat.kind.clone(),
        api_path: stat.api_path.clone(),
    }
}

fn debug_render_html(plugins: &[DebugPluginEntry]) -> String {
    let rows = plugins
        .iter()
        .map(debug_render_plugin_row)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<!doctype html><html><head><title>maw plugins</title></head><body><h1>Plugins</h1><p>count: {}</p><table>{rows}</table></body></html>",
        plugins.len()
    )
}

fn debug_render_plugin_row(plugin: &DebugPluginEntry) -> String {
    format!(
        "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
        debug_escape_html(&plugin.name),
        debug_escape_html(&plugin.version),
        debug_escape_html(&plugin.kind),
        plugin.enabled
    )
}

fn debug_escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{modules::servecore_mount_modules, servecore_apply_pipeline};
    use axum::http::StatusCode;
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;

    async fn debug_spawn() -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = servecore_mount_modules(Router::new(), &["debug".to_owned()]);
        let app = servecore_apply_pipeline(router);
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("server");
        });
        std::mem::forget(tx);
        addr
    }

    fn debug_stats() -> Vec<DebugPluginStats> {
        vec![DebugPluginStats {
            name: "alpha".to_owned(),
            version: "1.0.0".to_owned(),
            enabled: true,
            kind: "rust".to_owned(),
            api_path: Some("/api/plugins/alpha".to_owned()),
        }]
    }

    #[test]
    fn debug_lifecycle_uses_core_weight_and_name() {
        assert_eq!(debug_lifecycle_module().name, "debug");
        assert_eq!(debug_lifecycle_module().weight, 6);
    }

    #[test]
    fn debug_payload_sorts_and_escapes_html() {
        let state = DebugPluginState::debug_with_stats(debug_stats());
        let payload = debug_plugins_payload(&state.debug_stats());
        assert_eq!(payload[0].name, "alpha");
        assert!(debug_render_html(&payload).contains("alpha"));
        assert_eq!(debug_escape_html("<x&y>"), "&lt;x&amp;y&gt;");
    }

    #[tokio::test]
    async fn debug_public_routes_and_reload_default_deny_are_hermetic() {
        let addr = debug_spawn().await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let api = client
            .get(format!("http://{addr}/api/plugins"))
            .send()
            .await
            .expect("plugins api");
        assert_eq!(api.status(), StatusCode::OK);
        let payload = api.json::<serde_json::Value>().await.expect("json");
        assert_eq!(payload["count"], 0);
        assert_eq!(payload["enabled"], 0);

        let html = client
            .get(format!("http://{addr}/plugins"))
            .send()
            .await
            .expect("plugins html");
        assert_eq!(html.status(), StatusCode::OK);
        assert!(html.text().await.expect("body").contains("Plugins"));

        assert!(maw_auth::is_protected("/api/plugins/reload", "POST"));
        let reload = client
            .post(format!("http://{addr}/api/plugins/reload"))
            .send()
            .await
            .expect("plugins reload");
        assert_eq!(reload.status(), StatusCode::FORBIDDEN);
    }
}
