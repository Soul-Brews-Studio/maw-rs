use super::ServecoreModuleRegistration;
use crate::serve_core::{ServecoreLifecycleModule, ServecoreSharedState};
use axum::{routing::get, Extension, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

#[must_use]
pub fn info_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "info".to_owned(),
        weight: 50,
    }
}

#[must_use]
pub fn info_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: info_lifecycle_module(),
        mount: info_mount,
    }
}

pub fn info_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route("/info", get(info_get))
}

async fn info_get(Extension(state): Extension<Arc<ServecoreSharedState>>) -> Json<Value> {
    Json(info_payload(state.agents_node.as_deref()))
}

fn info_payload(node: Option<&str>) -> Value {
    let node = node
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("local");
    let mut payload = json!({
        "node": node,
        "version": env!("CARGO_PKG_VERSION"),
        "ts": info_now_iso(),
        "maw": {
            "schema": "1",
            "plugins": {"manifestEndpoint": "/api/plugins"},
            "capabilities": ["plugin.listManifest", "peer.handshake", "info"]
        }
    });
    if let Some(user) = info_user() {
        payload["user"] = json!(user);
    }
    payload
}

fn info_user() -> Option<String> {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn info_now_iso() -> String {
    let millis = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    info_iso_millis(millis)
}

fn info_iso_millis(millis: u64) -> String {
    let seconds = millis / 1000;
    let millis_part = millis % 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = info_civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis_part:03}Z")
}

fn info_civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_epoch.saturating_add(719_468);
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_day = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * year_day + 2) / 153;
    let day = year_day - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{
        servecore_apply_pipeline, servecore_with_shared_state, ServecoreSharedState,
    };
    use axum::http::StatusCode;
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;

    async fn info_spawn(state: ServecoreSharedState) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = servecore_with_shared_state(info_mount(Router::new()), state);
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

    #[test]
    fn info_lifecycle_matches_public_module_contract() {
        let module = info_lifecycle_module();
        assert_eq!(module.name, "info");
        assert_eq!(module.weight, 50);
    }

    #[test]
    fn info_payload_matches_maw_js_probe_shape() {
        let payload = info_payload(Some("node-a"));
        assert_eq!(payload["node"], "node-a");
        assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(payload["maw"]["schema"], "1");
        assert_eq!(
            payload["maw"]["plugins"]["manifestEndpoint"],
            "/api/plugins"
        );
        assert_eq!(payload["maw"]["capabilities"][2], "info");
        assert!(payload["ts"].as_str().is_some_and(|ts| ts.ends_with('Z')));
    }

    #[test]
    fn info_iso_millis_is_stable() {
        assert_eq!(info_iso_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            info_iso_millis(1_704_067_200_123),
            "2024-01-01T00:00:00.123Z"
        );
    }

    #[tokio::test]
    async fn info_route_is_public_root_endpoint_for_js_probe() {
        let state = ServecoreSharedState::default()
            .servecore_with_agents_node(Some("rs-node".to_owned()))
            .servecore_with_auth(Some("workspace-secret".to_owned()), None);
        let addr = info_spawn(state).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/info"))
            .send()
            .await
            .expect("info");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<Value>().await.expect("json");
        assert_eq!(payload["node"], "rs-node");
        assert!(payload["maw"].is_object());
    }
}
