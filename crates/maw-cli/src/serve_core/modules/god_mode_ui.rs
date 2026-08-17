use super::agent_status::{
    agentstatus_drain_feed, agentstatus_feed_history_and_cursor, agentstatus_poll_global,
    agentstatus_sessions_from_tmux, AgentStatusSnapshot,
};
use super::ServecoreModuleRegistration;
use crate::serve_core::{
    process_engine::{serveengine_tmux_capture, serveengine_tmux_capture_lines},
    servecore_ws_connection_guard, servecore_ws_connection_limit_reached,
    servecore_ws_handle_frame, servecore_ws_send, servecore_ws_send_text_frames,
    servecore_ws_target, ServecoreAgentPane, ServecoreLifecycleModule, ServecoreSharedState,
    ServecoreWsKind,
};
use axum::{
    body::{to_bytes, Body},
    extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    http::{Request, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use maw_tmux::{TmuxSession, TmuxWindow};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const GODUI_TEAM_RECENT_MS: u64 = 2 * 60 * 60 * 1_000;
const GODUI_POST_BODY_LIMIT: usize = 64 * 1024;
const GODUI_PREVIEW_LINES: u32 = 15;

#[must_use]
pub fn godui_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "god-ui".to_owned(),
        weight: 50,
    }
}

#[must_use]
pub fn godui_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: godui_lifecycle_module(),
        mount: godui_mount,
    }
}

pub fn godui_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/api/costs", get(godui_costs_get))
        .route("/api/teams", get(godui_teams_get))
        .route(
            "/api/ui-state",
            get(godui_ui_state_get).post(godui_ui_state_post),
        )
        .route("/api/asks", get(godui_asks_get).post(godui_asks_post))
        .route("/api/pin-info", get(godui_pin_info_get))
        .route(
            "/ws",
            get(godui_ws_upgrade).layer(Extension(
                super::websocket_routes::WsConfig::ws_from_process_env(),
            )),
        )
}

async fn godui_costs_get() -> impl IntoResponse {
    Json(godui_costs_payload()).into_response()
}

async fn godui_teams_get(
    Extension(state): Extension<Arc<ServecoreSharedState>>,
) -> impl IntoResponse {
    // #880: PROPAGATE. The team configs come off disk, but each team's
    // `alive` flag is decided by matching member pane ids against the live
    // tmux pane set. An unreachable tmux made that set empty, so every team
    // shipped `alive: false` -- "your whole team is dead", asserted from a
    // read that never happened. There is no honest partial answer here (the
    // flag is not optional in the payload), so the whole response is 503.
    match godui_teams_payload(&state) {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": error})),
        )
            .into_response(),
    }
}

async fn godui_ui_state_get() -> impl IntoResponse {
    Json(godui_ui_state_payload()).into_response()
}

async fn godui_ui_state_post(req: Request<Body>) -> Response {
    godui_store_json_body(req, &godui_current_dir_file("ui-state.json")).await
}

async fn godui_asks_get() -> impl IntoResponse {
    Json(godui_asks_payload()).into_response()
}

async fn godui_asks_post(req: Request<Body>) -> Response {
    godui_store_json_body(req, &godui_current_dir_file("asks.json")).await
}

async fn godui_pin_info_get() -> impl IntoResponse {
    Json(godui_pin_info_payload()).into_response()
}

async fn godui_ws_upgrade(
    ws: WebSocketUpgrade,
    uri: Uri,
    Extension(state): Extension<Arc<ServecoreSharedState>>,
    Extension(config): Extension<super::websocket_routes::WsConfig>,
) -> Response {
    let target = match super::websocket_routes::ws_validate_target(servecore_ws_target(uri.query()))
    {
        Ok(target) => target,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error":error}))).into_response()
        }
    };
    if state
        .engine
        .servecore_ws_open(ServecoreWsKind::Engine, target.as_deref())
        .is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"ws_engine_unavailable"})),
        )
            .into_response();
    }
    if servecore_ws_connection_limit_reached(config.max_connections) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"ws_connection_limit"})),
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| godui_ws_stream(socket, state, target, config))
        .into_response()
}

