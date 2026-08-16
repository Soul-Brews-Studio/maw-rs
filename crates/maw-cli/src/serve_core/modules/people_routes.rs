use super::ServecoreModuleRegistration;
use crate::serve_core::{ServecoreLifecycleModule, ServecoreSharedState};
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};
type PeopleDedupeEntries = Vec<((String, String), Instant)>;
static PEOPLE_DEDUPE: LazyLock<Mutex<PeopleDedupeEntries>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
#[must_use]
pub fn people_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "people".to_owned(),
        weight: 52,
    }
}
#[must_use]
pub fn people_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: people_lifecycle_module(),
        mount: people_mount,
    }
}
pub fn people_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/api/people/analyze", post(people_analyze))
        .layer(middleware::from_fn(people_loopback_layer))
}
async fn people_loopback_layer(req: Request<Body>, next: Next) -> Response {
    if req.uri().path() != "/api/people/analyze" {
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
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeopleAnalyzeRequest {
    intent: String,
    thread_id: String,
    oracle: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct PeopleIntent {
    thread_id: String,
    oracle: String,
    target: String,
}
async fn people_analyze(
    Extension(state): Extension<Arc<ServecoreSharedState>>,
    req: Request<Body>,
) -> Response {
    people_analyze_with_delivery(state, req, crate::deliver_people_analyze_intent).await
}

async fn people_analyze_with_delivery(
    state: Arc<ServecoreSharedState>,
    req: Request<Body>,
    deliver: impl FnOnce(&str, &str) -> Result<(), String>,
) -> Response {
    let Ok(body) = to_bytes(req.into_body(), 16 * 1024).await else {
        return people_bad_request("body too large");
    };
    let payload = match serde_json::from_slice::<PeopleAnalyzeRequest>(&body) {
        Ok(payload) => payload,
        Err(error) => return people_bad_request(&format!("body must match contract: {error}")),
    };
    let intent = match people_intent_from_request(&state, payload) {
        Ok(intent) => intent,
        Err(error) => return people_bad_request(&error),
    };
    if !people_dedupe_accept(&intent) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"ok":false,"error":"duplicate","reason":"duplicate request within dedupe window"})),
        )
            .into_response();
    }
    if let Err(error) = people_deliver(&intent, deliver) {
        return people_delivery_failed(&error);
    }
    Json(json!({"ok":true,"status":"accepted","intent":intent})).into_response()
}

const PEOPLE_ANALYZE_TEMPLATE: &str =
    "analyze thread {thread_id}: run People core conversation-scoped analysis";

fn people_delivery_text(thread_id: &str) -> String {
    PEOPLE_ANALYZE_TEMPLATE.replace("{thread_id}", thread_id)
}

fn people_deliver(
    intent: &PeopleIntent,
    deliver: impl FnOnce(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    let result = deliver(&intent.target, &people_delivery_text(&intent.thread_id));
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    eprintln!(
        "people_analyze_delivery thread_id={} target={} timestamp={timestamp} outcome={}",
        intent.thread_id,
        intent.target,
        if result.is_ok() { "success" } else { "failure" },
    );
    result
}

fn people_delivery_failed(reason: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"ok":false,"error":"delivery_failed","reason":reason})),
    )
        .into_response()
}

fn people_intent_from_request(
    state: &ServecoreSharedState,
    request: PeopleAnalyzeRequest,
) -> Result<PeopleIntent, String> {
    if request.intent != "analyze_thread" {
        return Err("intent must equal analyze_thread".to_owned());
    }
    if request.thread_id.is_empty() || !request.thread_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("thread_id must be ASCII digits only".to_owned());
    }
    Ok(PeopleIntent {
        target: people_resolve_oracle(state, &request.oracle)?,
        thread_id: request.thread_id,
        oracle: request.oracle,
    })
}
fn people_resolve_oracle(state: &ServecoreSharedState, oracle: &str) -> Result<String, String> {
    let needle = people_normalize_oracle(oracle);
    state
        .servecore_agents_panes()
        .into_iter()
        .find(|pane| {
            pane.target.to_ascii_lowercase().contains("oracle")
                && people_normalize_oracle(&pane.target) == needle
        })
        .map(|pane| pane.target)
        .ok_or_else(|| format!("oracle '{oracle}' is not live"))
}

