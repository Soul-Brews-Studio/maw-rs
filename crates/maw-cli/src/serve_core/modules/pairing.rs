use super::ServecoreModuleRegistration;
use crate::serve_core::ServecoreLifecycleModule;
use axum::{
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use maw_auth::{
    generate_pair_code_from_bytes, pair_api_accept_plan, pair_api_generate_plan,
    pair_api_status_plan, PairAcceptInput, PairApiConfig, PairCodeStore,
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct PairServeState {
    store: Arc<Mutex<PairCodeStore>>,
    config: PairApiConfig,
    mint: fn() -> String,
    now_ms: fn() -> u64,
}

#[derive(Deserialize)]
struct PairGenerateBody {
    #[serde(default, rename = "ttlMs")]
    ttl_ms: Option<u64>,
    #[serde(default, rename = "expiresSec")]
    expires_sec: Option<u64>,
}

#[derive(Deserialize)]
struct PairAcceptBody {
    node: String,
    #[serde(default)]
    url: Option<String>,
}

#[must_use]
pub fn pair_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "pair".to_owned(),
        weight: 50,
    }
}

#[must_use]
pub fn pair_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: pair_lifecycle_module(),
        mount: pair_mount,
    }
}

pub fn pair_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    pair_mount_with_state(router, PairServeState::from_env())
}

fn pair_mount_with_state<S>(router: Router<S>, state: PairServeState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/api/pair/generate", post(pair_generate))
        .route("/api/pair/status/:code", get(pair_status))
        .route("/api/pair/:code", post(pair_accept))
        .layer(Extension(state))
}

async fn pair_generate(
    Extension(state): Extension<PairServeState>,
    Json(body): Json<PairGenerateBody>,
) -> impl IntoResponse {
    let code = (state.mint)();
    let mut store = state
        .store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let result = pair_api_generate_plan(
        &mut store,
        &state.config,
        &code,
        body.expires_sec,
        body.ttl_ms,
        (state.now_ms)(),
    );
    (
        StatusCode::CREATED,
        Json(json!({
            "ok": result.ok,
            "code": result.code,
            "expiresAt": result.expires_at,
            "ttlMs": result.ttl_ms,
            "node": result.node,
            "port": result.port,
            "federationToken": null,
        })),
    )
}

async fn pair_status(
    Extension(state): Extension<PairServeState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let store = state
        .store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let result = pair_api_status_plan(&store, &code, (state.now_ms)());
    let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({
            "ok": result.ok,
            "error": result.error,
            "consumed": result.consumed,
            "remoteNode": result.remote_node,
            "remoteUrl": result.remote_url,
        })),
    )
}

async fn pair_accept(
    Extension(state): Extension<PairServeState>,
    Path(code): Path<String>,
    Json(body): Json<PairAcceptBody>,
) -> impl IntoResponse {
    let input = PairAcceptInput {
        node: body.node,
        url: body.url,
    };
    let mut store = state
        .store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let result = pair_api_accept_plan(
        &mut store,
        &state.config,
        &code,
        Some(input),
        (state.now_ms)(),
    );
    let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({
            "ok": result.ok,
            "error": result.error,
            "node": result.node,
            "oracle": result.oracle,
            "url": result.url,
            "federationToken": result.federation_token,
        })),
    )
}

impl PairServeState {
    fn from_env() -> Self {
        Self {
            store: Arc::new(Mutex::new(PairCodeStore::default())),
            config: pair_config_from_env(),
            mint: pair_secure_code,
            now_ms: pair_now_ms,
        }
    }
}