#[allow(clippy::too_many_lines)]
async fn godui_ws_stream(
    mut socket: WebSocket,
    state: Arc<ServecoreSharedState>,
    target: Option<String>,
    config: super::websocket_routes::WsConfig,
) {
    let Some(_guard) = servecore_ws_connection_guard(config.max_connections) else {
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code: 1013,
                reason: "ws connection limit".into(),
            })))
            .await;
        return;
    };
    let Some(mut feed_cursor) = godui_ws_send_initial(&mut socket, &state, &config).await else {
        state
            .engine
            .servecore_ws_close(ServecoreWsKind::Engine, target.as_deref());
        return;
    };
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + config.heartbeat_interval,
        config.heartbeat_interval,
    );
    let mut refresh = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(2),
        Duration::from_secs(2),
    );
    let mut capture_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + config.capture_interval,
        config.capture_interval,
    );
    let mut previews_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + config.previews_interval,
        config.previews_interval,
    );
    let mut subscribed_target = target.clone();
    let mut preview_targets = BTreeSet::new();
    let mut last_previews = BTreeMap::new();
    let idle_timer = tokio::time::sleep(config.idle_timeout);
    tokio::pin!(idle_timer);
    loop {
        tokio::select! {
            _ = refresh.tick() => {
                if !godui_ws_send_session_recent(&mut socket, &state, &config, &mut feed_cursor).await {
                    break;
                }
                idle_timer.as_mut().reset(tokio::time::Instant::now() + config.idle_timeout);
            }
            _ = capture_tick.tick() => {
                if let Some(frame) = subscribed_target.as_deref().and_then(godui_ws_capture_frame) {
                    if servecore_ws_send(&mut socket, Message::Text(frame), config.send_timeout).await.is_err() {
                        break;
                    }
                    idle_timer.as_mut().reset(tokio::time::Instant::now() + config.idle_timeout);
                }
            }
            _ = previews_tick.tick() => {
                if let Some(frame) = godui_ws_previews_frame(&preview_targets, &mut last_previews, godui_ws_preview_capture) {
                    if servecore_ws_send(&mut socket, Message::Text(frame), config.send_timeout).await.is_err() {
                        break;
                    }
                    idle_timer.as_mut().reset(tokio::time::Instant::now() + config.idle_timeout);
                }
            }
            _ = heartbeat.tick() => {
                if servecore_ws_send(&mut socket, Message::Ping(Vec::new()), config.send_timeout).await.is_err() {
                    break;
                }
            }
            () = &mut idle_timer => {
                let _ = servecore_ws_send(&mut socket, Message::Close(None), config.send_timeout).await;
                break;
            }
            frame = socket.recv() => {
                match frame {
                    Some(Ok(frame)) => {
                        let resets_idle = !matches!(frame, Message::Pong(_));
                        if resets_idle {
                            idle_timer.as_mut().reset(tokio::time::Instant::now() + config.idle_timeout);
                        }
                        let frame_target = if let Message::Text(text) = &frame {
                            if let Some(selected) = godui_ws_selected_target(text) {
                                subscribed_target = Some(selected);
                            }
                            if let Some(targets) = godui_ws_preview_targets(text) {
                                godui_ws_replace_preview_targets(
                                    &mut preview_targets,
                                    &mut last_previews,
                                    targets,
                                );
                                continue;
                            }
                            godui_ws_message_target(text).or_else(|| subscribed_target.clone()).or_else(|| target.clone())
                        } else {
                            subscribed_target.clone().or_else(|| target.clone())
                        };
                        if !servecore_ws_handle_frame(
                            &mut socket,
                            state.as_ref(),
                            ServecoreWsKind::Engine,
                            frame_target.as_deref(),
                            &config,
                            frame,
                        )
                        .await
                        {
                            break;
                        }
                    }
                    Some(Err(_)) | None => break,
                }
            }
        }
    }
    state
        .engine
        .servecore_ws_close(ServecoreWsKind::Engine, target.as_deref());
}

// #880: PROPAGATE, websocket-style. A websocket cannot answer 503 -- by the
// time this runs the HTTP handshake is already complete and the status line
// is spent. Probing tmux back in `godui_ws_upgrade` so it *could* 503 would
// cost a second tmux round trip per connection and still race (tmux can die
// in the gap), so the wire analog is used instead: the typed `{"type":
// "error"}` frame the engine socket already speaks, followed by a close.
// A connect-time failure means there is no truthful first snapshot to paint,
// and nothing to recover from within this stream -- the browser's reconnect
// is the retry -- so the socket does not stay open pretending.
async fn godui_ws_send_initial(
    socket: &mut WebSocket,
    state: &ServecoreSharedState,
    config: &super::websocket_routes::WsConfig,
) -> Option<u64> {
    let sessions = match state.servecore_tmux_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            let _ =
                servecore_ws_send_text_frames(socket, vec![godui_ws_error_frame(&error)], config)
                    .await;
            let _ = servecore_ws_send(socket, Message::Close(None), config.send_timeout).await;
            return None;
        }
    };
    let (frames, cursor) = godui_ws_initial_frames(sessions);
    servecore_ws_send_text_frames(socket, frames, config)
        .await
        .then_some(cursor)
}

// #880: DEGRADE, deliberately -- but without lying. This is the two-second
// refresh tick on an already-established dashboard socket, so tearing the
// connection down over a transient tmux hiccup would be worse than useless:
// the browser would reconnect into the same hiccup. What it must not do is
// keep pushing `sessions: []`, which is the #860 falsehood arriving on a
// timer. So a failed read emits the same typed error frame and *skips* the
// sessions/recent frames for this tick; the socket stays open and the next
// tick recovers on its own. Feed events do not come from tmux and still flow.
async fn godui_ws_send_session_recent(
    socket: &mut WebSocket,
    state: &ServecoreSharedState,
    config: &super::websocket_routes::WsConfig,
    feed_cursor: &mut u64,
) -> bool {
    let mut frames = match state.servecore_tmux_sessions() {
        Ok(sessions) => godui_ws_session_recent_frames(sessions),
        Err(error) => vec![godui_ws_error_frame(&error)],
    };
    let (events, next_cursor) = agentstatus_drain_feed(*feed_cursor);
    *feed_cursor = next_cursor;
    frames.extend(
        events
            .into_iter()
            .map(|event| godui_ws_json_text(&json!({"type": "feed", "event": event}))),
    );
    servecore_ws_send_text_frames(socket, frames, config).await
}

/// #880: same `{"type":"error","error":"tmux unreachable: …"}` shape the
/// engine `/ws/pty` socket already uses, so a dashboard has one error frame
/// to handle rather than one per stream.
fn godui_ws_error_frame(error: &str) -> String {
    godui_ws_json_text(&json!({"type": "error", "error": error}))
}

fn godui_costs_payload() -> Value {
    json!({
        "agents": [],
        "total": {
            "tokens": 0,
            "cost": 0.0,
            "sessions": 0,
            "agents": 0
        }
    })
}

