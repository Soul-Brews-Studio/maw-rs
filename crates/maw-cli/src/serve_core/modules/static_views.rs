use super::ServecoreLifecycleModule;
use axum::{
    body::Body,
    extract::Request,
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Router,
};
use serde_json::json;
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tower::ServiceExt;
use tower_http::services::ServeDir;

const VIEWS_INLINE_DOOR_HTML: &str = r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><title>maw</title></head><body style="font-family:monospace;background:#0d0d0d;color:#ccc;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0"><div style="text-align:center"><h1 style="color:#fff">maw</h1><p>maw-ui not installed. Run <code style="color:#7dd3fc">maw ui build</code> or install maw-ui.</p></div></body></html>"#;

#[derive(Clone, Debug)]
pub struct ViewsConfig {
    ui_dist_dir: PathBuf,
    door_html_path: PathBuf,
    topology_html_path: PathBuf,
}

impl ViewsConfig {
    #[must_use]
    pub fn views_from_process_env() -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        let vars = [
            "MAW_HOME",
            "MAW_DATA_DIR",
            "MAW_XDG",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
        let env = maw_xdg::MawXdgEnv::with_vars(home, vars);
        let ui_dist_dir = std::env::var_os("MAW_UI_DIR").map_or_else(
            || maw_xdg::maw_data_path(&env, &["ui", "dist"]),
            PathBuf::from,
        );
        Self {
            ui_dist_dir,
            door_html_path: PathBuf::from("core/static/door.html"),
            topology_html_path: std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("ψ")
                .join("outbox")
                .join("fleet-topology.html"),
        }
    }

    #[must_use]
    pub fn views_with_paths(
        ui_dist_dir: impl Into<PathBuf>,
        door_html_path: impl Into<PathBuf>,
        topology_html_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ui_dist_dir: ui_dist_dir.into(),
            door_html_path: door_html_path.into(),
            topology_html_path: topology_html_path.into(),
        }
    }
}

#[must_use]
pub fn views_lifecycle_module() -> ServecoreLifecycleModule {
    ServecoreLifecycleModule {
        name: "views".to_owned(),
        weight: 10,
    }
}

pub fn views_mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
}

#[must_use]
pub fn views_registration<S>() -> super::ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    super::ServecoreModuleRegistration {
        lifecycle: views_lifecycle_module(),
        mount: views_mount,
    }
}

pub fn views_apply_fallback<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    views_apply_fallback_with_config(router, ViewsConfig::views_from_process_env())
}

pub fn views_apply_fallback_with_config<S>(router: Router<S>, config: ViewsConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/topology", get(views_topology))
        .route("/", get(views_door))
        .fallback(views_static_fallback)
        .layer(Extension(Arc::new(config)))
}

pub async fn views_topology(Extension(config): Extension<Arc<ViewsConfig>>) -> Response {
    views_read_html_or_text(&config.topology_html_path, "fleet-topology.html not found").await
}

pub async fn views_door(Extension(config): Extension<Arc<ViewsConfig>>) -> Response {
    views_door_response(&config.door_html_path).await
}

pub async fn views_static_fallback(
    Extension(config): Extension<Arc<ViewsConfig>>,
    req: Request,
) -> Response {
    let path = req.uri().path().to_owned();
    if path.starts_with("/api/") || path == "/api" {
        return views_not_found_json();
    }
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if !views_static_path_is_bounded(&config.ui_dist_dir, &path) {
        return views_not_found_json();
    }
    views_serve_static(config.ui_dist_dir.clone(), req).await
}

async fn views_read_html_or_text(path: &Path, missing: &'static str) -> Response {
    match tokio::fs::read_to_string(path).await {
        Ok(html) => views_html_response(html),
        Err(_) => (StatusCode::NOT_FOUND, missing).into_response(),
    }
}

async fn views_door_response(path: &Path) -> Response {
    match tokio::fs::read_to_string(path).await {
        Ok(html) => views_html_response(html),
        Err(_) => views_html_response(VIEWS_INLINE_DOOR_HTML),
    }
}

fn views_html_response(html: impl Into<String>) -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html.into(),
    )
        .into_response()
}

fn views_not_found_json() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({"error":"not_found","fallback":"views"})),
    )
        .into_response()
}

