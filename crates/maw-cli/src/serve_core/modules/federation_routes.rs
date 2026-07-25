use super::ServecoreModuleRegistration;
use crate::serve_core::ServecoreLifecycleModule;
use axum::{
    extract::{ConnectInfo, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Extension, Json, Router,
};
use maw_transport::FederationStatus;
use serde::Serialize;
use serde_json::json;
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

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
        // `/fed.json` is served OUTSIDE the `/api/` gate (the token gate only
        // guards `/api/*`), so a browser on another machine can fetch it once a
        // token exists — which means it MUST redact off-loopback (#9).
        .route("/fed.json", get(federation_fed_json_get))
        .layer(Extension(Arc::new(state)))
}

async fn federation_fed_json_get(ConnectInfo(peer): ConnectInfo<SocketAddr>) -> impl IntoResponse {
    let mut payload = federation_live_payload();
    if !peer.ip().is_loopback() {
        federation_redact_payload(&mut payload);
    }
    Json(payload).into_response()
}

/// Strip topology detail that a remote (non-loopback) viewer should not see:
/// the full URL collapses to its host, and the resolved IP is dropped. Node,
/// oracle, reachability and the `node_unique` flag stay — they are the map.
fn federation_redact_payload(payload: &mut FederationStatusPayload) {
    for peer in &mut payload.peers {
        peer.url = federation_host_only(&peer.url);
        peer.resolved_ip = None;
    }
}

fn federation_host_only(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned))
        .unwrap_or_else(|| url.to_owned())
}

async fn federation_status_get(
    Extension(state): Extension<Arc<FederationState>>,
) -> impl IntoResponse {
    let payload = match &state.status_override {
        Some(status) => federation_status_payload(status),
        None => federation_live_payload(),
    };
    Json(payload).into_response()
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
    /// Test-only injected status. In production this is `None`, so the handler
    /// reads the real peer store **per request** (fixing the freeze where the
    /// status was baked empty at mount time — #7, same class as #524).
    status_override: Option<FederationStatus>,
    discoveries: Vec<FederationDiscoveredPeer>,
}

fn federation_default_state() -> FederationState {
    FederationState {
        status_override: None,
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
    /// Real oracle name from the peer's pinned identity — no longer the fake
    /// fleet-wide `"mawjs"` default now that `/info` returns `oracle` (Truth #3).
    oracle: Option<String>,
    /// IP the peer URL resolves to, so the map can flag the
    /// `m5.local → 127.0.0.1` loopback-to-self trap (Truth #4).
    resolved_ip: Option<String>,
    /// `true` when no other peer shares this `node` name. A duplicate node makes
    /// `matches_local_peer` match someone else's row as "us" → fake Healthy.
    node_unique: bool,
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
    let node_counts = federation_node_counts(status.peers.iter().map(|peer| peer.node.as_deref()));
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
                oracle: None,
                resolved_ip: None,
                node_unique: federation_node_unique(&node_counts, peer.node.as_deref()),
            })
            .collect(),
    }
}

/// Count how many peers carry each `node` name, so a row can flag itself unique.
fn federation_node_counts<'a>(
    nodes: impl Iterator<Item = Option<&'a str>>,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for node in nodes.flatten().filter(|node| !node.is_empty()) {
        *counts.entry(node.to_owned()).or_insert(0) += 1;
    }
    counts
}

fn federation_node_unique(counts: &BTreeMap<String, usize>, node: Option<&str>) -> bool {
    node.is_some_and(|node| counts.get(node).copied().unwrap_or(0) <= 1)
}

/// Build the federation status payload from the real peer store (`peers.json`),
/// read fresh on each request so an added/removed/re-probed peer shows up
/// without restarting serve. This is the production replacement for the old
/// empty `federation_default_state` stub (#7).
fn federation_live_payload() -> FederationStatusPayload {
    federation_payload_from_store(&federation_load_real_peer_store())
}

/// Pure mapping from a peer store to the status payload — the deterministic core
/// of `federation_live_payload`, split out so it is testable without disk/env.
fn federation_payload_from_store(store: &maw_peer::PeerStoreFile) -> FederationStatusPayload {
    let node_counts =
        federation_node_counts(store.peers.values().map(|record| record.node.as_deref()));
    let peers = store
        .peers
        .values()
        .map(|record| FederationStatusPeer {
            url: record.url.clone(),
            node: record.node.clone(),
            // A peer whose last probe left no error is reachable; a recorded
            // `lastError` means the last handshake failed.
            reachable: record.last_error.is_none(),
            latency: None,
            agents: Vec::new(),
            clock_warning: false,
            oracle: record
                .identity
                .as_ref()
                .map(|identity| identity.oracle.clone())
                .filter(|oracle| !oracle.is_empty()),
            resolved_ip: federation_resolve_ip(&record.url),
            node_unique: federation_node_unique(&node_counts, record.node.as_deref()),
        })
        .collect();
    FederationStatusPayload {
        local_url: String::new(),
        peers,
    }
}