fn godui_teams_payload(state: &ServecoreSharedState) -> Result<GoduiTeamsResponse, String> {
    let home = godui_home_dir();
    let panes = state.servecore_agents_panes()?;
    let teams = godui_scan_teams(
        &home.join(".claude").join("teams"),
        &home.join(".claude").join("tasks"),
        &home,
        &panes,
        godui_now_millis(),
    );
    Ok(GoduiTeamsResponse {
        total: teams.len(),
        teams,
    })
}

fn godui_ui_state_payload() -> Value {
    godui_read_json(&godui_current_dir_file("ui-state.json")).unwrap_or_else(|| json!({}))
}

fn godui_asks_payload() -> Value {
    godui_read_json(&godui_current_dir_file("asks.json")).unwrap_or_else(|| json!([]))
}

fn godui_pin_info_payload() -> GoduiPinInfoResponse {
    let config = maw_xdg::load_merged_config(&godui_xdg_env()).config;
    let pin = config
        .get("pin")
        .and_then(Value::as_str)
        .unwrap_or_default();
    GoduiPinInfoResponse {
        length: pin.chars().count(),
        enabled: !pin.is_empty(),
    }
}

pub(crate) fn godui_ws_initial_frames(sessions: Vec<TmuxSession>) -> (Vec<String>, u64) {
    let agent_sessions = agentstatus_sessions_from_tmux(&sessions);
    let snapshot = agentstatus_poll_global(&agent_sessions);
    let (events, cursor) = agentstatus_feed_history_and_cursor();
    let mut frames = Vec::with_capacity(3);
    frames.push(godui_ws_json_text(
        &json!({"type": "feed-history", "events": events}),
    ));
    frames.extend(godui_ws_session_recent_frames_with_snapshot(
        sessions, &snapshot,
    ));
    (frames, cursor)
}

pub(crate) fn godui_ws_session_recent_frames(sessions: Vec<TmuxSession>) -> Vec<String> {
    let agent_sessions = agentstatus_sessions_from_tmux(&sessions);
    let snapshot = agentstatus_poll_global(&agent_sessions);
    godui_ws_session_recent_frames_with_snapshot(sessions, &snapshot)
}

fn godui_ws_session_recent_frames_with_snapshot(
    sessions: Vec<TmuxSession>,
    snapshot: &AgentStatusSnapshot,
) -> Vec<String> {
    let sessions = godui_ws_sessions(sessions, snapshot);
    vec![
        godui_ws_json_text(&json!({"type": "sessions", "sessions": sessions})),
        godui_ws_json_text(
            &json!({"type": "recent", "agents": godui_ws_recent_agents(&sessions, snapshot)}),
        ),
    ]
}

fn godui_ws_json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned())
}

fn godui_ws_selected_target(text: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    matches!(
        value.get("type").and_then(Value::as_str),
        Some("subscribe" | "select")
    )
    .then(|| godui_ws_value_target(&value))
    .flatten()
}

fn godui_ws_message_target(text: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    godui_ws_value_target(&value)
}

fn godui_ws_value_target(value: &Value) -> Option<String> {
    let target = value.get("target").and_then(Value::as_str)?;
    super::websocket_routes::ws_validate_target(Some(target))
        .ok()
        .flatten()
}

fn godui_ws_preview_targets(text: &str) -> Option<BTreeSet<String>> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    (value.get("type").and_then(Value::as_str) == Some("subscribe-previews")).then(|| {
        value
            .get("targets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|target| {
                super::websocket_routes::ws_validate_target(Some(target))
                    .ok()
                    .flatten()
            })
            .collect()
    })
}

fn godui_ws_replace_preview_targets(
    current: &mut BTreeSet<String>,
    last_previews: &mut BTreeMap<String, String>,
    next: BTreeSet<String>,
) {
    *current = next;
    last_previews.retain(|target, _| current.contains(target));
}

fn godui_ws_capture_frame(target: &str) -> Option<String> {
    let content = serveengine_tmux_capture(target).ok()?;
    Some(godui_ws_json_text(
        &json!({"type":"capture","target":target,"content":content}),
    ))
}

fn godui_ws_preview_capture(target: &str) -> Option<String> {
    serveengine_tmux_capture_lines(target, Some(GODUI_PREVIEW_LINES)).ok()
}

fn godui_ws_previews_frame<F>(
    targets: &BTreeSet<String>,
    last_previews: &mut BTreeMap<String, String>,
    mut capture: F,
) -> Option<String>
where
    F: FnMut(&str) -> Option<String>,
{
    let mut data = Map::new();
    for target in targets {
        let Some(content) = capture(target) else {
            continue;
        };
        if last_previews.get(target) == Some(&content) {
            continue;
        }
        last_previews.insert(target.clone(), content.clone());
        data.insert(target.clone(), Value::String(content));
    }
    (!data.is_empty()).then(|| godui_ws_json_text(&json!({"type":"previews","data":data})))
}