fn people_normalize_oracle(value: &str) -> String {
    let value = value.trim();
    let value = value.split_once(':').map_or(value, |(_, rest)| {
        rest.rsplit_once('.').map_or(rest, |(window, _)| window)
    });
    let value = value.to_ascii_lowercase();
    let value = value.strip_suffix("-oracle").unwrap_or(&value);
    value
        .split_once('-')
        .filter(|(prefix, suffix)| {
            !suffix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map_or_else(|| value.to_owned(), |(_, suffix)| suffix.to_owned())
}

fn people_dedupe_accept(intent: &PeopleIntent) -> bool {
    let now = Instant::now();
    let mut guard = PEOPLE_DEDUPE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.retain(|(_, seen)| now.duration_since(*seen) < Duration::from_secs(5));
    let key = (intent.thread_id.clone(), intent.oracle.clone());
    if guard.iter().any(|(seen, _)| seen == &key) {
        return false;
    }
    guard.push((key, now));
    true
}

fn people_bad_request(reason: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"ok":false,"error":"bad_request","reason":reason})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{servecore_with_shared_state, ServecoreAgentPane};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    fn req(intent: &str, thread_id: &str, oracle: &str) -> PeopleAnalyzeRequest {
        PeopleAnalyzeRequest {
            intent: intent.to_owned(),
            thread_id: thread_id.to_owned(),
            oracle: oracle.to_owned(),
        }
    }

    fn state() -> ServecoreSharedState {
        ServecoreSharedState::default().servecore_with_agents_snapshot(vec![ServecoreAgentPane {
            id: "%7".to_owned(),
            command: "2.1.219".to_owned(),
            target: "17-people:people-oracle.0".to_owned(),
            title: "people-oracle".to_owned(),
            cwd: None,
            pid: Some(7),
            last_activity: None,
        }])
    }

    fn analyze_request(thread_id: &str, oracle: &str) -> Request<Body> {
        Request::post("/api/people/analyze")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"intent":"analyze_thread","thread_id":thread_id,"oracle":oracle})
                    .to_string(),
            ))
            .expect("request")
    }

    /// Serializes the tests that share `PEOPLE_DEDUPE` (#824).
    ///
    /// `PEOPLE_DEDUPE` is a process-global `LazyLock<Mutex<_>>` and four tests
    /// mutate it. `reset_dedupe()` clears it but holds nothing, so a sibling
    /// resetting mid-flight wipes the entry this test just recorded — the
    /// second request then sees an empty store, returns OK instead of
    /// CONFLICT, and delivers twice. That is why it passes alone and fails in
    /// the full suite, and why the #757 env lock did not help: this is a
    /// different global.
    /// A tokio mutex, not a std one: three of the four callers are
    /// `#[tokio::test]` and hold the guard across `.await`. A std `MutexGuard`
    /// is not `Send` and pins the runtime thread if held there —
    /// `clippy::await_holding_lock` flags exactly this, and on a multi-threaded
    /// runtime it deadlocks rather than merely warning.
    static DEDUPE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn reset_dedupe() -> tokio::sync::MutexGuard<'static, ()> {
        let guard = DEDUPE_TEST_LOCK.lock().await;
        PEOPLE_DEDUPE.lock().expect("dedupe lock").clear();
        guard
    }

    #[test]
    fn people_contract_validation_dedupe_and_typed_intent() {
        let state = state();
        assert!(serde_json::from_str::<PeopleAnalyzeRequest>(
            r#"{"intent":"analyze_thread","thread_id":"1","oracle":"people","extra":true}"#,
        )
        .is_err());
        assert_eq!(
            people_intent_from_request(&state, req("talk", "1", "people")).unwrap_err(),
            "intent must equal analyze_thread"
        );
        assert_eq!(
            people_intent_from_request(&state, req("analyze_thread", "1x", "people")).unwrap_err(),
            "thread_id must be ASCII digits only"
        );
        let err =
            people_intent_from_request(&state, req("analyze_thread", "1", "missing")).unwrap_err();
        assert!(err.contains("is not live"));
        let dupe = people_intent_from_request(&state, req("analyze_thread", "765", "people"))
            .expect("intent");
        assert!(people_dedupe_accept(&dupe) && !people_dedupe_accept(&dupe));
        let intent = people_intent_from_request(&state, req("analyze_thread", "123", "people"))
            .expect("intent");
        assert_eq!(intent.thread_id, "123");
        assert_eq!(intent.oracle, "people");
        assert_eq!(intent.target, "17-people:people-oracle.0");
    }

    #[tokio::test]
    async fn people_analyze_rejects_non_loopback_origin() {
        let app = servecore_with_shared_state(people_mount(Router::new()), state());
        let mut req = Request::post("/api/people/analyze")
            .body(Body::empty())
            .expect("request");
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([198, 51, 100, 10], 49_152))));
        let response = app.oneshot(req).await.expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn people_analyze_delivers_one_fixed_intent_to_resolved_target() {
        let _dedupe_guard = reset_dedupe().await;
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&delivered);
        let response = people_analyze_with_delivery(
            Arc::new(state()),
            analyze_request("768", "people"),
            move |target, text| {
                captured
                    .lock()
                    .expect("delivery lock")
                    .push((target.to_owned(), text.to_owned()));
                Ok(())
            },
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            *delivered.lock().expect("delivery lock"),
            vec![(
                "17-people:people-oracle.0".to_owned(),
                "analyze thread 768: run People core conversation-scoped analysis".to_owned(),
            )]
        );
    }

    #[tokio::test]
    async fn people_analyze_dedupe_prevents_second_delivery() {
        let _dedupe_guard = reset_dedupe().await;
        let deliveries = Arc::new(AtomicUsize::new(0));
        for expected in [StatusCode::OK, StatusCode::CONFLICT] {
            let deliveries = Arc::clone(&deliveries);
            let response = people_analyze_with_delivery(
                Arc::new(state()),
                analyze_request("769", "people"),
                move |_, _| {
                    deliveries.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await;
            assert_eq!(response.status(), expected);
        }
        assert_eq!(deliveries.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn people_analyze_reports_dead_target_delivery_failure() {
        let _dedupe_guard = reset_dedupe().await;
        let response = people_analyze_with_delivery(
            Arc::new(state()),
            analyze_request("770", "people"),
            |_, _| Err("target pane no longer exists".to_owned()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn people_delivery_call_receives_only_fixed_template_and_thread_id() {
        let intent = PeopleIntent {
            thread_id: "771".to_owned(),
            oracle: "$(not-delivered)".to_owned(),
            target: "17-people:people-oracle.0".to_owned(),
        };
        let mut delivered = None;
        people_deliver(&intent, |target, text| {
            delivered = Some((target.to_owned(), text.to_owned()));
            Ok(())
        })
        .expect("delivery");
        assert_eq!(
            delivered,
            Some((
                "17-people:people-oracle.0".to_owned(),
                "analyze thread 771: run People core conversation-scoped analysis".to_owned(),
            ))
        );
    }
}
