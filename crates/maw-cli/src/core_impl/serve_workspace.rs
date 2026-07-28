// Shared workspaces: several agents, possibly on different nodes, in one room.
//
// A workspace is created with a key, joined by agents who then post status and
// messages into a common feed. Unlike the federation routes these are authorized
// by the workspace's own key plus a timestamp window rather than by peer pubkey,
// because a member is not necessarily a known fleet peer.

async fn api_workspace_create(
    State(state): State<Arc<ServeState>>,
    Json(body): Json<WorkspaceCreateBody>,
) -> impl IntoResponse {
    let workspace = Workspace::new(body.name, body.node_id);
    let response = json!({
        "id": workspace.id,
        "token": workspace.token,
        "joinCode": workspace.join_code,
        "joinCodeExpiresAt": workspace.join_code_expires_at,
    });
    with_workspace_store(&state, |store| {
        store.join_codes.insert(workspace.join_code.clone(), workspace.id.clone());
        store.workspaces.insert(workspace.id.clone(), workspace);
    });
    Json(response).into_response()
}

async fn api_workspace_join(
    State(state): State<Arc<ServeState>>,
    Json(body): Json<WorkspaceJoinBody>,
) -> impl IntoResponse {
    with_workspace_store(&state, |store| {
        let Some(workspace_id) = store.join_codes.get(&body.code).cloned() else {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response();
        };
        let Some(workspace) = store.workspaces.get_mut(&workspace_id) else {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response();
        };
        workspace.nodes.insert(body.node_id);
        Json(json!({
            "workspaceId": workspace.id,
            "token": workspace.token,
            "name": workspace.name,
        }))
        .into_response()
    })
}

async fn api_workspace_agents_post(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    let agent = serde_json::from_slice::<WorkspaceAgentBody>(&body).unwrap_or_default();
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get_mut(&id) else {
            return workspace_not_found();
        };
        if !agent.node_id.is_empty() {
            workspace.nodes.insert(agent.node_id.clone());
        }
        if !agent.name.is_empty() {
            workspace.agents.insert(
                agent_key(&agent.node_id, &agent.name),
                WorkspaceAgent {
                    name: agent.name,
                    node_id: agent.node_id,
                    status: agent.status,
                    capabilities: agent.capabilities,
                },
            );
        }
        Json(json!({"ok": true, "agents": workspace.agents.len()})).into_response()
    })
}

async fn api_workspace_agents_get(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get(&id) else {
            return workspace_not_found();
        };
        let agents = workspace
            .agents
            .values()
            .map(|agent| {
                json!({
                    "name": agent.name,
                    "nodeId": agent.node_id,
                    "status": agent.status,
                    "capabilities": agent.capabilities,
                })
            })
            .collect::<Vec<_>>();
        Json(json!({"agents": agents, "total": workspace.agents.len()})).into_response()
    })
}

async fn api_workspace_status(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get(&id) else {
            return workspace_not_found();
        };
        Json(json!({
            "id": workspace.id,
            "name": workspace.name,
            "createdAt": workspace.created_at,
            "nodes": workspace.nodes.iter().cloned().collect::<Vec<_>>(),
            "nodeCount": workspace.nodes.len(),
            "healthyNodes": workspace.nodes.len(),
            "agents": workspace.agents.values().map(|agent| json!({"name": agent.name, "nodeId": agent.node_id, "status": agent.status, "capabilities": agent.capabilities})).collect::<Vec<_>>(),
            "agentCount": workspace.agents.len(),
            "feedCount": workspace.feed.len(),
        }))
        .into_response()
    })
}

async fn api_workspace_feed(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<WorkspaceFeedQuery>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get(&id) else {
            return workspace_not_found();
        };
        let limit = query.limit.unwrap_or(workspace.feed.len());
        let start = workspace.feed.len().saturating_sub(limit);
        Json(json!({"events": workspace.feed[start..].to_vec(), "total": workspace.feed.len()}))
            .into_response()
    })
}

async fn api_workspace_message(
    State(state): State<Arc<ServeState>>,
    AxumPath(id): AxumPath<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_workspace_request(&state, &id, &method, &uri, &headers) {
        return response;
    }
    let message = serde_json::from_slice::<WorkspaceMessageBody>(&body).unwrap_or_default();
    with_workspace_store(&state, |store| {
        let Some(workspace) = store.workspaces.get_mut(&id) else {
            return workspace_not_found();
        };
        workspace.feed.push(json!({
            "from": message.from,
            "text": message.text,
            "to": message.to,
            "timestamp": unix_seconds(),
        }));
        Json(json!({"ok": true})).into_response()
    })
}

fn verify_workspace_request(
    state: &ServeState,
    id: &str,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
) -> Option<axum::response::Response> {
    with_workspace_store(state, |store| {
        let Some(workspace) = store.workspaces.get(id) else {
            return Some(workspace_not_found());
        };
        let timestamp = header_to_string(headers, "x-maw-timestamp");
        let signature = header_to_string(headers, "x-maw-signature");
        let Some(signed_at) = parse_workspace_timestamp(&timestamp) else {
            return Some(workspace_auth_failed());
        };
        let now = verify_now(state);
        if (now - signed_at).abs() > 300 {
            return Some(workspace_auth_failed());
        }
        let payload = format!("{}:{}:{}", method.as_str(), uri.path(), timestamp);
        if maw_auth::verify_hmac_sig(&workspace.token, &payload, &signature) {
            None
        } else {
            Some(workspace_auth_failed())
        }
    })
}

fn parse_workspace_timestamp(timestamp: &str) -> Option<i64> {
    if timestamp.chars().all(|ch| ch.is_ascii_digit()) {
        timestamp.parse().ok()
    } else {
        None
    }
}

fn workspace_auth_failed() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized"})),
    )
        .into_response()
}

fn workspace_not_found() -> axum::response::Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
}

fn with_workspace_store<T>(state: &ServeState, op: impl FnOnce(&mut WorkspaceStore) -> T) -> T {
    let mut guard = state
        .workspaces
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    op(&mut guard)
}

fn agent_key(node_id: &str, name: &str) -> String {
    format!("{node_id}:{name}")
}

#[derive(Default)]
struct WorkspaceStore {
    workspaces: HashMap<String, Workspace>,
    join_codes: HashMap<String, String>,
}

struct Workspace {
    id: String,
    name: String,
    token: String,
    join_code: String,
    join_code_expires_at: u64,
    created_at: u64,
    nodes: HashSet<String>,
    agents: HashMap<String, WorkspaceAgent>,
    feed: Vec<Value>,
}

impl Workspace {
    fn new(name: String, node_id: String) -> Self {
        let created_at = unix_millis();
        let mut nodes = HashSet::new();
        nodes.insert(node_id);
        Self {
            id: format!("ws-{}", random_hex(8)),
            name,
            token: random_hex(32),
            join_code: random_hex(3),
            join_code_expires_at: created_at.saturating_add(15 * 60 * 1_000),
            created_at,
            nodes,
            agents: HashMap::new(),
            feed: Vec::new(),
        }
    }
}

struct WorkspaceAgent {
    name: String,
    node_id: String,
    status: Option<String>,
    capabilities: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCreateBody {
    name: String,
    node_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceJoinBody {
    code: String,
    node_id: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceAgentBody {
    name: String,
    node_id: String,
    status: Option<String>,
    capabilities: Option<Value>,
}

#[derive(Default, Deserialize)]
struct WorkspaceMessageBody {
    from: String,
    text: String,
    to: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceFeedQuery {
    limit: Option<usize>,
}
