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
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

/// How long serve trusts its aggregated fleet-session snapshot before a request
/// triggers a fresh sweep — small enough to feel live, large enough that a burst
/// of page loads causes at most one fan-out per window (#15).
const FLEET_SESSIONS_TTL_MS: u64 = 15_000;

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
    let mut payload = federation_live_payload().await;
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
        // Session names reveal what each node is running — off-loopback keep only
        // the count is still too much detail, so drop them entirely.
        peer.agents = Vec::new();
        // The fetch error embeds the resolved IP/URL — drop it off-loopback too.
        peer.fetch_error = None;
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
        None => federation_live_payload().await,
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

// A wire/display row: each bool is a distinct flag the /fed page and consumers
// read by name (reachable / node_unique / loopback_self / clock_warning), so
// grouping them would break the JSON contract, not clarify it.
#[allow(clippy::struct_excessive_bools)]
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
    /// Whether our signed requests authenticate to this peer (`POST /api/probe`).
    /// `None` = not checked / unreachable; `Some(false)` = reachable but auth
    /// refused — the `/info`-OK-but-`/api/send`-401 gap the map must show.
    auth_ok: Option<bool>,
    /// Why the `/api/sessions` aggregation returned no sessions for this peer, if
    /// it failed — so an empty `agents` is diagnosable at a glance instead of
    /// meaning four things at once. `None` = the fetch succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    fetch_error: Option<String>,
    /// `true` when `resolved_ip` is one of THIS machine's own interface addresses
    /// — the probe looped back to our own serve, so those `agents` are ours, not
    /// the peer's ("green while broken"). Catches self-reference through a real
    /// interface (e.g. `10.20.0.18`), not just `127.0.0.1` (twin-found).
    loopback_self: bool,
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
                auth_ok: None,
                fetch_error: None,
                loopback_self: false,
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
async fn federation_live_payload() -> FederationStatusPayload {
    let store = federation_load_real_peer_store();
    let urls = store
        .peers
        .values()
        .map(|record| record.url.clone())
        .collect::<Vec<_>>();
    // Resolve every peer's routable IP ONCE, concurrently and ASYNC. A blocking
    // getaddrinfo inside the sweep serialized the fetches and made one peer's
    // measured latency include another peer's DNS wait — the number was the
    // loop's, not the peer's (twin). The fetch pin AND the displayed resolved_ip
    // both read this one map, so they cannot diverge either.
    let resolved = federation_resolve_all(&urls).await;
    let outcomes = federation_fleet_sessions(&urls, &resolved).await;
    federation_payload_from_store(&store, &outcomes, &resolved)
}

/// Resolve each URL's routable IP concurrently, off the blocking DNS path.
async fn federation_resolve_all(urls: &[String]) -> BTreeMap<String, std::net::IpAddr> {
    let lookups = urls.iter().cloned().map(|url| async move {
        let ip = federation_resolve_routable(&url).await?;
        Some((url, ip))
    });
    futures_util::future::join_all(lookups)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// Routable resolution, off the async task. Uses `std::to_socket_addrs` on a
/// blocking thread (`spawn_blocking`) — NOT `tokio::net::lookup_host`, which
/// returned nothing for `.local`/mDNS names on Linux and broke every hostname
/// peer (regression in v1503). This keeps the sync resolver that actually
/// resolves `.local`, while still not blocking the sweep, plus the same
/// link-local/loopback filter, preferring IPv4.
async fn federation_resolve_routable(url: &str) -> Option<std::net::IpAddr> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_owned();
    let port = parsed.port_or_known_default().unwrap_or(3456);
    let host_is_loopback = federation_host_is_loopback(&host);
    tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        let mut addrs = (host.as_str(), port)
            .to_socket_addrs()
            .ok()?
            .filter(|addr| host_is_loopback || federation_addr_is_routable(&addr.ip()))
            .collect::<Vec<_>>();
        // Prefer IPv4 — LAN peers answer on it and it dodges zone-less IPv6.
        addrs.sort_by_key(|addr| u8::from(addr.is_ipv6()));
        addrs.into_iter().next().map(|addr| addr.ip())
    })
    .await
    .ok()
    .flatten()
}

/// One peer's live fetch result. `error` is kept so an empty `sessions` says WHY
/// (unreachable vs refused vs genuinely none vs unbuildable URL). `connected`
/// records whether we got ANY response — it drives `reachable`, so the map can
/// stop showing a green card next to empty data (twin: reachable came from a
/// different path than the fetch, so both could be true and vacant at once).
#[derive(Clone, Debug, Default)]
struct FleetOutcome {
    sessions: Vec<String>,
    error: Option<String>,
    connected: bool,
    /// Round-trip time of the sweep's GET, `None` when we never connected. Now
    /// that the fetch is live, measuring it is nearly free — and a field that is
    /// null forever is its own quiet lie (twin).
    latency_ms: Option<u64>,
}

