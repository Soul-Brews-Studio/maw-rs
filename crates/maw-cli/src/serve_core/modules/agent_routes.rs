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
            // #880: PROPAGATE. This endpoint's entire product is the pane
            // listing, so an unreachable tmux has nothing truthful to return.
            // 503 with the typed body `/api/sessions` established lets a
            // client distinguish "no agents are running" from "I could not
            // look" -- the distinction #860 was filed over.
            let panes = match state.servecore_agents_panes() {
                Ok(panes) => panes,
                Err(error) => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error": error})),
                    )
                        .into_response()
                }
            };
            let total = panes.len();
            let agents = agents_render(&panes, state.agents_node.as_deref(), options.all);
            Json(json!({
                "agents": agents,
                "count": agents.len(),
                "total": total,
                "filtered_by": if options.all {
                    "none (?all=1)"
                } else {
                    "title/command heuristic (agent|oracle|codex|claude substring, or a semver-shaped command); pass ?all=1 for every pane on the node"
                },
                "node": state.agents_node,
            }))
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

/// A live Claude Code pane reports its own version string as the tmux pane
/// command (e.g. `2.1.219`), not a process name — the title carries "Claude
/// Code" only while idle and switches to a task-status line while busy (#520
/// gotcha). #813 moved that shape check, and the title keywords, into the one
/// shared `maw_tmux::is_agent_pane` so this listing and the `hey` guards can
/// no longer drift apart.
pub(crate) fn agents_is_agent_pane(pane: &ServecoreAgentPane) -> bool {
    maw_tmux::is_agent_pane(Some(&pane.command), Some(&pane.title))
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

    /// #813: the local `agents_command_is_versioned_binary` is gone; the same
    /// property now belongs to the shared version arm. Every input and every
    /// expected verdict below is unchanged -- only the function under test
    /// moved, so this keeps guarding the shape from its new home.
    #[test]
    fn agents_command_is_versioned_binary_matches_semver_only() {
        assert!(maw_tmux::is_claude_like_pane(Some("2.1.219")));
        assert!(maw_tmux::is_claude_like_pane(Some("10.0.1")));
        assert!(!maw_tmux::is_claude_like_pane(Some("codex")));
        assert!(!maw_tmux::is_claude_like_pane(Some("bash")));
        assert!(!maw_tmux::is_claude_like_pane(Some("2.1")));
        assert!(!maw_tmux::is_claude_like_pane(Some("")));
        assert!(!maw_tmux::is_claude_like_pane(Some("2..1")));
        assert!(!maw_tmux::is_claude_like_pane(Some("v2.1.219")));
    }

    #[test]
    fn agents_render_keeps_busy_pane_whose_title_lost_the_claude_keyword() {
        // #690 repro shape: an ACTIVE Claude Code pane replaces its idle
        // "✳ Claude Code" title with a live task-status line while it works,
        // but its pane command still reports the app version. The node's own
        // most-active session must not be the one silently dropped.
        let panes = vec![
            agents_pane(
                "%1",
                "33-maw-rs:maw-rs.0",
                "⠐ maw-js to maw-rs incremental migration planning",
                "2.1.219",
            ),
            agents_pane("%2", "01-kalyana:kalyana.0", "✳ Claude Code", "2.1.219"),
        ];
        let rendered = agents_render(&panes, Some("maw-rs-black"), false);
        assert_eq!(rendered.len(), 2, "{rendered:?}");
        assert!(rendered
            .iter()
            .any(|entry| entry.target == "33-maw-rs:maw-rs.0"));
    }

    #[tokio::test]
    async fn agents_real_wire_is_public_and_uses_fake_state() {
        let state = ServecoreSharedState::default()
            .servecore_with_agents_node(Some("node-a".to_owned()))
            .servecore_with_agents_snapshot(vec![
                agents_pane("%1", "s:0.0", "nova-agent", "codex"),
                agents_pane("%2", "s:0.1", "logs", "tail"),
            ]);
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
        assert_eq!(payload["total"], 2);
        assert!(
            payload["filtered_by"]
                .as_str()
                .expect("filtered_by")
                .contains("all=1"),
            "{payload}"
        );
        assert_eq!(payload["node"], "node-a");
        assert_eq!(payload["agents"][0]["target"], "s:0.0");

        let unfiltered = client
            .get(format!("http://{addr}/api/agents?all=1"))
            .send()
            .await
            .expect("agents all");
        let unfiltered_payload = unfiltered.json::<serde_json::Value>().await.expect("json");
        assert_eq!(unfiltered_payload["count"], 2);
        assert_eq!(unfiltered_payload["total"], 2);
        assert_eq!(unfiltered_payload["filtered_by"], "none (?all=1)");
    }

    /// #880: inside `maw serve`, an unreachable tmux used to render as
    /// `{"agents":[],"count":0,"total":0}` -- HTTP 200, indistinguishable
    /// from a node that genuinely has no agents. That is the #860 defect on
    /// the daemon surface. `/api/agents` now answers 503 with the same typed
    /// body `/api/sessions` uses, so a client can tell "nothing running" from
    /// "I cannot see".
    #[tokio::test]
    async fn agents880_unreachable_tmux_is_503_not_an_empty_agent_list() {
        let state = ServecoreSharedState::default()
            .servecore_with_agents_node(Some("node-a".to_owned()))
            .servecore_with_tmux_unreachable(
                "error connecting to /tmp/tmux-1028/default (No such file or directory)",
            );
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

        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "an unreachable tmux must not be reported as a successful empty listing"
        );
        let payload = response.json::<serde_json::Value>().await.expect("json");
        let error = payload["error"].as_str().unwrap_or_default();
        assert!(
            error.contains("tmux unreachable"),
            "503 body must carry the typed tmux-unreachable error: {payload}"
        );
        assert!(
            error.contains("/tmp/tmux-1028/default"),
            "the underlying tmux error must stay visible for debugging: {payload}"
        );
        assert!(
            payload.get("agents").is_none() && payload.get("count").is_none(),
            "a failed read must not also ship a fleet listing: {payload}"
        );
    }

    /// True-negative guard for #880: a reachable tmux with no panes is still a
    /// perfectly good 200 with an empty list. The fix must not turn every
    /// empty answer into an error.
    #[tokio::test]
    async fn agents880_reachable_and_genuinely_empty_still_answers_200() {
        let state = ServecoreSharedState::default()
            .servecore_with_agents_node(Some("node-a".to_owned()))
            .servecore_with_agents_snapshot(Vec::new());
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
        assert_eq!(payload["count"], 0);
        assert_eq!(payload["total"], 0);
    }
}