fn pair_config_from_env() -> PairApiConfig {
    // #734: env wins, then the merged maw.config.json (the same source
    // `maw hey`/`maw peers` read), then a default. Previously this read
    // MAW_NODE/MAW_PORT/MAW_ORACLE env ONLY, so a serve started without those env
    // vars advertised node "local" / port 3456 regardless of config — and a pair
    // handshake echoed those defaults into both peers' stores, so a successful
    // pair silently pointed each side at localhost:<default> (itself).
    let merged = maw_xdg::load_merged_config(&pair_real_xdg_env()).config;
    let cfg_str = |key: &str| merged.get(key).and_then(serde_json::Value::as_str).filter(|v| !v.is_empty());
    let cfg_port = merged.get("port").and_then(|v| {
        v.as_u64().and_then(|n| u16::try_from(n).ok()).or_else(|| v.as_str().and_then(|s| s.parse::<u16>().ok()))
    });
    let node = std::env::var("MAW_NODE")
        .ok()
        .or_else(|| cfg_str("node").map(ToOwned::to_owned))
        .unwrap_or_else(|| "local".to_owned());
    let port = std::env::var("MAW_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .or(cfg_port)
        .unwrap_or(3456);
    let oracle = std::env::var("MAW_ORACLE")
        .ok()
        .or_else(|| cfg_str("oracle").map(ToOwned::to_owned))
        .unwrap_or_else(|| "mawjs".to_owned());
    // #734: shared resolver with the client — MAW_BASE_URL > a real config host
    // > localhost. The server must surface the localhost-fallback warning (the
    // resolver returns it for exactly this case), not swallow it via `.0` —
    // otherwise a serve with no MAW_BASE_URL and no real config host advertises
    // localhost silently, the original self-pair trap. Log it once at startup.
    let (base_url, base_url_warning) = maw_xdg::advertised_base_url(&pair_real_xdg_env(), port);
    if let Some(warning) = base_url_warning {
        eprintln!("maw serve: pair {warning}");
    }
    PairApiConfig {
        node,
        oracle,
        port,
        base_url,
        federation_token: std::env::var("MAW_FEDERATION_TOKEN").unwrap_or_default(),
        pubkey: std::env::var("MAW_PUBKEY").unwrap_or_default(),
    }
}

/// Real-process XDG env for reading the merged maw config (mirrors `core_impl`'s
/// `real_xdg_env`, which `serve_core` cannot reach because `core_impl` is private).
fn pair_real_xdg_env() -> maw_xdg::MawXdgEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = [
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_DATA_DIR",
        "MAW_STATE_DIR",
        "MAW_CACHE_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "MAW_TEST_MODE",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok().map(|value| (name.to_owned(), value)));
    maw_xdg::MawXdgEnv::with_vars(home, vars)
}

fn pair_secure_code() -> String {
    let mut bytes = [0_u8; 6];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    generate_pair_code_from_bytes(&bytes)
}

fn pair_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;

    fn pair_test_state() -> PairServeState {
        PairServeState {
            store: Arc::new(Mutex::new(PairCodeStore::default())),
            config: PairApiConfig {
                node: "node-a".to_owned(),
                oracle: "oracle-a".to_owned(),
                port: 5002,
                base_url: "http://node-a:5002".to_owned(),
                federation_token: "secret-test-token".to_owned(),
                pubkey: "pub-test".to_owned(),
            },
            mint: || "W4K7F3".to_owned(),
            now_ms: || 1_700_000_000_000,
        }
    }

    async fn pair_spawn() -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = crate::serve_core::servecore_apply_pipeline(pair_mount_with_state(
            Router::new(),
            pair_test_state(),
        ));
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await
                .expect("server");
        });
        std::mem::forget(tx);
        addr
    }

    #[test]
    fn pair_lifecycle_matches_module_contract() {
        let module = pair_lifecycle_module();
        assert_eq!(module.name, "pair");
        assert_eq!(module.weight, 50);
    }

    #[test]
    fn pair_secure_code_uses_valid_shape() {
        let code = pair_secure_code();
        assert!(maw_auth::is_valid_pair_code_shape(&code));
    }

    #[tokio::test]
    async fn pair_routes_mint_status_accept_without_token_in_generate_or_status() {
        let addr = pair_spawn().await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let generated: serde_json::Value = client
            .post(format!("http://{addr}/api/pair/generate"))
            .json(&json!({"ttlMs": 60_000}))
            .send()
            .await
            .expect("generate")
            .json()
            .await
            .expect("generate json");
        assert_eq!(generated["code"], "W4K-7F3");
        assert!(generated["federationToken"].is_null());
        let status: serde_json::Value = client
            .get(format!("http://{addr}/api/pair/status/W4K7F3"))
            .send()
            .await
            .expect("status")
            .json()
            .await
            .expect("status json");
        assert_eq!(status["consumed"], false);
        assert!(!format!("{status}").contains("secret-test-token"));
        let accepted: serde_json::Value = client
            .post(format!("http://{addr}/api/pair/W4K7F3"))
            .json(&json!({"node": "remote", "url": "http://remote:5002"}))
            .send()
            .await
            .expect("accept")
            .json()
            .await
            .expect("accept json");
        assert_eq!(accepted["node"], "node-a");
        assert_eq!(accepted["federationToken"], "secret-test-token");
        let consumed: serde_json::Value = client
            .get(format!("http://{addr}/api/pair/status/W4K7F3"))
            .send()
            .await
            .expect("consumed")
            .json()
            .await
            .expect("consumed json");
        assert_eq!(consumed["consumed"], true);
        assert_eq!(consumed["remoteNode"], "remote");
    }
}