async fn godui_store_json_body(req: Request<Body>, path: &Path) -> Response {
    let body = match to_bytes(req.into_body(), GODUI_POST_BODY_LIMIT).await {
        Ok(body) => body,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("body read failed: {error}")})),
            )
                .into_response()
        }
    };
    let payload = match serde_json::from_slice::<Value>(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("body must be valid json: {error}")})),
            )
                .into_response()
        }
    };
    match godui_write_json(path, &payload) {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
struct GoduiTeamsResponse {
    teams: Vec<Value>,
    total: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct GoduiPinInfoResponse {
    length: usize,
    enabled: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct GoduiWsSession {
    name: String,
    windows: Vec<GoduiWsWindow>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct GoduiWsWindow {
    index: u32,
    name: String,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct GoduiWsRecentAgent {
    target: String,
    name: String,
    session: String,
}

fn godui_scan_teams(
    teams_dir: &Path,
    tasks_root: &Path,
    home: &Path,
    panes: &[ServecoreAgentPane],
    now_ms: u64,
) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(teams_dir) else {
        return Vec::new();
    };
    let live_pane_ids = panes
        .iter()
        .map(|pane| pane.id.clone())
        .collect::<BTreeSet<_>>();
    let mut teams = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            godui_read_team(&entry.path(), tasks_root, home, &live_pane_ids, now_ms)
        })
        .collect::<Vec<_>>();
    teams.sort_by(|left, right| left.0.cmp(&right.0));
    teams.into_iter().map(|(_, team)| team).collect()
}

fn godui_read_team(
    team_dir: &Path,
    tasks_root: &Path,
    home: &Path,
    live_pane_ids: &BTreeSet<String>,
    now_ms: u64,
) -> Option<(String, Value)> {
    let name = team_dir.file_name()?.to_string_lossy().into_owned();
    let config = godui_read_json(&team_dir.join("config.json"))?;
    let mut object = config.as_object().cloned()?;
    object
        .entry("name".to_owned())
        .or_insert_with(|| Value::String(name.clone()));
    let team_name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&name)
        .to_owned();
    godui_normalize_team_members(&team_name, &mut object);
    let tasks = godui_read_tasks(&tasks_root.join(&name));
    let alive = godui_team_alive(object.get("members"), live_pane_ids, home, now_ms);
    object.insert("tasks".to_owned(), Value::Array(tasks));
    object.insert("alive".to_owned(), Value::Bool(alive));
    for field in ["description", "leadRepo", "leadSessionId"] {
        if godui_field_str(&object, field).is_none() {
            object.insert(field.to_owned(), Value::String(String::new()));
        }
    }
    Some((team_name, Value::Object(object)))
}

fn godui_normalize_team_members(team_name: &str, object: &mut Map<String, Value>) {
    let lead_agent_id = format!("team-lead@{team_name}");
    object.insert(
        "leadAgentId".to_owned(),
        Value::String(lead_agent_id.clone()),
    );
    let created_at = object
        .get("createdAt")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let lead_repo = godui_field_str(object, "leadRepo")
        .unwrap_or_default()
        .to_owned();
    let members = match object.remove("members") {
        Some(Value::Array(members)) => members,
        _ => Vec::new(),
    };
    let members = members
        .into_iter()
        .map(|member| {
            godui_normalize_team_member(team_name, &lead_agent_id, created_at, &lead_repo, member)
        })
        .collect();
    object.insert("members".to_owned(), Value::Array(members));
}

fn godui_normalize_team_member(
    team_name: &str,
    lead_agent_id: &str,
    created_at: u64,
    lead_repo: &str,
    member: Value,
) -> Value {
    let mut object = match member {
        Value::Object(object) => object,
        _ => Map::new(),
    };
    let name = godui_field_str(&object, "name")
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            godui_field_str(&object, "agentId")
                .and_then(|agent_id| agent_id.split_once('@').map(|(name, _)| name.to_owned()))
        })
        .unwrap_or_else(|| "member".to_owned());
    object.insert("name".to_owned(), Value::String(name.clone()));

    let agent_id = godui_field_str(&object, "agentId")
        .filter(|agent_id| !agent_id.is_empty())
        .map_or_else(|| format!("{name}@{team_name}"), str::to_owned);
    object.insert("agentId".to_owned(), Value::String(agent_id.clone()));

    let agent_type = godui_field_str(&object, "agentType")
        .filter(|agent_type| !agent_type.is_empty())
        .map_or_else(
            || {
                if agent_id == lead_agent_id || name == "team-lead" || name == "lead" {
                    "lead".to_owned()
                } else {
                    "member".to_owned()
                }
            },
            str::to_owned,
        );
    object.insert("agentType".to_owned(), Value::String(agent_type));

    if object.get("joinedAt").and_then(Value::as_u64).is_none() {
        object.insert("joinedAt".to_owned(), Value::from(created_at));
    }
    if godui_field_str(&object, "tmuxPaneId").is_none() {
        object.insert("tmuxPaneId".to_owned(), Value::String(String::new()));
    }
    let cwd = godui_field_str(&object, "cwd")
        .filter(|cwd| !cwd.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            godui_field_str(&object, "repo")
                .filter(|repo| !repo.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| (!lead_repo.is_empty()).then(|| lead_repo.to_owned()))
        .unwrap_or_default();
    object.insert("cwd".to_owned(), Value::String(cwd));
    if !object.get("subscriptions").is_some_and(Value::is_array) {
        object.insert("subscriptions".to_owned(), Value::Array(Vec::new()));
    }
    if godui_field_str(&object, "backendType").is_none() {
        object.insert(
            "backendType".to_owned(),
            Value::String("in-process".to_owned()),
        );
    }
    for field in ["model", "repo", "color"] {
        if godui_field_str(&object, field).is_none() {
            object.insert(field.to_owned(), Value::String(String::new()));
        }
    }
    Value::Object(object)
}

fn godui_read_tasks(tasks_dir: &Path) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(tasks_dir) else {
        return Vec::new();
    };
    let mut files = entries.filter_map(Result::ok).collect::<Vec<_>>();
    files.sort_by_key(std::fs::DirEntry::file_name);
    files
        .into_iter()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| godui_read_json(&entry.path()))
        .collect()
}

fn godui_team_alive(
    members: Option<&Value>,
    live_pane_ids: &BTreeSet<String>,
    home: &Path,
    now_ms: u64,
) -> bool {
    members.and_then(Value::as_array).is_some_and(|members| {
        members
            .iter()
            .any(|member| godui_member_alive(member, live_pane_ids, home, now_ms))
    })
}