/// Read the real `peers.json` the same way serve's startup warnings do, honoring
/// `PEERS_FILE`/`MAW_HOME`/XDG overrides so serve and `maw peers` agree.
fn federation_load_real_peer_store() -> maw_peer::PeerStoreFile {
    let home = std::env::var_os("HOME")
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = [
        "PEERS_FILE",
        "MAW_HOME",
        "MAW_STATE_DIR",
        "MAW_XDG",
        "XDG_STATE_HOME",
        "MAW_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "MAW_DATA_DIR",
        "XDG_DATA_HOME",
        "MAW_CACHE_DIR",
        "XDG_CACHE_HOME",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key, value)));
    let env = maw_peer::PeerStoreEnv::with_vars(home, vars);
    maw_peer::load_peer_store(&env)
}

/// Resolve a peer URL's host to its first IP (best effort) so the map can catch
/// the loopback-to-self trap. `None` when the URL has no host or DNS fails.
fn federation_resolve_ip(url: &str) -> Option<String> {
    use std::net::ToSocketAddrs;
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default().unwrap_or(3456);
    (host, port)
        .to_socket_addrs()
        .ok()?
        .next()
        .map(|addr| addr.ip().to_string())
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

    fn federation_store_record(
        url: &str,
        node: &str,
        oracle: Option<&str>,
        errored: bool,
    ) -> maw_peer::PeerRecord {
        maw_peer::PeerRecord {
            url: url.to_owned(),
            node: Some(node.to_owned()),
            added_at: "2026-07-25T00:00:00Z".to_owned(),
            last_seen: Some("2026-07-25T00:00:00Z".to_owned()),
            last_error: errored.then(|| maw_peer::ProbeLastError {
                code: maw_peer::ProbeErrorCode::Dns,
                message: "getaddrinfo ENOTFOUND".to_owned(),
                at: "2026-07-25T00:00:00Z".to_owned(),
            }),
            nickname: None,
            pubkey: None,
            pubkey_first_seen: None,
            identity: oracle.map(|oracle| maw_peer::PeerIdentity {
                oracle: oracle.to_owned(),
                node: node.to_owned(),
            }),
            one_way: None,
            last_symmetric_check: None,
        }
    }

    #[test]
    fn federation_payload_from_store_maps_peers_flags_duplicate_nodes_and_reachability() {
        let mut store = maw_peer::PeerStoreFile::default();
        store.peers.insert(
            "a".to_owned(),
            federation_store_record("http://a.test:3456", "dup", Some("atlas"), false),
        );
        store.peers.insert(
            "b".to_owned(),
            federation_store_record("http://b.test:3456", "dup", None, true),
        );
        store.peers.insert(
            "c".to_owned(),
            federation_store_record("http://c.test:3456", "solo", Some("nova"), false),
        );

        let payload = federation_payload_from_store(&store);
        assert_eq!(payload.peers.len(), 3, "real peers, not the empty stub");

        let by_node = |node: &str| {
            payload
                .peers
                .iter()
                .find(|peer| peer.node.as_deref() == Some(node))
                .cloned()
                .expect("peer present")
        };
        // Two peers share "dup" → neither is unique; "solo" is.
        assert!(!by_node("dup").node_unique);
        assert!(by_node("solo").node_unique);
        // Real oracle from identity, no longer the fake fleet-wide "mawjs".
        assert_eq!(by_node("solo").oracle.as_deref(), Some("nova"));
        // last_error present → not reachable.
        assert!(payload
            .peers
            .iter()
            .any(|peer| peer.node.as_deref() == Some("dup") && !peer.reachable));
    }

    #[test]
    fn federation_redact_payload_hides_url_and_ip_but_keeps_the_map() {
        let mut payload = FederationStatusPayload {
            local_url: "http://local:3456".to_owned(),
            peers: vec![FederationStatusPeer {
                url: "http://192.168.1.118:3456".to_owned(),
                node: Some("m5".to_owned()),
                reachable: true,
                latency: None,
                agents: Vec::new(),
                clock_warning: false,
                oracle: Some("atlas".to_owned()),
                resolved_ip: Some("192.168.1.118".to_owned()),
                node_unique: true,
            }],
        };
        federation_redact_payload(&mut payload);
        // Topology detail hidden off-loopback…
        assert_eq!(payload.peers[0].url, "192.168.1.118");
        assert_eq!(payload.peers[0].resolved_ip, None);
        // …but the map itself (node, oracle, reachability, uniqueness) stays.
        assert_eq!(payload.peers[0].node.as_deref(), Some("m5"));
        assert_eq!(payload.peers[0].oracle.as_deref(), Some("atlas"));
        assert!(payload.peers[0].reachable && payload.peers[0].node_unique);
    }

    #[test]
    fn federation_default_state_reads_live_never_a_baked_status() {
        // The production mount must leave `status_override` empty so the handler
        // reads the peer store live. Baking a status here (the old #524-class
        // stub) would make the map lie — this guard flips if that regresses.
        assert!(federation_default_state().status_override.is_none());
    }

    #[tokio::test]
    async fn federation_real_wire_is_public_under_default_deny() {
        let state = FederationState {
            status_override: Some(FederationStatus {
                local_url: "http://local.test:3456".to_owned(),
                peers: vec![FederationPeerStatus {
                    url: "http://paired.test:3456".to_owned(),
                    node: Some("paired".to_owned()),
                    reachable: true,
                    latency: Some(12),
                    agents: vec!["paired:claude".to_owned()],
                    clock_warning: false,
                }],
            }),
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