impl FleetOutcome {
    /// A fetch that never got a response (`connected: false`) or could not even
    /// build a URL — no latency.
    fn failed(error: String, connected: bool) -> Self {
        Self {
            sessions: Vec::new(),
            error: Some(error),
            connected,
            latency_ms: None,
        }
    }

    /// Connected (we got a round-trip) but the response was not usable — non-2xx
    /// or bad JSON. Reachable, with a measured latency, but no sessions.
    fn reached(error: String, latency_ms: Option<u64>) -> Self {
        Self {
            sessions: Vec::new(),
            error: Some(error),
            connected: true,
            latency_ms,
        }
    }
}

/// `(fetched_at_ms, outcome-by-peer-url)` guarded for the TTL fleet cache.
type FleetSessionsCache = Mutex<(u64, BTreeMap<String, FleetOutcome>)>;

fn fleet_sessions_cache() -> &'static FleetSessionsCache {
    static CACHE: OnceLock<FleetSessionsCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new((0, BTreeMap::new())))
}

/// Session outcomes per peer URL, aggregated **server-side** on a TTL cycle —
/// the browser never fans out to peers (CORS + auth + N connections). The lock
/// is never held across the `.await`, so concurrent requests at most double a
/// sweep (idempotent) rather than deadlock (#15).
async fn federation_fleet_sessions(
    urls: &[String],
    resolved: &BTreeMap<String, std::net::IpAddr>,
) -> BTreeMap<String, FleetOutcome> {
    let now = federation_now_millis();
    if let Ok(guard) = fleet_sessions_cache().lock() {
        if !guard.1.is_empty() && now.saturating_sub(guard.0) < FLEET_SESSIONS_TTL_MS {
            return guard.1.clone();
        }
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(2500))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            let error = format!("http client build failed: {error}");
            return urls
                .iter()
                .map(|url| (url.clone(), FleetOutcome::failed(error.clone(), false)))
                .collect();
        }
    };
    // Signed once per sweep, not once per peer: the credentials are identical
    // for every peer and assembling them touches the config/peer-key files.
    // #866 gated GET /sessions behind the signed-peer check, so an unsigned
    // sweep would read 403 from every patched peer and render the whole fleet
    // as unreachable.
    let signed_headers = federation_signed_sessions_headers();
    let fetches = urls.iter().cloned().map(|url| {
        let client = client.clone();
        let pin = resolved.get(&url).copied();
        let headers = signed_headers.clone();
        async move {
            let outcome = federation_fetch_peer_sessions(&client, &url, pin, &headers).await;
            (url, outcome)
        }
    });
    let map = futures_util::future::join_all(fetches)
        .await
        .into_iter()
        .collect::<BTreeMap<String, FleetOutcome>>();
    if let Ok(mut guard) = fleet_sessions_cache().lock() {
        if !map.is_empty() {
            *guard = (now, map.clone());
        }
    }
    map
}

/// Fetch one peer's `/api/sessions` and keep just the session names, plus the
/// error string when it did not succeed — so `agents: []` is never ambiguous.
/// The headers that authenticate this node's fleet sweep to a peer, or an empty
/// list when this node cannot sign (no identity / peer key / federation token).
/// Signing is best-effort by design: an unsignable node still sweeps, and an
/// unpatched peer still answers, so the failure mode is a clear 403 from a
/// patched peer rather than a serve that cannot sweep at all.
fn federation_signed_sessions_headers() -> Vec<(String, String)> {
    crate::core_impl::federation_signed_get_headers("/api/sessions")
        .map(|headers| headers.to_btree_map().into_iter().collect())
        .unwrap_or_default()
}