async fn views_serve_static(dist: PathBuf, req: Request) -> Response {
    match ServeDir::new(dist).oneshot(req).await {
        Ok(response) => response.map(Body::new),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn views_static_path_is_bounded(dist: &Path, request_path: &str) -> bool {
    if views_path_has_encoded_parent_or_backslash(request_path) {
        return false;
    }
    let Ok(dist_root) = dist.canonicalize() else {
        return true;
    };
    let mut candidate = dist.to_path_buf();
    for component in Path::new(request_path.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => candidate.push(part),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return false,
        }
    }
    if candidate.exists() {
        let Ok(real) = candidate.canonicalize() else {
            return false;
        };
        real.starts_with(dist_root)
    } else {
        true
    }
}

fn views_path_has_encoded_parent_or_backslash(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("..") || lower.contains("%2e") || lower.contains('\\') || lower.contains("%5c")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve_core::{
        servecore_apply_pipeline_with_views_config, servecore_mount_core_routes,
    };
    use std::{
        fs,
        net::Ipv4Addr,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use tokio::sync::oneshot;

    async fn views_spawn(config: ViewsConfig, router: Router) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = servecore_apply_pipeline_with_views_config(router, config);
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

    fn views_temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("maw-views-{name}-{nanos}"));
        fs::create_dir_all(&root).expect("root");
        root
    }

    fn views_test_config(root: &Path) -> ViewsConfig {
        let dist = root.join("dist");
        fs::create_dir_all(&dist).expect("dist");
        fs::write(dist.join("app.js"), "console.log('maw');").expect("app");
        fs::write(root.join("door.html"), "<h1>door</h1>").expect("door");
        let topology = root.join("ψ").join("outbox").join("fleet-topology.html");
        fs::create_dir_all(topology.parent().expect("topology parent")).expect("topology parent");
        fs::write(&topology, "<h1>topology</h1>").expect("topology");
        ViewsConfig::views_with_paths(dist, root.join("door.html"), topology)
    }

    #[tokio::test]
    async fn views_serves_topology_door_and_static_dist() {
        let root = views_temp_root("happy");
        let addr = views_spawn(views_test_config(&root), Router::new()).await;
        let client = reqwest::Client::new();

        let topology = client
            .get(format!("http://{addr}/topology"))
            .send()
            .await
            .expect("topology");
        assert_eq!(topology.status(), StatusCode::OK);
        assert!(topology.text().await.expect("body").contains("topology"));

        let door = client
            .get(format!("http://{addr}/"))
            .send()
            .await
            .expect("door");
        assert_eq!(door.status(), StatusCode::OK);
        assert!(door.text().await.expect("body").contains("door"));

        let static_file = client
            .get(format!("http://{addr}/app.js"))
            .send()
            .await
            .expect("static");
        assert_eq!(static_file.status(), StatusCode::OK);
        assert!(static_file.text().await.expect("body").contains("maw"));

        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn views_rejects_traversal_and_out_of_dist_symlink() {
        let root = views_temp_root("safe");
        let config = views_test_config(&root);
        fs::write(root.join("secret.txt"), "secret").expect("secret");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("secret.txt"), root.join("dist").join("leak.txt"))
            .expect("symlink");
        let addr = views_spawn(config, Router::new()).await;
        let client = reqwest::Client::new();

        let traversal = client
            .get(format!("http://{addr}/../../etc/passwd"))
            .send()
            .await
            .expect("traversal");
        assert!(matches!(
            traversal.status(),
            StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST
        ));

        #[cfg(unix)]
        {
            let symlink = client
                .get(format!("http://{addr}/leak.txt"))
                .send()
                .await
                .expect("symlink");
            assert_eq!(symlink.status(), StatusCode::NOT_FOUND);
            assert!(!symlink.text().await.expect("body").contains("secret"));
        }

        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn views_fallback_does_not_bypass_api_default_deny_or_api_routes() {
        let root = views_temp_root("api");
        let addr = views_spawn(
            views_test_config(&root),
            servecore_mount_core_routes(Router::new()),
        )
        .await;
        let client = reqwest::Client::new();

        let protected = client
            .post(format!("http://{addr}/api/triggers/fire"))
            .send()
            .await
            .expect("protected");
        assert_eq!(protected.status(), StatusCode::FORBIDDEN);
        assert!(protected
            .text()
            .await
            .expect("protected body")
            .contains("missing-credentials"));

        let pipeline = client
            .get(format!("http://{addr}/api/serve-core/pipeline"))
            .send()
            .await
            .expect("pipeline");
        assert_eq!(pipeline.status(), StatusCode::OK);
        assert!(pipeline
            .text()
            .await
            .expect("body")
            .contains("fallback-views"));

        let unmatched_api = client
            .get(format!("http://{addr}/api/nope"))
            .send()
            .await
            .expect("api 404");
        assert_eq!(unmatched_api.status(), StatusCode::NOT_FOUND);
        assert!(!unmatched_api.text().await.expect("body").contains("door"));

        tokio::time::sleep(Duration::from_millis(1)).await;
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn views_lifecycle_is_last_weight_and_noop_mount() {
        let module = views_lifecycle_module();
        assert_eq!(module.name, "views");
        assert_eq!(module.weight, 10);
        let router: Router = views_mount(Router::new());
        let _ = router;
    }
}
