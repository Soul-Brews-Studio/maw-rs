use super::ServecoreModuleRegistration;
use crate::serve_core::{ServecoreLifecycleModule, ServecoreSharedState, ServecoreTriggerEvent};
use axum::{
    body::{to_bytes, Body},
    http::{Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::sync::Arc;

#[must_use]
pub fn triggersmutate_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "triggers-mutate".to_owned(),
        weight: 51,
    }
}

#[must_use]
pub fn triggersmutate_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: triggersmutate_lifecycle_module(),
        mount: triggersmutate_mount,
    }
}

pub fn triggersmutate_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(middleware::from_fn(triggersmutate_fire_layer))
}

async fn triggersmutate_fire_layer(req: Request<Body>, next: Next) -> Response {
    if !triggersmutate_is_fire_request(&req) {
        return next.run(req).await;
    }
    let state = req.extensions().get::<Arc<ServecoreSharedState>>().cloned();
    let Some(state) = state else {
        return triggersmutate_bad_request("missing serve state");
    };
    let Ok(body) = to_bytes(req.into_body(), 64 * 1024).await else {
        return triggersmutate_bad_request("body must be valid json");
    };
    let Ok(body) = serde_json::from_slice::<Value>(&body) else {
        return triggersmutate_bad_request("body must be valid json");
    };
    match triggersmutate_request(&body) {
        Ok(request) => {
            let event = triggersmutate_event(&request);
            state.trigger_bus.servecore_fire(event);
            Json(json!({"ok": true, "results": [triggersmutate_result(&request)]})).into_response()
        }
        Err(error) => triggersmutate_bad_request(error),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TriggersMutateRequest {
    event: String,
    context: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct TriggersMutateResult {
    event: String,
    fired: bool,
    context: Map<String, Value>,
}

fn triggersmutate_is_fire_request(req: &Request<Body>) -> bool {
    req.method() == Method::POST && req.uri().path() == "/api/triggers/fire"
}

fn triggersmutate_request(body: &Value) -> Result<TriggersMutateRequest, &'static str> {
    let object = body.as_object().ok_or("body must be an object")?;
    let event = object
        .get("event")
        .and_then(Value::as_str)
        .filter(|value| triggersmutate_valid_text(value))
        .ok_or("event must be a safe string")?
        .to_owned();
    let context = triggersmutate_context(object.get("context"))?;
    Ok(TriggersMutateRequest { event, context })
}

fn triggersmutate_context(value: Option<&Value>) -> Result<Map<String, Value>, &'static str> {
    let Some(value) = value else {
        return Ok(Map::new());
    };
    let object = value.as_object().ok_or("context must be an object")?;
    if object.iter().all(|(key, value)| {
        triggersmutate_valid_text(key) && value.as_str().is_some_and(triggersmutate_valid_text)
    }) {
        Ok(object.clone())
    } else {
        Err("context must be a flat string map")
    }
}

fn triggersmutate_valid_text(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value != "--"
        && !value.starts_with('-')
        && !value.chars().any(char::is_control)
}

fn triggersmutate_event(request: &TriggersMutateRequest) -> ServecoreTriggerEvent {
    ServecoreTriggerEvent {
        name: request.event.clone(),
        payload: json!({"event": request.event, "context": request.context}).to_string(),
    }
}

fn triggersmutate_result(request: &TriggersMutateRequest) -> TriggersMutateResult {
    TriggersMutateResult {
        event: request.event.clone(),
        fired: true,
        context: request.context.clone(),
    }
}

fn triggersmutate_bad_request(error: &'static str) -> axum::response::Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{
        modules::servecore_mount_modules, servecore_apply_pipeline, servecore_mount_core_routes,
        servecore_with_shared_state, ServecoreSharedState,
    };
    use axum::http::StatusCode;
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;

    async fn triggersmutate_spawn(apply_pipeline: bool) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = ServecoreSharedState::default();
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_mount_modules(router, &["triggers-mutate".to_owned()]);
        let router = servecore_with_shared_state(router, state);
        let app = if apply_pipeline {
            servecore_apply_pipeline(router)
        } else {
            router
        };
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
    fn triggersmutate_lifecycle_matches_mutating_module_contract() {
        let module = triggersmutate_lifecycle_module();
        assert_eq!(module.name, "triggers-mutate");
        assert_eq!(module.weight, 51);
    }

    #[test]
    fn triggersmutate_validates_event_and_flat_context() {
        let request = triggersmutate_request(&json!({
            "event": "agent-idle",
            "context": {"repo": "maw-rs"}
        }))
        .expect("request");
        assert_eq!(request.event, "agent-idle");
        assert_eq!(request.context["repo"], "maw-rs");
        assert!(triggersmutate_request(&json!({"event": "-bad"})).is_err());
        assert!(triggersmutate_request(&json!({"event": "ok", "context": {"repo": 7}})).is_err());
        assert!(triggersmutate_request(&json!({"event": "ok", "context": {"--": "bad"}})).is_err());
    }

    #[tokio::test]
    async fn triggersmutate_fire_is_protected_by_default_deny() {
        let addr = triggersmutate_spawn(true).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        assert!(maw_auth::is_protected("/api/triggers/fire", "POST"));
        assert!(maw_auth::is_protected("/triggers/fire", "POST"));
        let response = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .json(&json!({"event": "agent-idle", "context": {"repo": "maw-rs"}}))
            .send()
            .await
            .expect("fire");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn triggersmutate_bad_body_returns_400_before_hook() {
        let addr = triggersmutate_spawn(false).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .json(&json!({"event": "ok", "context": {"repo": 7}}))
            .send()
            .await
            .expect("bad body");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn triggersmutate_fire_records_on_fake_bus_without_pipeline() {
        let state = ServecoreSharedState::default();
        let bus = state.trigger_bus.clone();
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_with_shared_state(triggersmutate_mount(router), state);
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("server");
        });
        std::mem::forget(tx);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .json(&json!({"event": "agent-idle", "context": {"repo": "maw-rs"}}))
            .send()
            .await
            .expect("fire");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(bus.servecore_snapshot()[0].name, "agent-idle");
    }
}
