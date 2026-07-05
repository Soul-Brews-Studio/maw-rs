use super::ServecoreModuleRegistration;
use crate::serve_core::ServecoreLifecycleModule;
use axum::{
    extract::Query, http::StatusCode, response::IntoResponse, routing::get, Extension, Json, Router,
};
use maw_transport::FederationStatus;
use serde::Serialize;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

const FEDERATION_DEFAULT_LIMIT: usize = 50;

#[must_use]
pub fn federation_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "federation".to_owned(),
        weight: 50,
    }
}

#[must_use]
pub fn federation_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: federation_lifecycle_module(),
        mount: federation_mount,
    }
}

pub fn federation_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    federation_mount_with_state(router, federation_default_state())
}

fn federation_mount_with_state<S>(router: Router<S>, state: FederationState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/api/federation/status", get(federation_status_get))
        .route("/api/peers/discoveries", get(federation_discoveries_get))
        .route("/api/peers/discovered", get(federation_discoveries_get))
        .layer(Extension(Arc::new(state)))
}

async fn federation_status_get(
    Extension(state): Extension<Arc<FederationState>>,
) -> impl IntoResponse {
    Json(federation_status_payload(&state.status)).into_response()
}

async fn federation_discoveries_get(
    Extension(state): Extension<Arc<FederationState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> impl IntoResponse {
    match federation_parse_query(&query) {
        Ok(options) => {
            let peers =
                federation_render_discoveries(&state.discoveries, options, federation_now_millis());
            Json(peers).into_response()
        }
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": message})),
        )
            .into_response(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FederationState {
    status: FederationStatus,
    discoveries: Vec<FederationDiscoveredPeer>,
}

fn federation_default_state() -> FederationState {
    FederationState {
        status: FederationStatus {
            local_url: String::new(),
            peers: Vec::new(),
        },
        discoveries: Vec::new(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FederationDiscoveredPeer {
    zid: String,
    node: String,
    oracle: String,
    host: String,
    locators: Vec<String>,
    capabilities: Vec<String>,
    oracles: Vec<String>,
    last_seen: u64,
    paired: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FederationQuery {
    all: bool,
    limit: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct FederationStatusPayload {
    local_url: String,
    peers: Vec<FederationStatusPeer>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct FederationStatusPeer {
    url: String,
    node: Option<String>,
    reachable: bool,
    latency: Option<u64>,
    agents: Vec<String>,
    clock_warning: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct FederationDiscoveryResponse {
    ok: bool,
    total: usize,
    shown: usize,
    filtered: bool,
    peers: Vec<FederationDiscoveryRow>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct FederationDiscoveryRow {
    zid: String,
    node: String,
    oracle: String,
    host: String,
    locators: Vec<String>,
    capabilities: Vec<String>,
    oracles: Vec<String>,
    #[serde(rename = "firstSeen")]
    first_seen: String,
    #[serde(rename = "lastSeen")]
    last_seen: String,
    #[serde(rename = "seenRel")]
    seen_rel: String,
    paired: bool,
}

fn federation_status_payload(status: &FederationStatus) -> FederationStatusPayload {
    FederationStatusPayload {
        local_url: status.local_url.clone(),
        peers: status
            .peers
            .iter()
            .map(|peer| FederationStatusPeer {
                url: peer.url.clone(),
                node: peer.node.clone(),
                reachable: peer.reachable,
                latency: peer.latency,
                agents: peer.agents.clone(),
                clock_warning: peer.clock_warning,
            })
            .collect(),
    }
}

fn federation_parse_query(query: &BTreeMap<String, String>) -> Result<FederationQuery, String> {
    let mut all = false;
    let mut limit = FEDERATION_DEFAULT_LIMIT;
    for (key, value) in query {
        federation_guard_query_part(key, "query key")?;
        federation_guard_query_part(value, key)?;
        match key.as_str() {
            "all" => all = federation_parse_bool(value)?,
            "limit" => limit = federation_parse_limit(value)?,
            other => return Err(format!("serve-federation: unknown query parameter {other}")),
        }
    }
    Ok(FederationQuery { all, limit })
}

fn federation_parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err("serve-federation: all must be boolean".to_owned()),
    }
}

fn federation_parse_limit(value: &str) -> Result<usize, String> {
    let limit = value
        .parse::<usize>()
        .map_err(|_| "serve-federation: limit must be a positive number".to_owned())?;
    if limit == 0 {
        return Err("serve-federation: limit must be a positive number".to_owned());
    }
    Ok(limit.min(FEDERATION_DEFAULT_LIMIT))
}

fn federation_guard_query_part(value: &str, label: &str) -> Result<(), String> {
    if value == "--" || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("serve-federation: {label} is not allowed"));
    }
    Ok(())
}

fn federation_render_discoveries(
    peers: &[FederationDiscoveredPeer],
    options: FederationQuery,
    now: u64,
) -> FederationDiscoveryResponse {
    let mut filtered = peers
        .iter()
        .filter(|peer| options.all || !peer.paired)
        .cloned()
        .collect::<Vec<_>>();
    filtered.sort_by(|left, right| {
        right
            .last_seen
            .cmp(&left.last_seen)
            .then(left.node.cmp(&right.node))
    });
    let shown = filtered
        .iter()
        .take(options.limit)
        .map(|peer| federation_discovery_row(peer, now))
        .collect::<Vec<_>>();
    FederationDiscoveryResponse {
        ok: true,
        total: filtered.len(),
        shown: shown.len(),
        filtered: !options.all,
        peers: shown,
    }
}

fn federation_discovery_row(peer: &FederationDiscoveredPeer, now: u64) -> FederationDiscoveryRow {
    let seen = federation_iso_millis(peer.last_seen);
    FederationDiscoveryRow {
        zid: peer.zid.clone(),
        node: peer.node.clone(),
        oracle: peer.oracle.clone(),
        host: peer.host.clone(),
        locators: peer.locators.clone(),
        capabilities: peer.capabilities.clone(),
        oracles: peer.oracles.clone(),
        first_seen: seen.clone(),
        last_seen: seen,
        seen_rel: federation_relative_seen(now.saturating_sub(peer.last_seen)),
        paired: peer.paired,
    }
}

fn federation_now_millis() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

fn federation_iso_millis(millis: u64) -> String {
    let seconds = millis / 1000;
    let millis_part = millis % 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = federation_civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis_part:03}Z")
}

fn federation_civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_epoch.saturating_add(719_468);
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

fn federation_relative_seen(delta_ms: u64) -> String {
    let seconds = delta_ms / 1000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    format!("{}d", hours / 24)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::servecore_apply_pipeline;
    use maw_transport::{FederationPeerStatus, FederationStatus};
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;

    async fn federation_spawn(state: FederationState) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = federation_mount_with_state(Router::new(), state);
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

    fn federation_peer(node: &str, last_seen: u64, paired: bool) -> FederationDiscoveredPeer {
        FederationDiscoveredPeer {
            zid: format!("zid-{node}"),
            node: node.to_owned(),
            oracle: format!("{node}-oracle"),
            host: format!("{node}.local"),
            locators: vec![format!("http://{node}.local:3456")],
            capabilities: vec!["feed".to_owned()],
            oracles: vec![format!("{node}:claude")],
            last_seen,
            paired,
        }
    }

    #[test]
    fn federation_query_guards_and_caps_limit() {
        let mut query = BTreeMap::new();
        query.insert("all".to_owned(), "1".to_owned());
        query.insert("limit".to_owned(), "999".to_owned());
        assert_eq!(
            federation_parse_query(&query).expect("query"),
            FederationQuery {
                all: true,
                limit: FEDERATION_DEFAULT_LIMIT
            }
        );
        query.insert("limit".to_owned(), "--".to_owned());
        assert!(federation_parse_query(&query)
            .expect_err("guard")
            .contains("limit"));
    }

    #[test]
    fn federation_discoveries_filter_sort_and_alias_shape() {
        let peers = vec![
            federation_peer("paired", 1_700_000_000_000, true),
            federation_peer("newer", 1_700_000_003_000, false),
            federation_peer("older", 1_700_000_001_000, false),
        ];
        let response = federation_render_discoveries(
            &peers,
            FederationQuery {
                all: false,
                limit: 10,
            },
            1_700_000_004_000,
        );
        assert_eq!(response.total, 2);
        assert_eq!(response.shown, 2);
        assert!(response.filtered);
        assert_eq!(response.peers[0].node, "newer");
        assert_eq!(response.peers[0].seen_rel, "1s");
        assert_eq!(response.peers[1].node, "older");
    }

    #[tokio::test]
    async fn federation_real_wire_is_public_under_default_deny() {
        let state = FederationState {
            status: FederationStatus {
                local_url: "http://local.test:3456".to_owned(),
                peers: vec![FederationPeerStatus {
                    url: "http://paired.test:3456".to_owned(),
                    node: Some("paired".to_owned()),
                    reachable: true,
                    latency: Some(12),
                    agents: vec!["paired:claude".to_owned()],
                    clock_warning: false,
                }],
            },
            discoveries: vec![
                federation_peer("paired", 1_700_000_000_000, true),
                federation_peer("fresh", 1_700_000_005_000, false),
            ],
        };
        let addr = federation_spawn(state).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let status = client
            .get(format!("http://{addr}/api/federation/status"))
            .send()
            .await
            .expect("status");
        assert_eq!(status.status(), StatusCode::OK);
        let status_payload = status
            .json::<serde_json::Value>()
            .await
            .expect("status json");
        assert_eq!(status_payload["local_url"], "http://local.test:3456");
        let discoveries = client
            .get(format!("http://{addr}/api/peers/discovered?limit=1"))
            .send()
            .await
            .expect("discoveries");
        assert_eq!(discoveries.status(), StatusCode::OK);
        let payload = discoveries
            .json::<serde_json::Value>()
            .await
            .expect("discoveries json");
        assert_eq!(payload["shown"], 1);
        assert_eq!(payload["peers"][0]["node"], "fresh");
        assert_eq!(payload["filtered"], true);
    }
}