fn godui_member_alive(
    member: &Value,
    live_pane_ids: &BTreeSet<String>,
    home: &Path,
    now_ms: u64,
) -> bool {
    let Some(member) = member.as_object() else {
        return false;
    };
    let backend = godui_field_str(member, "backendType");
    let cwd = godui_field_str(member, "cwd");
    let joined_at = member.get("joinedAt").and_then(Value::as_u64);
    if backend == Some("tmux")
        && godui_field_str(member, "tmuxPaneId").is_some_and(|pane| live_pane_ids.contains(pane))
    {
        return true;
    }
    if backend == Some("in-process") && godui_recent_local(cwd, joined_at, home, now_ms) {
        return true;
    }
    let agent_type = godui_field_str(member, "agentType");
    let name = godui_field_str(member, "name");
    (agent_type == Some("team-lead") || name == Some("team-lead"))
        && godui_recent_local(cwd, joined_at, home, now_ms)
}

fn godui_recent_local(cwd: Option<&str>, joined_at: Option<u64>, home: &Path, now_ms: u64) -> bool {
    let Some(cwd) = cwd else {
        return false;
    };
    if !Path::new(cwd).starts_with(home) {
        return false;
    }
    joined_at.is_some_and(|joined_at| now_ms.saturating_sub(joined_at) < GODUI_TEAM_RECENT_MS)
}

fn godui_field_str<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object.get(field).and_then(Value::as_str)
}

fn godui_read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn godui_write_json(path: &Path, payload: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(payload).unwrap_or_else(|_| "null".to_owned());
    fs::write(path, format!("{text}\n"))
}

fn godui_ws_sessions(
    sessions: Vec<TmuxSession>,
    snapshot: &AgentStatusSnapshot,
) -> Vec<GoduiWsSession> {
    sessions
        .into_iter()
        .map(|session| GoduiWsSession {
            windows: session
                .windows
                .into_iter()
                .map(|window| godui_ws_window(&session.name, window, snapshot))
                .collect(),
            name: session.name,
        })
        .collect()
}

fn godui_ws_window(
    session_name: &str,
    window: TmuxWindow,
    snapshot: &AgentStatusSnapshot,
) -> GoduiWsWindow {
    let target = format!("{}:{}", session_name, window.index);
    GoduiWsWindow {
        index: window.index,
        name: window.name,
        active: window.active,
        cwd: window.cwd,
        status: snapshot
            .agentstatus_status(&target)
            .map(std::borrow::ToOwned::to_owned),
    }
}

fn godui_ws_recent_agents(
    sessions: &[GoduiWsSession],
    snapshot: &AgentStatusSnapshot,
) -> Vec<GoduiWsRecentAgent> {
    sessions
        .iter()
        .flat_map(|session| {
            session.windows.iter().filter_map(|window| {
                let target = format!("{}:{}", session.name, window.index);
                snapshot
                    .agentstatus_is_agent_target(&target)
                    .then(|| GoduiWsRecentAgent {
                        target,
                        name: window.name.clone(),
                        session: session.name.clone(),
                    })
            })
        })
        .collect()
}

