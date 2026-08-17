use super::ServecoreModuleRegistration;
use crate::core_impl::ServeidentityIdentityError;
use crate::serve_core::ServecoreLifecycleModule;
use axum::{http::StatusCode, response::IntoResponse, routing::get, Extension, Json, Router};
use serde_json::{json, Value};

#[derive(Clone, Copy)]
struct IdentityProvider {
    payload: fn() -> Result<Value, ServeidentityIdentityError>,
}

#[must_use]
pub fn identity_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "identity".to_owned(),
        weight: 50,
    }
}

#[must_use]
pub fn identity_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: identity_lifecycle_module(),
        mount: identity_mount,
    }
}

pub fn identity_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    identity_mount_with_provider(
        router,
        IdentityProvider {
            payload: crate::core_impl::serveidentity_http_payload_read_only,
        },
    )
}

fn identity_mount_with_provider<S>(router: Router<S>, provider: IdentityProvider) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/api/identity", get(identity_get))
        .layer(Extension(provider))
}

async fn identity_get(Extension(provider): Extension<IdentityProvider>) -> impl IntoResponse {
    match (provider.payload)() {
        Ok(payload) => Json(payload).into_response(),
        // #867: a node that has never run `maw peers add` has no peer-key file yet — that is
        // the expected pre-pairing state, not a server fault. Report it distinctly (409) rather
        // than folding it into the generic 500 below.
        Err(error) if error.is_not_paired() => (
            StatusCode::CONFLICT,
            Json(json!({"error": "not_paired", "reason": error.message()})),
        )
            .into_response(),
        // Any other I/O fault (permissions, disk error, empty/corrupt key, ...) stays a genuine
        // 500 — this fix is about not misrepresenting "not paired yet", not about hiding real
        // faults.
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.message()})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::servecore_apply_pipeline;
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;

    fn identity_fake_payload() -> Result<Value, ServeidentityIdentityError> {
        serde_json::from_str(
            r#"{
                "node":"test@local",
                "host":"local",
                "oracle":"gm-bo",
                "version":"1.2.3",
                "agents":["nova"],
                "uptime":42,
                "clockUtc":"2026-06-25T00:00:00.000Z",
                "endpoints":["/api/identity"],
                "pubkey":"pub-test"
            }"#,
        )
        .map_err(|error| ServeidentityIdentityError::Failed(error.to_string()))
    }

    // #867: stands in for the real peer-key read hitting `ErrorKind::NotFound` — a node that
    // has never run `maw peers add`. Verified against the real filesystem behavior in
    // `core_impl::serve_identity`'s `serveidentity_http_provider_reads_peer_key_without_creating_one`
    // test; this one isolates the HTTP-mapping half of the fix (`identity_get`'s match arms).
    fn identity_fake_payload_not_paired() -> Result<Value, ServeidentityIdentityError> {
        Err(ServeidentityIdentityError::NotPaired)
    }

    // #867: stands in for a genuine, non-NotFound I/O fault (e.g. permission-denied) reading an
    // *existing* peer-key file. Verified against a real chmod'd file in
    // `core_impl::serve_identity`'s
    // `serveidentity_permission_denied_on_existing_peer_key_is_not_mistaken_for_not_paired` test.
    fn identity_fake_payload_io_fault() -> Result<Value, ServeidentityIdentityError> {
        Err(ServeidentityIdentityError::Failed(
            "failed to read peer-key for identity: Permission denied (os error 13)".to_owned(),
        ))
    }

    async fn identity_spawn_with(
        payload: fn() -> Result<Value, ServeidentityIdentityError>,
    ) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = identity_mount_with_provider(Router::new(), IdentityProvider { payload });
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

    async fn identity_spawn() -> std::net::SocketAddr {
        identity_spawn_with(identity_fake_payload).await
    }

    #[test]
    fn identity_lifecycle_matches_public_module_contract() {
        let module = identity_lifecycle_module();
        assert_eq!(module.name, "identity");
        assert_eq!(module.weight, 50);
    }

    #[tokio::test]
    async fn identity_route_is_public_and_returns_pubkey_payload() {
        let addr = identity_spawn().await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/api/identity"))
            .send()
            .await
            .expect("identity");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<Value>().await.expect("json");
        assert_eq!(payload["pubkey"], "pub-test");
        assert_eq!(payload["node"], "test@local");
    }

    // #867: a never-paired node (no peer-key file yet) must not read as a server fault. Before
    // the fix this returned bare 500 `{"error":"failed to read peer-key for identity: No such
    // file or directory (os error 2)"}` — see the issue for the literal repro.
    #[tokio::test]
    async fn identity_route_returns_409_not_paired_instead_of_500_when_never_paired() {
        let addr = identity_spawn_with(identity_fake_payload_not_paired).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/api/identity"))
            .send()
            .await
            .expect("identity");
        let status = response.status();
        let body = response.json::<Value>().await.expect("json");
        eprintln!("identity not-paired response: status={status} body={body}");
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "not_paired");
        assert!(
            body["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("maw peers add")),
            "reason should point at the fix: {body}"
        );
    }

    // #867 regression guard: a genuine, non-NotFound I/O fault (permissions, disk error, ...)
    // reading the peer-key must keep the original 500 — this fix only reclassifies the
    // "haven't paired yet" case, it must not swallow real faults.
    #[tokio::test]
    async fn identity_route_still_500s_on_a_genuine_io_fault() {
        let addr = identity_spawn_with(identity_fake_payload_io_fault).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/api/identity"))
            .send()
            .await
            .expect("identity");
        let status = response.status();
        let body = response.json::<Value>().await.expect("json");
        eprintln!("identity io-fault response: status={status} body={body}");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body["error"],
            "failed to read peer-key for identity: Permission denied (os error 13)"
        );
    }
}