async fn federation_fetch_peer_sessions(
    client: &reqwest::Client,
    url: &str,
    pin: Option<std::net::IpAddr>,
    signed_headers: &[(String, String)],
) -> FleetOutcome {
    // `pin` was resolved once, up front, off the blocking DNS path — a zone-less
    // link-local IPv6 (which getaddrinfo may return first) can't silently sink
    // the fetch, and because resolution is not in this timed region the latency
    // below measures only this peer's round-trip.
    let Some(endpoint) = federation_sessions_endpoint(url, pin) else {
        return FleetOutcome::failed(format!("could not build endpoint from {url}"), false);
    };
    let target = endpoint.to_string();
    let started = std::time::Instant::now();
    let mut request = client.get(endpoint);
    for (name, value) in signed_headers {
        request = request.header(name, value);
    }
    let response = match request.send().await {
        Ok(response) => response,
        // Never got a response → not connected → not reachable, no latency.
        Err(error) => return FleetOutcome::failed(federation_send_error(&target, &error), false),
    };
    // We have a response: the peer is connected with a round-trip, even if the
    // status is not 2xx — a 404/401 means "reached, refused", not "unreachable".
    let latency_ms = u64::try_from(started.elapsed().as_millis()).ok();
    let status = response.status();
    if !status.is_success() {
        return FleetOutcome::reached(format!("GET {target} -> HTTP {status}"), latency_ms);
    }
    let value = match response.json::<serde_json::Value>().await {
        Ok(value) => value,
        Err(error) => {
            return FleetOutcome::reached(format!("bad JSON from {target}: {error}"), latency_ms)
        }
    };
    let names = value
        .as_array()
        .map(|sessions| {
            sessions
                .iter()
                .filter_map(|session| session.get("name").and_then(serde_json::Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    FleetOutcome {
        sessions: names,
        error: None,
        connected: true,
        latency_ms,
    }
}

/// Full reason a `/api/sessions` GET failed, not just reqwest's outer Display.
/// Classifies the failure (connect / timeout / request) and walks the
/// `Error::source()` chain so the OS-level cause surfaces — otherwise the string
/// stops at "error sending request" one layer short of the real reason (e.g.
/// macOS "Operation not permitted" when a daemon lacks Local Network access, or
/// "No route to host"). The diagnostic must reach the actual cause, not near it.
fn federation_send_error(target: &str, error: &reqwest::Error) -> String {
    let kind = if error.is_connect() {
        "connect"
    } else if error.is_timeout() {
        "timeout"
    } else if error.is_request() {
        "request"
    } else {
        "send"
    };
    let mut detail = format!("GET {target} [{kind}]: {error}");
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        detail.push_str(" | ");
        detail.push_str(&inner.to_string());
        source = inner.source();
    }
    detail
}

/// Pure mapping from a peer store (+ aggregated fleet sessions) to the status
/// payload — the deterministic core of `federation_live_payload`, split out so
/// it is testable without disk/env/network.
fn federation_payload_from_store(
    store: &maw_peer::PeerStoreFile,
    outcomes: &BTreeMap<String, FleetOutcome>,
    resolved: &BTreeMap<String, std::net::IpAddr>,
) -> FederationStatusPayload {
    let node_counts =
        federation_node_counts(store.peers.values().map(|record| record.node.as_deref()));
    let peers = store
        .peers
        .values()
        .map(|record| {
            let outcome = outcomes.get(&record.url);
            FederationStatusPeer {
                url: record.url.clone(),
                node: record.node.clone(),
                // Reachable = we actually connected on THIS sweep, so a green card
                // can't sit next to empty data (twin: reachable used to come from a
                // separate stale probe). Falls back to the stored probe only when a
                // peer somehow was not swept.
                reachable: outcome.map_or(record.last_error.is_none(), |outcome| outcome.connected),
                latency: outcome.and_then(|outcome| outcome.latency_ms),
                agents: outcome
                    .map(|outcome| outcome.sessions.clone())
                    .unwrap_or_default(),
                clock_warning: false,
                oracle: record
                    .identity
                    .as_ref()
                    .map(|identity| identity.oracle.clone())
                    .filter(|oracle| !oracle.is_empty()),
                resolved_ip: resolved.get(&record.url).map(std::net::IpAddr::to_string),
                node_unique: federation_node_unique(&node_counts, record.node.as_deref()),
                auth_ok: record.auth_ok,
                fetch_error: outcome.and_then(|outcome| outcome.error.clone()),
                loopback_self: resolved
                    .get(&record.url)
                    .is_some_and(|ip| federation_is_local_ip(*ip)),
            }
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

/// Build the `/api/sessions` endpoint for a peer URL: pin the host to a routable
/// IP from `resolve` (so a zone-less link-local address can't sink the fetch),
/// and **append** `/api/sessions` to any existing base path rather than
/// replacing it — a peer behind a reverse proxy prefix (`…/maw`) keeps it.
///
/// `resolve` is taken as a parameter (not computed at the call site) so a single
/// test with a fake resolver guards the *whole* decision — reverting the pin,
/// the resolver call, or the path-append all turn a test red. Guarding only the
/// pure predicates would leave the "do we even resolve+pin?" wiring unwatched.
/// Build the `/api/sessions` endpoint: pin the host to the pre-resolved routable
/// `pin` (so a zone-less link-local address can't sink the fetch) and **append**
/// `/api/sessions` to any existing base path rather than replacing it — a peer
/// behind a reverse-proxy prefix (`…/maw`) keeps it. `pin` is resolved once,
/// up front and async, so the pin decision + the displayed `resolved_ip` share a
/// single source and the sweep never blocks on DNS. Reverting the pin or the
/// path-append turns a test red.
fn federation_sessions_endpoint(url: &str, pin: Option<std::net::IpAddr>) -> Option<reqwest::Url> {
    let mut endpoint = reqwest::Url::parse(url).ok()?;
    if let Some(ip) = pin {
        // Fail OPEN: if pinning ever fails, keep the hostname and let the client
        // resolve rather than abandoning the fetch (twin nit 1). `set_ip_host`
        // only errors for a cannot-be-a-base URL, which http never is.
        let _ = endpoint.set_ip_host(ip);
    }
    let base = endpoint.path().trim_end_matches('/').to_owned();
    endpoint.set_path(&format!("{base}/api/sessions"));
    Some(endpoint)
}

/// Is `ip` one of THIS machine's own interface addresses? Binding an ephemeral
/// socket to it succeeds only for a local address (the OS rejects binding a
/// non-local IP with `EADDRNOTAVAIL`), so this catches self-reference through any
/// real interface — loopback AND e.g. `10.20.0.18` — with no crate and no unsafe.
fn federation_is_local_ip(ip: std::net::IpAddr) -> bool {
    std::net::TcpListener::bind((ip, 0)).is_ok()
}

fn federation_host_is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn federation_addr_is_routable(ip: &std::net::IpAddr) -> bool {
    if ip.is_loopback() {
        return false;
    }
    match ip {
        std::net::IpAddr::V4(v4) => !v4.is_link_local(),
        std::net::IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) != 0xfe80,
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
            auth_ok: None,
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

        let payload = federation_payload_from_store(&store, &BTreeMap::new(), &BTreeMap::new());
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
                auth_ok: None,
                fetch_error: Some("GET http://192.168.1.118:3456/api/sessions: boom".to_owned()),
                loopback_self: false,
            }],
        };
        federation_redact_payload(&mut payload);
        // Topology detail hidden off-loopback…
        assert_eq!(payload.peers[0].url, "192.168.1.118");
        assert_eq!(payload.peers[0].resolved_ip, None);
        // …the fetch error embeds the IP/URL, so it drops too…
        assert_eq!(payload.peers[0].fetch_error, None);
        // …but the map itself (node, oracle, reachability, uniqueness) stays.
        assert_eq!(payload.peers[0].node.as_deref(), Some("m5"));
        assert_eq!(payload.peers[0].oracle.as_deref(), Some("atlas"));
        assert!(payload.peers[0].reachable && payload.peers[0].node_unique);
    }

    #[test]
    fn federation_addr_routable_skips_loopback_and_link_local() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        assert!(federation_addr_is_routable(&IpAddr::V4(Ipv4Addr::new(
            192, 168, 1, 184
        ))));
        assert!(!federation_addr_is_routable(&IpAddr::V4(
            Ipv4Addr::LOCALHOST
        )));
        assert!(!federation_addr_is_routable(&IpAddr::V4(Ipv4Addr::new(
            169, 254, 1, 1
        ))));
        assert!(!federation_addr_is_routable(&IpAddr::V6(
            "fe80::da5e:d3ff:fe0d:c99f".parse().unwrap()
        )));
        assert!(federation_addr_is_routable(&IpAddr::V6(
            "2001:db8::1".parse().unwrap()
        )));
        assert!(!federation_addr_is_routable(&IpAddr::V6(
            Ipv6Addr::LOCALHOST
        )));
        // A loopback host keeps loopback (the m5.local→127.0.0.1 self-trap).
        assert!(federation_host_is_loopback("localhost"));
        assert!(federation_host_is_loopback("127.0.0.1"));
        assert!(!federation_host_is_loopback("black.local"));
    }

    #[test]
    fn federation_is_local_ip_flags_own_interface_not_a_remote() {
        use std::net::{IpAddr, Ipv4Addr};
        // Loopback is always a local interface → self-reference.
        assert!(federation_is_local_ip(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        // TEST-NET-3 (203.0.113.0/24, RFC 5737) is never assigned to an
        // interface → a real remote. This is the flag the CLI/page rely on to
        // catch "the probe reached our own serve" even via a non-loopback IP.
        assert!(!federation_is_local_ip(IpAddr::V4(Ipv4Addr::new(
            203, 0, 113, 7
        ))));
    }

    #[test]
    fn federation_sessions_endpoint_resolves_pins_and_keeps_base_path() {
        use std::net::{IpAddr, Ipv4Addr};
        // The pin is the pre-resolved routable IP (async resolution happens up
        // front now). Reverting the pin or the path-append turns this red.
        let routable = Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 184)));
        assert_eq!(
            federation_sessions_endpoint("http://black.local:3456", routable)
                .unwrap()
                .as_str(),
            "http://192.168.1.184:3456/api/sessions"
        );
        // R3: a reverse-proxy base path is preserved, not discarded.
        assert_eq!(
            federation_sessions_endpoint("http://host:3456/maw", None)
                .unwrap()
                .as_str(),
            "http://host:3456/maw/api/sessions"
        );
        // No pin + no base path → clean /api/sessions.
        assert_eq!(
            federation_sessions_endpoint("http://host:3456", None)
                .unwrap()
                .as_str(),
            "http://host:3456/api/sessions"
        );
    }

    #[test]
    fn federation_payload_maps_fleet_sessions_and_redaction_clears_them() {
        let mut store = maw_peer::PeerStoreFile::default();
        store.peers.insert(
            "a".to_owned(),
            federation_store_record("http://a.test:3456", "na", None, false),
        );
        store.peers.insert(
            "b".to_owned(),
            federation_store_record("http://b.test:3456", "nb", None, false),
        );
        store.peers.insert(
            "c".to_owned(),
            federation_store_record("http://c.test:3456", "nc", None, false),
        );
        let mut outcomes = BTreeMap::new();
        outcomes.insert(
            "http://a.test:3456".to_owned(),
            FleetOutcome {
                sessions: vec!["06-fb".to_owned(), "08-gpu".to_owned()],
                error: None,
                connected: true,
                latency_ms: Some(7),
            },
        );
        // A failed fetch carries WHY, so `agents: []` is never ambiguous — and a
        // failure to connect drops `reachable`, so the card can't be green + empty.
        outcomes.insert(
            "http://b.test:3456".to_owned(),
            FleetOutcome::failed(
                "GET http://b.test:3456/api/sessions: timed out".to_owned(),
                false,
            ),
        );
        // The dividing line of this fix: a non-2xx peer CONNECTED (reached,
        // refused) — reachable stays true with a latency, only agents is empty.
        outcomes.insert(
            "http://c.test:3456".to_owned(),
            FleetOutcome::reached(
                "GET http://c.test:3456/api/sessions -> HTTP 404".to_owned(),
                Some(9),
            ),
        );
        // resolved_ip on the payload comes from this pre-resolved map (shared
        // with the fetch pin), not a separate lookup that could disagree.
        let mut resolved = BTreeMap::new();
        resolved.insert(
            "http://a.test:3456".to_owned(),
            "192.168.1.10".parse::<std::net::IpAddr>().unwrap(),
        );
        let mut payload = federation_payload_from_store(&store, &outcomes, &resolved);
        let by_url = |u: &str| {
            payload
                .peers
                .iter()
                .find(|p| p.url == u)
                .cloned()
                .expect("peer")
        };
        assert_eq!(by_url("http://a.test:3456").agents, vec!["06-fb", "08-gpu"]);
        assert_eq!(by_url("http://a.test:3456").fetch_error, None);
        assert!(by_url("http://a.test:3456").reachable);
        assert_eq!(
            by_url("http://a.test:3456").resolved_ip.as_deref(),
            Some("192.168.1.10")
        );
        assert_eq!(by_url("http://a.test:3456").latency, Some(7));
        assert!(by_url("http://b.test:3456").agents.is_empty());
        assert!(
            !by_url("http://b.test:3456").reachable,
            "a fetch that never connected must not read reachable"
        );
        assert!(by_url("http://b.test:3456")
            .fetch_error
            .unwrap()
            .contains("timed out"));
        // non-2xx: reached but refused → reachable TRUE, latency present, no agents.
        let c = by_url("http://c.test:3456");
        assert!(
            c.reachable,
            "a peer that answered 404 was reached, not down"
        );
        assert_eq!(c.latency, Some(9));
        assert!(c.agents.is_empty());
        assert!(c.fetch_error.unwrap().contains("HTTP 404"));
        federation_redact_payload(&mut payload);
        assert!(
            payload.peers.iter().all(|p| p.agents.is_empty()),
            "off-loopback redaction drops session names"
        );
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