fn godui_current_dir_file(name: &str) -> PathBuf {
    if let Some(root) = std::env::var_os("MAW_GODUI_STATE_DIR") {
        return PathBuf::from(root).join(name);
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(name)
}

fn godui_home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn godui_xdg_env() -> maw_xdg::MawXdgEnv {
    let vars = [
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "MAW_STATE_DIR",
        "XDG_DATA_HOME",
        "MAW_DATA_DIR",
        "XDG_CACHE_HOME",
        "MAW_CACHE_DIR",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
    maw_xdg::MawXdgEnv::with_vars(godui_home_dir(), vars)
}

fn godui_now_millis() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{
        modules::servecore_mount_modules, servecore_apply_pipeline, servecore_mount_core_routes,
        servecore_with_shared_state,
    };
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
        Router,
    };
    use futures_util::StreamExt;
    use std::{
        net::{Ipv4Addr, SocketAddr},
        time::Duration,
    };
    use tokio::sync::oneshot;
    use tower::ServiceExt;

    // #757: was a private copy that serialized against nothing. Uses the
    // crate-wide guard, which holds the env lock for its lifetime.
    use crate::test_env::EnvVarGuard as EnvGuard;

    #[test]
    fn godui_empty_payloads_match_maw_js_shapes() {
        let costs = godui_costs_payload();
        assert_eq!(costs["agents"], json!([]));
        assert_eq!(costs["total"]["tokens"], 0);
        assert_eq!(costs["total"]["cost"], 0.0);
        assert_eq!(costs["total"]["sessions"], 0);
        assert_eq!(costs["total"]["agents"], 0);
    }

    #[test]
    fn godui_ws_frames_match_maw_js_sessions_and_recent_shapes() {
        let snapshot = AgentStatusSnapshot::default();
        let sessions = godui_ws_sessions(
            vec![TmuxSession {
                name: "142-athena".to_owned(),
                windows: vec![
                    TmuxWindow {
                        index: 1,
                        name: "athena-oracle".to_owned(),
                        active: true,
                        cwd: Some("/opt/athena".to_owned()),
                    },
                    TmuxWindow {
                        index: 2,
                        name: "athena-codex-1".to_owned(),
                        active: false,
                        cwd: None,
                    },
                ],
            }],
            &snapshot,
        );

        assert_eq!(
            serde_json::to_value(&sessions).expect("sessions json"),
            json!([{
                "name": "142-athena",
                "windows": [
                    {"index": 1, "name": "athena-oracle", "active": true, "cwd": "/opt/athena"},
                    {"index": 2, "name": "athena-codex-1", "active": false}
                ]
            }])
        );
        assert_eq!(
            serde_json::to_value(godui_ws_recent_agents(&sessions, &snapshot))
                .expect("recent json"),
            json!([])
        );
    }

    #[test]
    fn godui_ws_subscribe_target_drives_capture_frame() {
        let _guard = EnvGuard::set("MAW_RS_SERVECORE_FAKE_CAPTURE", "pane ansi");
        let target =
            godui_ws_selected_target(r#"{"type":"subscribe","target":"demo:1"}"#).expect("target");
        let frame = godui_ws_capture_frame(&target).expect("capture frame");
        let value = serde_json::from_str::<Value>(&frame).expect("json");
        assert_eq!(value["type"], "capture");
        assert_eq!(value["target"], "demo:1");
        assert_eq!(value["content"], "pane ansi");
    }

    #[test]
    fn godui_ws_previews_frame_sends_only_changed_targets() {
        let targets = BTreeSet::from(["demo:1".to_owned(), "demo:2".to_owned()]);
        let mut last_previews = BTreeMap::new();
        let frame = godui_ws_previews_frame(&targets, &mut last_previews, |target| {
            Some(format!("{target} pane \u{1b}[31mansi\u{1b}[0m"))
        })
        .expect("initial preview frame");
        let value = serde_json::from_str::<Value>(&frame).expect("json");
        assert_eq!(value["type"], "previews");
        assert_eq!(
            value["data"]["demo:1"],
            "demo:1 pane \u{1b}[31mansi\u{1b}[0m"
        );
        assert_eq!(
            value["data"]["demo:2"],
            "demo:2 pane \u{1b}[31mansi\u{1b}[0m"
        );

        assert!(
            godui_ws_previews_frame(&targets, &mut last_previews, |target| {
                Some(format!("{target} pane \u{1b}[31mansi\u{1b}[0m"))
            })
            .is_none()
        );

        let changed = godui_ws_previews_frame(&targets, &mut last_previews, |target| {
            Some(if target == "demo:2" {
                "demo:2 updated".to_owned()
            } else {
                format!("{target} pane \u{1b}[31mansi\u{1b}[0m")
            })
        })
        .expect("changed preview frame");
        let changed = serde_json::from_str::<Value>(&changed).expect("json");
        assert!(changed["data"].get("demo:1").is_none());
        assert_eq!(changed["data"]["demo:2"], "demo:2 updated");
    }

    #[test]
    fn godui_ws_subscribe_previews_replaces_targets_and_prunes_last_sent() {
        let mut targets = godui_ws_preview_targets(
            r#"{"type":"subscribe-previews","targets":["demo:1","demo:2"]}"#,
        )
        .expect("preview targets");
        let mut last_previews = BTreeMap::from([
            ("demo:1".to_owned(), "old 1".to_owned()),
            ("demo:2".to_owned(), "old 2".to_owned()),
        ]);

        let next =
            godui_ws_preview_targets(r#"{"type":"subscribe-previews","targets":["demo:2"]}"#)
                .expect("replacement targets");
        godui_ws_replace_preview_targets(&mut targets, &mut last_previews, next);

        assert_eq!(targets, BTreeSet::from(["demo:2".to_owned()]));
        assert_eq!(
            last_previews,
            BTreeMap::from([("demo:2".to_owned(), "old 2".to_owned())])
        );
    }

    #[tokio::test]
    async fn godui_ws_route_streams_sessions_and_recent_from_module() {
        super::super::agent_status::agentstatus_reset_global();
        // #889: make the former cross-test race deterministic by priming the
        // throttled global with a snapshot for a different session shape.
        let stale_sessions =
            super::super::agent_status::agentstatus_sessions_from_tmux(&[TmuxSession {
                name: "99-stale".to_owned(),
                windows: vec![maw_tmux::TmuxWindow {
                    index: 1,
                    name: "stale-oracle".to_owned(),
                    active: true,
                    cwd: None,
                }],
            }]);
        let _stale_snapshot = super::super::agent_status::agentstatus_poll_global(&stale_sessions);
        let state = ServecoreSharedState::default().servecore_with_tmux_sessions_snapshot(vec![
            TmuxSession {
                name: "142-athena".to_owned(),
                windows: vec![
                    maw_tmux::TmuxWindow {
                        index: 1,
                        name: "athena-oracle".to_owned(),
                        active: true,
                        cwd: Some("/opt/athena".to_owned()),
                    },
                    maw_tmux::TmuxWindow {
                        index: 2,
                        name: "athena-codex-1".to_owned(),
                        active: false,
                        cwd: None,
                    },
                ],
            },
        ]);
        let addr = godui_spawn_test_server(state).await;
        let (mut ws, _response) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .expect("connect websocket");
        let mut frames = Vec::new();
        while frames.len() < 3 {
            let received = ws.next().await.expect("frame").expect("frame ok");
            if let tokio_tungstenite::tungstenite::Message::Text(text) = received {
                frames.push(serde_json::from_str::<serde_json::Value>(&text).expect("json frame"));
            }
        }

        assert_eq!(frames[0]["type"], "feed-history");
        assert_eq!(frames[0]["events"], json!([]));
        assert_eq!(frames[1]["type"], "sessions");
        assert_eq!(frames[1]["sessions"][0]["name"], "142-athena");
        assert_eq!(frames[1]["sessions"][0]["windows"][0]["cwd"], "/opt/athena");
        assert_eq!(frames[1]["sessions"][0]["windows"][0]["status"], "idle");
        assert_eq!(frames[2]["type"], "recent");
        assert_eq!(frames[2]["agents"], json!([]));
    }

    /// #880: `/api/teams` reads team configs off disk but decides each team's
    /// `alive` flag from the live tmux pane ids. With tmux unreachable the
    /// pane set was empty, so every team was reported `alive: false` -- a
    /// confident "your whole team is dead" built on a read that never
    /// happened. It now answers 503 with the typed body instead.
    #[tokio::test]
    async fn godui880_teams_unreachable_tmux_is_503_not_a_dead_team_roster() {
        let state = ServecoreSharedState::default().servecore_with_tmux_unreachable(
            "error connecting to /tmp/tmux-1028/default (No such file or directory)",
        );
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_mount_modules(router, &["god-ui".to_owned()]);
        let router = servecore_with_shared_state(router, state);
        let app = servecore_apply_pipeline(router);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/teams")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "liveness that cannot be read must not be reported as 'not alive'"
        );
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        let payload = serde_json::from_slice::<Value>(&body).expect("json body");
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or_default()
                .contains("tmux unreachable"),
            "503 body must carry the typed tmux-unreachable error: {payload}"
        );
        assert!(
            payload.get("teams").is_none(),
            "a failed liveness read must not also ship a team roster: {payload}"
        );
    }

    /// #880, websocket half. A websocket cannot answer 503 -- by the time the
    /// stream runs, the HTTP handshake is already done. The wire analog is a
    /// typed `{"type":"error"}` frame followed by a close: the dashboard
    /// learns tmux is unreachable instead of painting a confidently empty
    /// fleet, which is exactly what the old `sessions: []` frame did.
    #[tokio::test]
    async fn godui880_ws_initial_unreachable_tmux_sends_error_frame_and_never_empty_sessions() {
        let state = ServecoreSharedState::default().servecore_with_tmux_unreachable(
            "error connecting to /tmp/tmux-1028/default (No such file or directory)",
        );
        let addr = godui_spawn_test_server(state).await;
        let (mut ws, _response) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .expect("connect websocket");

        // Bounded by frame count, not wall clock, so the failure output is the
        // same on every run: the broken path emits exactly the initial three
        // frames plus one refresh tick's two before this stops reading.
        let mut texts = Vec::new();
        let mut closed = false;
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(Ok(received)) = ws.next().await {
                match received {
                    tokio_tungstenite::tungstenite::Message::Text(text) => {
                        texts.push(serde_json::from_str::<Value>(&text).expect("json frame"));
                        if texts.len() >= 5 {
                            return;
                        }
                    }
                    tokio_tungstenite::tungstenite::Message::Close(_) => {
                        closed = true;
                        return;
                    }
                    _ => {}
                }
            }
        })
        .await;

        assert!(
            texts.iter().any(|frame| frame["type"] == "error"
                && frame["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("tmux unreachable")),
            "the dashboard must be told tmux is unreachable: {texts:?}"
        );
        assert!(
            !texts.iter().any(|frame| frame["type"] == "sessions"),
            "an unreachable tmux must never be streamed as a session list: {texts:?}"
        );
        assert!(
            closed,
            "a connect-time tmux failure has no recoverable stream to serve; close it"
        );
    }

    /// #880, the harder websocket half: tmux was alive at connect and dies
    /// while the dashboard socket is open. This tick is a best-effort poll on
    /// an already-established connection, so it deliberately does NOT tear the
    /// socket down -- but it must not push `sessions: []` either, because that
    /// is the #860 lie arriving every two seconds. It emits the typed error
    /// frame, skips the sessions frames for that tick, and stays open to
    /// recover.
    #[tokio::test]
    async fn godui880_ws_refresh_tmux_dying_mid_stream_errors_without_closing_or_lying() {
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&reads);
        let state = ServecoreSharedState::default().servecore_with_tmux_sessions_source(Arc::new(
            move || {
                if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Ok(Vec::new())
                } else {
                    Err("tmux unreachable: server exited unexpectedly".to_owned())
                }
            },
        ));
        let addr = godui_spawn_test_server(state).await;
        let (mut ws, _response) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .expect("connect websocket");

        // Stops after two refresh ticks rather than after a wall-clock window,
        // so both the broken and the fixed path produce the same counts on
        // every run.
        let mut initial_sessions_frame = false;
        let mut refreshed_session_frames = 0_usize;
        let mut error_frames = 0_usize;
        let mut ticks = 0_usize;
        let mut closed = false;
        let _ = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(Ok(received)) = ws.next().await {
                match received {
                    tokio_tungstenite::tungstenite::Message::Text(text) => {
                        let frame = serde_json::from_str::<Value>(&text).expect("json frame");
                        if frame["type"] == "sessions" {
                            if initial_sessions_frame {
                                refreshed_session_frames += 1;
                                ticks += 1;
                            } else {
                                initial_sessions_frame = true;
                            }
                        }
                        if frame["type"] == "error" {
                            error_frames += 1;
                            ticks += 1;
                        }
                        if ticks >= 2 {
                            return;
                        }
                    }
                    tokio_tungstenite::tungstenite::Message::Close(_) => {
                        closed = true;
                        return;
                    }
                    _ => {}
                }
            }
        })
        .await;

        assert!(
            initial_sessions_frame,
            "the first, reachable read must still stream its (empty but true) session list"
        );
        assert_eq!(
            refreshed_session_frames, 0,
            "a refresh whose tmux read failed must never be streamed as an empty session list"
        );
        assert_eq!(
            error_frames, 2,
            "each failed refresh tick must say so, and keep saying so"
        );
        assert!(
            !closed,
            "a transient tmux failure must not tear down a working dashboard socket"
        );
    }

    #[test]
    fn godui_scan_teams_merges_config_tasks_and_liveness() {
        let root = godui_test_root("teams");
        let home = root.join("home");
        let teams_dir = home.join(".claude/teams");
        let tasks_root = home.join(".claude/tasks");
        fs::create_dir_all(teams_dir.join("alpha")).expect("team dir");
        fs::create_dir_all(tasks_root.join("alpha")).expect("tasks dir");
        fs::write(
            teams_dir.join("alpha/config.json"),
            json!({
                "name": "alpha",
                "description": "demo",
                "members": [
                    {"name": "builder", "backendType": "tmux", "tmuxPaneId": "%9"}
                ]
            })
            .to_string(),
        )
        .expect("config");
        fs::write(
            tasks_root.join("alpha/1.json"),
            json!({"id": 1, "status": "in_progress"}).to_string(),
        )
        .expect("task");
        let panes = vec![ServecoreAgentPane {
            id: "%9".to_owned(),
            command: "codex".to_owned(),
            target: "alpha:1.0".to_owned(),
            title: "builder-agent".to_owned(),
            cwd: Some(home.to_string_lossy().into_owned()),
            pid: Some(99),
            last_activity: Some(1),
        }];

        let teams = godui_scan_teams(&teams_dir, &tasks_root, &home, &panes, 1_000);

        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0]["name"], "alpha");
        assert_eq!(teams[0]["tasks"][0]["id"], 1);
        assert_eq!(teams[0]["alive"], true);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn godui_scan_teams_normalizes_bare_members_for_team_panel() {
        let root = godui_test_root("bare-members");
        let home = root.join("home");
        let teams_dir = home.join(".claude/teams");
        let tasks_root = home.join(".claude/tasks");
        fs::create_dir_all(teams_dir.join("alpha")).expect("team dir");
        fs::write(
            teams_dir.join("alpha/config.json"),
            json!({
                "name": "alpha",
                "createdAt": 42,
                "leadRepo": "/opt/alpha",
                "members": [
                    {"model": "sonnet", "name": "builder"}
                ]
            })
            .to_string(),
        )
        .expect("config");

        let teams = godui_scan_teams(&teams_dir, &tasks_root, &home, &[], 1_000);

        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0]["leadAgentId"], "team-lead@alpha");
        let serialized = serde_json::to_string(&teams[0]["members"][0]).expect("member serializes");
        let member: Value = serde_json::from_str(&serialized).expect("member json");
        let member = member.as_object().expect("member object");
        for key in [
            "agentId",
            "name",
            "agentType",
            "joinedAt",
            "tmuxPaneId",
            "cwd",
            "subscriptions",
            "backendType",
        ] {
            assert!(member.contains_key(key), "{key}");
        }
        assert_eq!(member["agentId"], "builder@alpha");
        assert_eq!(member["name"], "builder");
        assert_eq!(member["agentType"], "member");
        assert_eq!(member["joinedAt"], 42);
        assert_eq!(member["tmuxPaneId"], "");
        assert_eq!(member["cwd"], "/opt/alpha");
        assert_eq!(member["subscriptions"], json!([]));
        assert_eq!(member["backendType"], "in-process");
        assert_eq!(member["model"], "sonnet");
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn godui_routes_return_200_json_and_cors_headers() {
        let state = ServecoreSharedState::default().servecore_with_agents_snapshot(Vec::new());
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_mount_modules(router, &["god-ui".to_owned()]);
        let router = servecore_with_shared_state(router, state);
        let app = servecore_apply_pipeline(router);
        let endpoints = [
            "/api/costs",
            "/api/teams",
            "/api/ui-state",
            "/api/asks",
            "/api/pin-info",
        ];

        for endpoint in endpoints {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri(endpoint)
                        .header("origin", "https://god.buildwithoracle.com")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK, "{endpoint}");
            assert_eq!(
                response.headers().get("access-control-allow-origin"),
                Some(&"https://god.buildwithoracle.com".parse().expect("origin")),
                "{endpoint}"
            );
            assert!(response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("application/json")));
        }
    }

    #[tokio::test]
    async fn godui_post_routes_persist_json_and_return_ok() {
        let root = godui_test_root("post");
        fs::create_dir_all(&root).expect("root");
        let original_state_dir = std::env::var_os("MAW_GODUI_STATE_DIR");
        std::env::set_var("MAW_GODUI_STATE_DIR", &root);
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_mount_modules(router, &["god-ui".to_owned()]);
        let router = servecore_with_shared_state(router, ServecoreSharedState::default());
        let app = servecore_apply_pipeline(router);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/ui-state")
                    .header("origin", "https://god.buildwithoracle.com")
                    .body(Body::from(r#"{"mission":"live"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(godui_ui_state_payload(), json!({"mission": "live"}));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/asks")
                    .header("origin", "https://god.buildwithoracle.com")
                    .body(Body::from(r#"[{"id":1}]"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(godui_asks_payload(), json!([{"id": 1}]));

        if let Some(original_state_dir) = original_state_dir {
            std::env::set_var("MAW_GODUI_STATE_DIR", original_state_dir);
        } else {
            std::env::remove_var("MAW_GODUI_STATE_DIR");
        }
        fs::remove_dir_all(root).ok();
    }

    fn godui_test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_nanos();
        std::env::temp_dir().join(format!(
            "maw-rs-god-ui-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    async fn godui_spawn_test_server(state: ServecoreSharedState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = servecore_mount_core_routes(Router::new());
        let router = servecore_mount_modules(router, &["god-ui".to_owned()]);
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
}
