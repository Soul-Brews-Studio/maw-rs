fn trust_entries_json(entries: &[TrustEntryPlan]) -> Vec<Value> {
    let mut rows = entries.to_vec();
    rows.sort_by(|left, right| left.added_at.cmp(&right.added_at));
    rows.into_iter()
        .map(|entry| {
            json!({
                "sender": entry.sender,
                "target": entry.target,
                "addedAt": entry.added_at,
                "peerKey": if entry.peer_key.is_some() { "received (redacted)" } else { "missing" }
            })
        })
        .collect()
}

fn trust_outcome_state(outcome: &TrustWriteOutcome) -> &'static str {
    match outcome {
        TrustWriteOutcome::Added => "trusted",
        TrustWriteOutcome::AlreadyTrusted => "already-trusted",
        TrustWriteOutcome::UpdatedPin => "pin-updated",
    }
}

fn trust_http_error(status: StatusCode, message: &str) -> axum::response::Response {
    (status, Json(json!({"ok": false, "error": message}))).into_response()
}

fn unix_millis_i64() -> i64 {
    i64::try_from(unix_millis()).unwrap_or(i64::MAX)
}

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

async fn api_not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"})))
}

