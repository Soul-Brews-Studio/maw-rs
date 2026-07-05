use super::ServecoreModuleRegistration;
use crate::serve_core::{ServecoreLifecycleModule, ServecoreSharedState, ServecoreTriggerEvent};
use axum::{response::IntoResponse, routing::get, Extension, Json, Router};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[must_use]
pub fn triggers_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "triggers".to_owned(),
        weight: 50,
    }
}

#[must_use]
pub fn triggers_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: triggers_lifecycle_module(),
        mount: triggers_mount,
    }
}

pub fn triggers_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route("/api/triggers", get(triggers_get))
}

async fn triggers_get(Extension(state): Extension<Arc<ServecoreSharedState>>) -> impl IntoResponse {
    let events = state.trigger_bus.servecore_snapshot();
    let triggers = triggers_render(&events);
    Json(json!({"triggers": triggers, "total": triggers.len()}))
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct TriggersReadEntry {
    index: usize,
    on: String,
    repo: Option<String>,
    timeout: Option<i64>,
    action: String,
    name: Option<String>,
    #[serde(rename = "lastFired")]
    last_fired: Option<TriggersLastFired>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct TriggersLastFired {
    ts: String,
    ok: bool,
    action: String,
    error: Option<String>,
}

fn triggers_render(events: &[ServecoreTriggerEvent]) -> Vec<TriggersReadEntry> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| triggers_entry(index, event))
        .collect()
}

fn triggers_entry(index: usize, event: &ServecoreTriggerEvent) -> TriggersReadEntry {
    let payload = triggers_payload(&event.payload);
    let on = triggers_string(&payload, &["on", "event"]).unwrap_or_else(|| event.name.clone());
    let action = triggers_string(&payload, &["action"]).unwrap_or_default();
    let name = triggers_string(&payload, &["name"]).or_else(|| Some(event.name.clone()));
    TriggersReadEntry {
        index,
        on,
        repo: triggers_string(&payload, &["repo"]),
        timeout: triggers_i64(&payload, "timeout"),
        action,
        name,
        last_fired: triggers_last_fired(&payload),
    }
}

fn triggers_payload(payload: &str) -> Value {
    serde_json::from_str(payload).unwrap_or(Value::Null)
}

fn triggers_string(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| payload.get(*key).and_then(Value::as_str))
        .find(|value| triggers_valid_text(value))
        .map(ToOwned::to_owned)
}

fn triggers_i64(payload: &Value, key: &str) -> Option<i64> {
    payload.get(key).and_then(Value::as_i64)
}

fn triggers_last_fired(payload: &Value) -> Option<TriggersLastFired> {
    let last = payload
        .get("lastFired")
        .or_else(|| payload.get("last_fired"))
        .or_else(|| payload.get("result"))
        .unwrap_or(payload);
    let ts = triggers_string(last, &["ts"])?;
    let action = triggers_string(last, &["action"]).unwrap_or_default();
    Some(TriggersLastFired {
        ts,
        ok: last.get("ok").and_then(Value::as_bool).unwrap_or(false),
        action,
        error: triggers_string(last, &["error"]),
    })
}

fn triggers_valid_text(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value != "--"
        && !value.starts_with('-')
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{
        servecore_apply_pipeline, servecore_with_shared_state, ServecoreTriggerBus,
    };
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;

    async fn triggers_spawn(state: ServecoreSharedState) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = servecore_with_shared_state(triggers_mount(Router::new()), state);
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

    fn triggers_event(name: &str, payload: &str) -> ServecoreTriggerEvent {
        ServecoreTriggerEvent {
            name: name.to_owned(),
            payload: payload.to_owned(),
        }
    }

    #[test]
    fn triggers_lifecycle_matches_public_module_contract() {
        let module = triggers_lifecycle_module();
        assert_eq!(module.name, "triggers");
        assert_eq!(module.weight, 50);
    }

    #[test]
    fn triggers_render_maps_config_and_last_fired_history() {
        let events = vec![triggers_event(
            "idle-build",
            r#"{"on":"agent-idle","repo":"nova","timeout":30,"action":"maw hey nova done","name":"idle-build","lastFired":{"ts":"2026-06-25T00:00:00.000Z","ok":true,"action":"maw hey nova done"}}"#,
        )];
        let rows = triggers_render(&events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[0].on, "agent-idle");
        assert_eq!(rows[0].repo.as_deref(), Some("nova"));
        assert_eq!(rows[0].timeout, Some(30));
        assert!(rows[0].last_fired.as_ref().expect("last").ok);
    }

    #[test]
    fn triggers_guard_filters_injected_payload_fields() {
        let events = vec![triggers_event(
            "safe-name",
            r#"{"on":"--","repo":"-bad","action":"bad\nline","name":"-bad","ts":"2026-06-25T00:00:00.000Z"}"#,
        )];
        let rows = triggers_render(&events);
        assert_eq!(rows[0].on, "safe-name");
        assert_eq!(rows[0].repo, None);
        assert_eq!(rows[0].action, "");
        assert_eq!(rows[0].name.as_deref(), Some("safe-name"));
    }

    #[tokio::test]
    async fn triggers_real_wire_is_public_and_uses_fake_bus() {
        let bus = ServecoreTriggerBus::default();
        bus.servecore_fire(triggers_event(
            "idle-build",
            r#"{"event":"agent-idle","repo":"nova","timeout":30,"action":"maw hey nova done","name":"idle-build","result":{"ts":"2026-06-25T00:00:00.000Z","ok":true,"action":"maw hey nova done","error":null}}"#,
        ));
        let state = ServecoreSharedState {
            trigger_bus: bus,
            ..ServecoreSharedState::default()
        };
        let addr = triggers_spawn(state).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/api/triggers"))
            .send()
            .await
            .expect("triggers");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let payload = response.json::<serde_json::Value>().await.expect("json");
        assert_eq!(payload["total"], 1);
        assert_eq!(payload["triggers"][0]["on"], "agent-idle");
        assert_eq!(payload["triggers"][0]["lastFired"]["ok"], true);
    }
}
