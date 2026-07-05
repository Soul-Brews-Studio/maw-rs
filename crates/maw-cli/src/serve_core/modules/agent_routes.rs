use super::ServecoreModuleRegistration;
use crate::serve_core::{ServecoreAgentPane, ServecoreLifecycleModule, ServecoreSharedState};
use axum::{
    extract::Query, http::StatusCode, response::IntoResponse, routing::get, Extension, Json, Router,
};
use serde::Serialize;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

#[must_use]
pub fn agents_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "agents".to_owned(),
        weight: 50,
    }
}

#[must_use]
pub fn agents_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: agents_lifecycle_module(),
        mount: agents_mount,
    }
}

pub fn agents_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/api/agents", get(agents_get))
        .route("/api/agent", get(agents_get))
}

async fn agents_get(
    Extension(state): Extension<Arc<ServecoreSharedState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> impl IntoResponse {
    match agents_parse_query(&query) {
        Ok(options) => {
            let panes = state.servecore_agents_panes();
            let agents = agents_render(&panes, state.agents_node.as_deref(), options.all);
            Json(json!({"agents": agents, "count": agents.len(), "node": state.agents_node}))
                .into_response()
        }
        Err(message) => (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AgentsQuery {
    all: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct AgentsEntry {
    id: String,
    target: String,
    title: String,
    command: String,
    cwd: Option<String>,
    pid: Option<u32>,
    last_activity: Option<u64>,
    node: Option<String>,
}

fn agents_parse_query(query: &BTreeMap<String, String>) -> Result<AgentsQuery, String> {
    let mut all = false;
    for (key, value) in query {
        agents_guard_query_part(key, "query key")?;
        agents_guard_query_part(value, key)?;
        match key.as_str() {
            "all" => all = agents_parse_bool(value)?,
            other => return Err(format!("serve-agents: unknown query parameter {other}")),
        }
    }
    Ok(AgentsQuery { all })
}

fn agents_parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err("serve-agents: all must be boolean".to_owned()),
    }
}

fn agents_guard_query_part(value: &str, label: &str) -> Result<(), String> {
    if value == "--" || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("serve-agents: {label} is not allowed"));
    }
    Ok(())
}

fn agents_render(panes: &[ServecoreAgentPane], node: Option<&str>, all: bool) -> Vec<AgentsEntry> {
    let mut agents = panes
        .iter()
        .filter(|pane| all || agents_is_agent_pane(pane))
        .map(|pane| agents_entry(pane, node))
        .collect::<Vec<_>>();
    agents.sort_by(|left, right| left.target.cmp(&right.target).then(left.id.cmp(&right.id)));
    agents
}

fn agents_is_agent_pane(pane: &ServecoreAgentPane) -> bool {
    let title = pane.title.to_ascii_lowercase();
    let command = pane.command.to_ascii_lowercase();
    title.contains("agent")
        || title.contains("oracle")
        || title.contains("codex")
        || title.contains("claude")
        || command.contains("codex")
        || command.contains("claude")
}

fn agents_entry(pane: &ServecoreAgentPane, node: Option<&str>) -> AgentsEntry {
    AgentsEntry {
        id: pane.id.clone(),
        target: pane.target.clone(),
        title: pane.title.clone(),
        command: pane.command.clone(),
        cwd: pane.cwd.clone(),
        pid: pane.pid,
        last_activity: pane.last_activity,
        node: node.map(ToOwned::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{servecore_apply_pipeline, servecore_with_shared_state};
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;

    async fn agents_spawn(state: ServecoreSharedState) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = servecore_with_shared_state(agents_mount(Router::new()), state);
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

    fn agents_pane(id: &str, target: &str, title: &str, command: &str) -> ServecoreAgentPane {
        ServecoreAgentPane {
            id: id.to_owned(),
            command: command.to_owned(),
            target: target.to_owned(),
            title: title.to_owned(),
            cwd: Some("/tmp/repo".to_owned()),
            pid: Some(42),
            last_activity: Some(123),
        }
    }

    #[test]
    fn agents_query_guards_option_injection_and_bool_values() {
        let mut query = BTreeMap::new();
        query.insert("all".to_owned(), "1".to_owned());
        assert_eq!(
            agents_parse_query(&query).expect("query"),
            AgentsQuery { all: true }
        );
        query.insert("all".to_owned(), "--".to_owned());
        assert!(agents_parse_query(&query)
            .expect_err("guard")
            .contains("all"));
    }

    #[test]
    fn agents_render_filters_agent_panes_unless_all_requested() {
        let panes = vec![
            agents_pane("%1", "s:0.0", "nova-agent", "bash"),
            agents_pane("%2", "s:0.1", "logs", "tail"),
        ];
        assert_eq!(agents_render(&panes, Some("node-a"), false).len(), 1);
        assert_eq!(agents_render(&panes, Some("node-a"), true).len(), 2);
    }

    #[tokio::test]
    async fn agents_real_wire_is_public_and_uses_fake_state() {
        let state = ServecoreSharedState::default()
            .servecore_with_agents_node(Some("node-a".to_owned()))
            .servecore_with_agents_snapshot(vec![agents_pane(
                "%1",
                "s:0.0",
                "nova-agent",
                "codex",
            )]);
        let addr = agents_spawn(state).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/api/agents"))
            .send()
            .await
            .expect("agents");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<serde_json::Value>().await.expect("json");
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["node"], "node-a");
        assert_eq!(payload["agents"][0]["target"], "s:0.0");
    }
}
