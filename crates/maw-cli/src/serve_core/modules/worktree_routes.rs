use super::ServecoreModuleRegistration;
use crate::serve_core::ServecoreLifecycleModule;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

#[must_use]
pub fn worktrees_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "worktrees".to_owned(),
        weight: 50,
    }
}

#[must_use]
pub fn worktrees_registration<S>() -> ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    ServecoreModuleRegistration {
        lifecycle: worktrees_lifecycle_module(),
        mount: worktrees_mount,
    }
}

pub fn worktrees_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.merge(worktrees_router().with_state(Arc::new(worktrees_default_state())))
}

fn worktrees_router() -> Router<Arc<WorktreesModuleState>> {
    Router::new()
        .route("/api/worktrees", get(worktrees_get))
        .route("/api/worktrees/cleanup", post(worktrees_cleanup))
}

#[derive(Clone, Debug)]
struct WorktreesModuleState {
    root: PathBuf,
}

fn worktrees_default_state() -> WorktreesModuleState {
    WorktreesModuleState {
        root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

async fn worktrees_get(State(state): State<Arc<WorktreesModuleState>>) -> impl IntoResponse {
    match worktrees_scan_git(&state.root) {
        Ok(worktrees) => Json(worktrees).into_response(),
        Err(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": message})),
        )
            .into_response(),
    }
}

#[derive(Clone, Debug, Deserialize)]
struct WorktreesCleanupRequest {
    path: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct WorktreesCleanupResponse {
    ok: bool,
    path: String,
    log: Vec<String>,
}

async fn worktrees_cleanup(
    State(state): State<Arc<WorktreesModuleState>>,
    Json(body): Json<WorktreesCleanupRequest>,
) -> impl IntoResponse {
    match worktrees_cleanup_live(&state.root, Path::new(&body.path)) {
        Ok(response) => Json(response).into_response(),
        Err(message) => (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response(),
    }
}

fn worktrees_cleanup_live(root: &Path, path: &Path) -> Result<WorktreesCleanupResponse, String> {
    let target = worktrees_validate_cleanup_path(path, root)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("worktree")
        .arg("remove")
        .arg("--")
        .arg(&target)
        .output()
        .map_err(|error| format!("serve-worktrees: git worktree remove: {error}"))?;
    if !output.status.success() {
        let message = worktrees_stderr_message(&output.stderr);
        return Err(format!(
            "serve-worktrees: git worktree remove failed{message}"
        ));
    }
    Ok(WorktreesCleanupResponse {
        ok: true,
        path: target.to_string_lossy().into_owned(),
        log: worktrees_cleanup_log(&output.stdout, &output.stderr),
    })
}

fn worktrees_stderr_message(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr);
    let trimmed = message.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!(": {trimmed}")
    }
}

fn worktrees_cleanup_log(stdout: &[u8], stderr: &[u8]) -> Vec<String> {
    [stdout, stderr]
        .into_iter()
        .map(|bytes| String::from_utf8_lossy(bytes).trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct WorktreesEntry {
    path: String,
    branch: String,
    repo: String,
    #[serde(rename = "mainRepo")]
    main_repo: String,
    name: String,
    status: String,
    #[serde(rename = "tmuxWindow", skip_serializing_if = "Option::is_none")]
    tmux_window: Option<String>,
    #[serde(rename = "fleetFile", skip_serializing_if = "Option::is_none")]
    fleet_file: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WorktreesPorcelainEntry {
    path: PathBuf,
    branch: Option<String>,
    head: Option<String>,
    prunable: bool,
}

fn worktrees_scan_git(root: &Path) -> Result<Vec<WorktreesEntry>, String> {
    worktrees_validate_scan_root(root)?;
    let output = worktrees_git(root, &["worktree", "list", "--porcelain"])?;
    let mut rows = worktrees_parse_porcelain(&output)
        .into_iter()
        .map(|entry| worktrees_entry(root, &entry))
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(rows)
}

fn worktrees_validate_scan_root(root: &Path) -> Result<(), String> {
    let text = root.to_string_lossy();
    if text.is_empty() || text.contains('\0') || text.contains('\n') {
        return Err("serve-worktrees: root path is rejected".to_owned());
    }
    Ok(())
}

fn worktrees_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("serve-worktrees: git: {error}"))?;
    if !output.status.success() {
        return Err("serve-worktrees: git worktree list failed".to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn worktrees_parse_porcelain(text: &str) -> Vec<WorktreesPorcelainEntry> {
    let mut entries = Vec::new();
    let mut current = WorktreesPorcelainEntry::default();
    for line in text.lines() {
        if line.is_empty() {
            worktrees_push_entry(&mut entries, &mut current);
            continue;
        }
        worktrees_parse_porcelain_line(&mut entries, &mut current, line);
    }
    worktrees_push_entry(&mut entries, &mut current);
    entries
}

fn worktrees_parse_porcelain_line(
    entries: &mut Vec<WorktreesPorcelainEntry>,
    current: &mut WorktreesPorcelainEntry,
    line: &str,
) {
    if let Some(path) = line.strip_prefix("worktree ") {
        worktrees_push_entry(entries, current);
        current.path = PathBuf::from(path);
    } else if let Some(head) = line.strip_prefix("HEAD ") {
        current.head = Some(head.to_owned());
    } else if let Some(branch) = line.strip_prefix("branch ") {
        current.branch = Some(branch.to_owned());
    } else if line == "prunable" || line.starts_with("prunable ") {
        current.prunable = true;
    }
}

fn worktrees_push_entry(
    entries: &mut Vec<WorktreesPorcelainEntry>,
    current: &mut WorktreesPorcelainEntry,
) {
    if !current.path.as_os_str().is_empty() {
        entries.push(std::mem::take(current));
    }
}

fn worktrees_entry(root: &Path, entry: &WorktreesPorcelainEntry) -> WorktreesEntry {
    let path = worktrees_canonical_or_original(&entry.path);
    let (repo, main_repo, name) = worktrees_names(root, &path);
    WorktreesEntry {
        path: path.to_string_lossy().into_owned(),
        branch: worktrees_branch(entry.branch.as_deref()),
        repo,
        main_repo,
        name,
        status: if entry.prunable { "orphan" } else { "stale" }.to_owned(),
        tmux_window: None,
        fleet_file: None,
    }
}

fn worktrees_canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn worktrees_branch(branch: Option<&str>) -> String {
    branch
        .and_then(|value| value.strip_prefix("refs/heads/").or(Some(value)))
        .filter(|value| worktrees_valid_text(value))
        .unwrap_or("unknown")
        .to_owned()
}

fn worktrees_names(root: &Path, path: &Path) -> (String, String, String) {
    let repo = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| worktrees_valid_text(value))
        .unwrap_or("worktree")
        .to_owned();
    let main_repo = root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| worktrees_valid_text(value))
        .unwrap_or(repo.as_str())
        .to_owned();
    let name = repo
        .split_once(".wt-")
        .map_or(repo.as_str(), |(_, suffix)| suffix)
        .to_owned();
    (repo, main_repo, name)
}

fn worktrees_valid_text(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value != "--"
        && !value.starts_with('-')
        && !value.chars().any(char::is_control)
}

fn worktrees_validate_cleanup_path(path: &Path, root: &Path) -> Result<PathBuf, String> {
    let raw = path.to_string_lossy();
    if raw.is_empty() || raw.contains('\0') || raw.contains('\n') || raw.contains('\r') {
        return Err("serve-worktrees: cleanup path is rejected".to_owned());
    }
    if raw
        .split('/')
        .any(|segment| segment == ".." || segment == "--" || segment.starts_with('-'))
    {
        return Err("serve-worktrees: cleanup path segment is rejected".to_owned());
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("serve-worktrees: cleanup path: {error}"))?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("serve-worktrees: cleanup root: {error}"))?;
    let allowed_root = canonical_root.parent().unwrap_or(&canonical_root);
    if !canonical.starts_with(allowed_root)
        || canonical == canonical_root
        || !canonical.join(".git").exists()
        || !worktrees_registered_target(root, &canonical)?
    {
        return Err("serve-worktrees: cleanup target must be a git worktree near root".to_owned());
    }
    Ok(canonical)
}

fn worktrees_registered_target(root: &Path, target: &Path) -> Result<bool, String> {
    let output = worktrees_git(root, &["worktree", "list", "--porcelain"])?;
    Ok(worktrees_parse_porcelain(&output)
        .into_iter()
        .map(|entry| worktrees_canonical_or_original(&entry.path))
        .any(|path| path == target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{
        modules::servecore_mount_modules, servecore_apply_pipeline, servecore_mount_core_routes,
        servecore_with_shared_state, ServecoreSharedState,
    };
    use axum::http::StatusCode;
    use std::{
        net::Ipv4Addr,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use tokio::sync::oneshot;

    async fn worktrees_spawn_module(root: &Path) -> std::net::SocketAddr {
        let router: Router<()> = worktrees_router().with_state(Arc::new(WorktreesModuleState {
            root: root.to_path_buf(),
        }));
        worktrees_spawn_router(router).await
    }

    async fn worktrees_spawn_aggregator() -> std::net::SocketAddr {
        let router = servecore_mount_core_routes(servecore_mount_modules(
            Router::<()>::new(),
            &["worktrees".to_owned()],
        ));
        worktrees_spawn_router(router).await
    }

    async fn worktrees_spawn_router(router: Router<()>) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
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

    fn worktrees_temp(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("maw-rs-worktrees-{name}-{nonce}"));
        std::fs::create_dir_all(&path).expect("temp");
        path
    }

    fn worktrees_git_for_tests() -> PathBuf {
        option_env!("PATH")
            .and_then(|path| {
                std::env::split_paths(path)
                    .map(|dir| dir.join("git"))
                    .find(|candidate| candidate.is_file())
            })
            .unwrap_or_else(|| PathBuf::from("git"))
    }

    fn worktrees_run(root: &Path, args: &[&str]) {
        let output = Command::new(worktrees_git_for_tests())
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn worktrees_seed_repo() -> (PathBuf, PathBuf) {
        let root = worktrees_temp("repo").join("main");
        std::fs::create_dir_all(&root).expect("main");
        worktrees_run(&root, &["init"]);
        worktrees_run(&root, &["config", "user.email", "agent@example.invalid"]);
        worktrees_run(&root, &["config", "user.name", "Agent"]);
        std::fs::write(root.join("README.md"), "seed\n").expect("readme");
        worktrees_run(&root, &["add", "README.md"]);
        worktrees_run(&root, &["commit", "-m", "seed"]);
        let wt = root.with_file_name("main.wt-feature");
        worktrees_run(
            &root,
            &["worktree", "add", wt.to_str().expect("wt"), "-b", "feature"],
        );
        (root, wt)
    }

    #[test]
    fn worktrees_parse_porcelain_extracts_branch_and_prunable() {
        let rows = worktrees_parse_porcelain(
            "worktree /tmp/main\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/main.wt-feature\nHEAD def\nbranch refs/heads/feature\nprunable gitdir file points to non-existent location\n",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(rows[1].prunable);
    }

    #[test]
    fn worktrees_cleanup_path_validates_before_remove() {
        let (root, wt) = worktrees_seed_repo();
        let valid = worktrees_validate_cleanup_path(&wt, &root).expect("valid");
        assert!(valid.ends_with("main.wt-feature"));
        let rejected = worktrees_validate_cleanup_path(Path::new("--bad"), &root).expect_err("bad");
        assert!(rejected.contains("rejected"));
        let traversal = root.join("../main.wt-feature");
        let traversal_error =
            worktrees_validate_cleanup_path(&traversal, &root).expect_err("traversal");
        assert!(traversal_error.contains("segment"));
        let main_error = worktrees_validate_cleanup_path(&root, &root).expect_err("main");
        assert!(main_error.contains("git worktree near root"));
        let outside = worktrees_temp("outside");
        assert!(worktrees_validate_cleanup_path(&outside, &root).is_err());
        let _ = std::fs::remove_dir_all(root.parent().expect("parent"));
        let _ = std::fs::remove_dir_all(outside);
    }

    fn worktrees_signed_post(
        client: &reqwest::Client,
        addr: std::net::SocketAddr,
        body: String,
    ) -> reqwest::RequestBuilder {
        const KEY: &str = "worktrees-test-secret";
        const NOW: i64 = 1_700_000_000;
        let headers = maw_auth::sign_headers_v3_at(
            KEY,
            "gm-bo:test",
            "POST",
            "/worktrees/cleanup",
            Some(body.as_bytes()),
            NOW,
        )
        .expect("headers");
        let fleet_signature =
            maw_auth::sign_hmac_sig(KEY, &format!("POST:/api/worktrees/cleanup:{NOW}"));
        let mut request = client
            .post(format!("http://{addr}/api/worktrees/cleanup"))
            .header("content-type", "application/json")
            .header("x-maw-signature", fleet_signature)
            .body(body);
        for (name, value) in headers.to_btree_map() {
            request = request.header(name.as_str(), value);
        }
        request
    }

    async fn worktrees_spawn_authed_module(root: &Path) -> std::net::SocketAddr {
        const KEY: &str = "worktrees-test-secret";
        const NOW: i64 = 1_700_000_000;
        let router: Router<()> = worktrees_router().with_state(Arc::new(WorktreesModuleState {
            root: root.to_path_buf(),
        }));
        let state = ServecoreSharedState::default()
            .servecore_with_auth(Some(KEY.to_owned()), None)
            .servecore_with_auth_now(NOW);
        worktrees_spawn_router_with_shared_state(router, state).await
    }

    async fn worktrees_spawn_router_with_shared_state(
        router: Router<()>,
        state: ServecoreSharedState,
    ) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = servecore_with_shared_state(servecore_apply_pipeline(router), state);
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

    #[tokio::test]
    async fn worktrees_cleanup_live_removes_only_signed_valid_worktree() {
        let (root, wt) = worktrees_seed_repo();
        let addr = worktrees_spawn_authed_module(&root).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let bad_body =
            json!({"path": root.join("../main.wt-feature").to_string_lossy()}).to_string();
        let bad = worktrees_signed_post(&client, addr, bad_body)
            .send()
            .await
            .expect("bad cleanup");
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        assert!(wt.exists());
        let body = json!({"path": wt.to_string_lossy()}).to_string();
        let cleanup = worktrees_signed_post(&client, addr, body)
            .send()
            .await
            .expect("cleanup");
        assert_eq!(cleanup.status(), StatusCode::OK);
        let payload = cleanup.json::<serde_json::Value>().await.expect("json");
        assert_eq!(payload["ok"], true);
        assert!(!wt.exists(), "validated worktree should be removed");
        let list = worktrees_git(&root, &["worktree", "list", "--porcelain"]).expect("list");
        assert!(!list.contains("main.wt-feature"), "{list}");
        let _ = std::fs::remove_dir_all(root.parent().expect("parent"));
    }

    #[tokio::test]
    async fn worktrees_get_public_and_cleanup_default_denied() {
        let (root, _wt) = worktrees_seed_repo();
        let addr = worktrees_spawn_module(&root).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{addr}/api/worktrees"))
            .send()
            .await
            .expect("worktrees");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response.json::<serde_json::Value>().await.expect("json");
        let rows = payload.as_array().expect("array");
        assert!(
            rows.iter().any(|row| row["branch"] == "feature"),
            "{rows:?}"
        );
        assert!(maw_auth::is_protected("/api/worktrees/cleanup", "POST"));
        let cleanup_addr = worktrees_spawn_aggregator().await;
        let cleanup = client
            .post(format!("http://{cleanup_addr}/api/worktrees/cleanup"))
            .json(&json!({"path":"/tmp/nope"}))
            .send()
            .await
            .expect("cleanup");
        assert_eq!(cleanup.status(), StatusCode::FORBIDDEN);
        let _ = std::fs::remove_dir_all(root.parent().expect("parent"));
    }
}
