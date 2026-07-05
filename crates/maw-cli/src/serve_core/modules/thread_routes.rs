use super::ServecoreModuleRegistration;
use crate::serve_core::{
    ServecoreLifecycleModule, ServecoreSharedState, ServecoreThreadPostResult,
};
use axum::{
    body::{to_bytes, Body},
    extract::{ConnectInfo, Path, Query},
    http::{Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::{net::SocketAddr, sync::Arc};

const THREADSTORE_BODY_LIMIT: usize = 64 * 1024;
const THREADSTORE_LIST_LIMIT: usize = 50;

#[must_use]
pub fn threadstore_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "thread-store".to_owned(),
        weight: 52,
    }
}

#[must_use]
pub fn threadstore_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: threadstore_lifecycle_module(),
        mount: threadstore_mount,
    }
}

pub fn threadstore_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/api/threads", get(threadstore_list))
        .route("/api/thread", post(threadstore_post))
        .route("/api/thread/:id", get(threadstore_read))
        .layer(middleware::from_fn(threadstore_loopback_layer))
}

async fn threadstore_loopback_layer(req: Request<Body>, next: Next) -> Response {
    if !threadstore_is_request(&req) {
        return next.run(req).await;
    }
    let allowed = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|ConnectInfo(addr)| addr.ip().is_loopback());
    if allowed {
        return next.run(req).await;
    }
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error":"forbidden","reason":"loopback only"})),
    )
        .into_response()
}

fn threadstore_is_request(req: &Request<Body>) -> bool {
    let path = req.uri().path();
    path == "/api/threads" || path == "/api/thread" || path.starts_with("/api/thread/")
}

#[derive(Debug, Deserialize)]
struct ThreadStoreListQuery {
    limit: Option<usize>,
}

async fn threadstore_list(
    Query(query): Query<ThreadStoreListQuery>,
    Extension(state): Extension<Arc<ServecoreSharedState>>,
) -> impl IntoResponse {
    match state.thread_store.list() {
        Ok(mut threads) => {
            let limit = query.limit.unwrap_or(THREADSTORE_LIST_LIMIT);
            threads.truncate(limit.min(THREADSTORE_LIST_LIMIT));
            Json(json!({"threads": threads}))
        }
        Err(error) => Json(json!({"error": error})),
    }
}

async fn threadstore_read(
    Path(id): Path<String>,
    Extension(state): Extension<Arc<ServecoreSharedState>>,
) -> Response {
    match id.parse::<u64>() {
        Ok(id) => match state.thread_store.read(id) {
            Ok(record) => Json(record).into_response(),
            Err(error) => (StatusCode::NOT_FOUND, Json(json!({"error": error}))).into_response(),
        },
        Err(_) => threadstore_bad_request("thread id must be numeric"),
    }
}

async fn threadstore_post(
    Extension(state): Extension<Arc<ServecoreSharedState>>,
    req: Request<Body>,
) -> Response {
    if req.method() != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Ok(body) = to_bytes(req.into_body(), THREADSTORE_BODY_LIMIT).await else {
        return threadstore_bad_request("body too large");
    };
    let Ok(payload) = serde_json::from_slice::<ThreadStorePostRequest>(&body) else {
        return threadstore_bad_request("body must be valid json");
    };
    threadstore_post_payload(&state, payload)
}

#[derive(Debug, Deserialize)]
struct ThreadStorePostRequest {
    thread_id: Option<u64>,
    title: Option<String>,
    message: String,
    role: Option<String>,
}

fn threadstore_post_payload(
    state: &ServecoreSharedState,
    payload: ThreadStorePostRequest,
) -> Response {
    let role = payload.role.unwrap_or_else(|| "claude".to_owned());
    let result = if let Some(id) = payload.thread_id {
        state.thread_store.append(id, &role, &payload.message)
    } else {
        let title = payload.title.unwrap_or_else(|| "channel:cli".to_owned());
        state
            .thread_store
            .servecore_post_channel(&title, &role, &payload.message)
            .map(|(result, _)| result)
    };
    threadstore_post_response(result)
}

fn threadstore_post_response(result: Result<ServecoreThreadPostResult, String>) -> Response {
    match result {
        Ok(result) => Json(result).into_response(),
        Err(error) => threadstore_bad_request(&error),
    }
}

fn threadstore_bad_request(error: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{
        modules::servecore_mount_modules, servecore_apply_pipeline, servecore_mount_core_routes,
        servecore_with_shared_state, ServecoreSharedState, ServecoreThreadStore,
    };
    use std::{net::Ipv4Addr, path::PathBuf, time::Duration};
    use tokio::sync::oneshot;

    async fn threadstore_spawn(root: PathBuf) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = ServecoreSharedState::default()
            .servecore_with_thread_store(ServecoreThreadStore::servecore_with_root(root));
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_mount_modules(router, &["thread-store".to_owned()]);
        let router = servecore_with_shared_state(router, state);
        let app = servecore_apply_pipeline(router);
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            server.await.expect("server");
        });
        std::mem::forget(tx);
        addr
    }

    fn threadstore_temp(name: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        root.push(format!(
            "maw-rs-threadstore-{name}-{}-{nanos}",
            std::process::id()
        ));
        root
    }

    #[test]
    fn threadstore_lifecycle_matches_module_contract() {
        let module = threadstore_lifecycle_module();
        assert_eq!(module.name, "thread-store");
        assert_eq!(module.weight, 52);
        assert!(!maw_auth::is_protected("/api/thread", "POST"));
        assert!(!maw_auth::is_protected("/api/threads", "GET"));
    }

    #[tokio::test]
    async fn threadstore_routes_match_maw_js_talk_to_shape() {
        let addr = threadstore_spawn(threadstore_temp("routes")).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let created = client
            .post(format!("http://{addr}/api/thread"))
            .json(&json!({"title":"channel:alpha","message":"hello","role":"claude"}))
            .send()
            .await
            .expect("post");
        assert_eq!(created.status(), StatusCode::OK);
        let body = created.json::<serde_json::Value>().await.expect("json");
        assert_eq!(body["thread_id"], 1);
        assert_eq!(body["message_id"], 1);
        let list = client
            .get(format!("http://{addr}/api/threads?limit=50"))
            .send()
            .await
            .expect("list")
            .json::<serde_json::Value>()
            .await
            .expect("json");
        assert_eq!(list["threads"][0]["title"], "channel:alpha");
    }
}
