const DISPATCH_290: &[DispatcherEntry] = &[DispatcherEntry { command: "reindex-gpu", handler: Handler::Async(reindex_async_native) }];

const REINDEX_USAGE: &str = "usage: maw reindex-gpu [root] [--json] [--timeout-ms <ms>]";
const REINDEX_MAX_BYTES_PER_FILE: u64 = 256 * 1024;
const REINDEX_ALLOWED_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "json", "toml", "yaml", "yml"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReindexArgs290 {
    root: std::path::PathBuf,
    endpoint: String,
    alt_endpoint: Option<String>,
    index_path: std::path::PathBuf,
    json: bool,
    timeout_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReindexDocument290 {
    path: String,
    bytes: u64,
    text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReindexPayload290 {
    client: &'static str,
    mode: &'static str,
    root: String,
    documents: Vec<ReindexDocument290>,
}

#[derive(Debug, Clone)]
struct ReindexPostOutcome290 {
    endpoint: String,
    status: u16,
    body: String,
    used_alt: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReindexIndex290 {
    command: &'static str,
    endpoint: String,
    used_alt_endpoint: bool,
    status: u16,
    root: String,
    documents: Vec<ReindexIndexDocument290>,
    gateway_body: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReindexIndexDocument290 {
    path: String,
    bytes: u64,
}

trait ReindexGateway290 {
    fn post<'a>(
        &'a mut self,
        endpoint: &'a str,
        payload: &'a ReindexPayload290,
        timeout_ms: u64,
        used_alt: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ReindexPostOutcome290, String>> + Send + 'a>>;
}

struct ReindexReqwestGateway290;

impl ReindexGateway290 for ReindexReqwestGateway290 {
    fn post<'a>(
        &'a mut self,
        endpoint: &'a str,
        payload: &'a ReindexPayload290,
        timeout_ms: u64,
        used_alt: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ReindexPostOutcome290, String>> + Send + 'a>> {
        Box::pin(async move { reindex_reqwest_post_once(endpoint, payload, timeout_ms, used_alt).await })
    }
}

fn reindex_async_native(args: Vec<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = CliOutput> + Send>> {
    Box::pin(async move {
        let mut gateway = ReindexReqwestGateway290;
        reindex_run_async_with(&args, &mut gateway).await
    })
}

async fn reindex_run_async_with<G: ReindexGateway290 + Send>(argv: &[String], gateway: &mut G) -> CliOutput {
    match reindex_execute_with(argv, gateway).await {
        Ok(output) => output,
        Err(message) if message == REINDEX_USAGE => CliOutput { code: 0, stdout: format!("{REINDEX_USAGE}\n"), stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

async fn reindex_execute_with<G: ReindexGateway290 + Send>(argv: &[String], gateway: &mut G) -> Result<CliOutput, String> {
    let options = reindex_parse_args(argv)?;
    let payload = reindex_build_payload(&options)?;
    let count = payload.documents.len();
    let outcome = reindex_post_with_fallback(&options, &payload, gateway).await?;
    let index = reindex_build_index(&payload, &outcome);
    reindex_write_index_atomic(&options.index_path, &index)?;
    if options.json {
        let stdout = serde_json::to_string_pretty(&serde_json::json!({
            "ok": true,
            "command": "reindex-gpu",
            "endpoint": outcome.endpoint,
            "usedAltEndpoint": outcome.used_alt,
            "status": outcome.status,
            "documents": count,
            "indexPath": options.index_path,
        }))
        .map_err(|error| format!("reindex-gpu: encode json failed: {error}"))?;
        return Ok(CliOutput { code: 0, stdout: format!("{stdout}\n"), stderr: String::new() });
    }
    let alt = if outcome.used_alt { " (alternate endpoint)" } else { "" };
    Ok(CliOutput {
        code: 0,
        stdout: format!(
            "\x1b[32m✓\x1b[0m reindex-gpu posted {count} document(s) to {}{alt}\n  status: {}\n  index: {}\n",
            outcome.endpoint,
            outcome.status,
            options.index_path.display()
        ),
        stderr: String::new(),
    })
}

fn reindex_parse_args(argv: &[String]) -> Result<ReindexArgs290, String> {
    let mut root: Option<std::path::PathBuf> = None;
    let mut json = false;
    let mut timeout_ms = std::env::var("MAW_REINDEX_GPU_TIMEOUT_MS").ok().and_then(|value| value.parse().ok()).unwrap_or(10_000);
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "help" | "--help" | "-h" => return Err(REINDEX_USAGE.to_owned()),
            "--json" => json = true,
            "--timeout-ms" => {
                index += 1;
                let raw = argv.get(index).ok_or_else(|| "reindex-gpu: --timeout-ms requires a value".to_owned())?;
                reindex_validate_token(raw, "timeout")?;
                timeout_ms = raw.parse::<u64>().map_err(|_| "reindex-gpu: --timeout-ms must be numeric".to_owned())?;
                if !(100..=120_000).contains(&timeout_ms) { return Err("reindex-gpu: --timeout-ms must be between 100 and 120000".to_owned()); }
            }
            "--endpoint" | "--alt-endpoint" => return Err("reindex-gpu: gateway endpoint must come from config/env, not argv".to_owned()),
            _ if arg.starts_with('-') => return Err(format!("reindex-gpu: leading dash rejected: {arg}")),
            _ => {
                if root.is_some() { return Err(REINDEX_USAGE.to_owned()); }
                reindex_validate_path_arg(arg)?;
                root = Some(std::path::PathBuf::from(arg));
            }
        }
        index += 1;
    }
    let endpoint = reindex_endpoint_from_env_or_config()?;
    let alt_endpoint = reindex_alt_endpoint_from_env_or_config();
    reindex_validate_endpoint(&endpoint, "endpoint")?;
    if let Some(alt) = &alt_endpoint { reindex_validate_endpoint(alt, "alt endpoint")?; }
    let index_path = reindex_index_path_from_env_or_config()?;
    Ok(ReindexArgs290 { root: root.unwrap_or_else(|| std::path::PathBuf::from(".")), endpoint, alt_endpoint, index_path, json, timeout_ms })
}

fn reindex_endpoint_from_env_or_config() -> Result<String, String> {
    if let Ok(value) = std::env::var("MAW_REINDEX_GPU_ENDPOINT") { if !value.trim().is_empty() { return Ok(value); } }
    if let Some(value) = reindex_config_string("reindexGpuEndpoint").or_else(|| reindex_config_nested_string("reindexGpu", "endpoint")) { return Ok(value); }
    Err("reindex-gpu: gateway endpoint required via MAW_REINDEX_GPU_ENDPOINT or maw.config.json reindexGpu.endpoint".to_owned())
}

fn reindex_alt_endpoint_from_env_or_config() -> Option<String> {
    if let Ok(value) = std::env::var("MAW_REINDEX_GPU_ALT_ENDPOINT") { return (!value.trim().is_empty()).then_some(value); }
    reindex_config_string("reindexGpuAltEndpoint").or_else(|| reindex_config_nested_string("reindexGpu", "altEndpoint"))
}

fn reindex_index_path_from_env_or_config() -> Result<std::path::PathBuf, String> {
    let path = std::env::var("MAW_REINDEX_GPU_INDEX_PATH").ok().filter(|value| !value.trim().is_empty()).or_else(|| reindex_config_string("reindexGpuIndexPath").or_else(|| reindex_config_nested_string("reindexGpu", "indexPath")));
    if let Some(path) = path {
        reindex_validate_path_arg(&path)?;
        return Ok(std::path::PathBuf::from(path));
    }
    Ok(maw_state_path(&current_xdg_env(), &["reindex-gpu", "index.json"]))
}

fn reindex_config_string(key: &str) -> Option<String> {
    let path = maw_config_path(&current_xdg_env(), &["maw.config.json"]);
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get(key).and_then(serde_json::Value::as_str).map(str::to_owned)
}

fn reindex_config_nested_string(parent: &str, key: &str) -> Option<String> {
    let path = maw_config_path(&current_xdg_env(), &["maw.config.json"]);
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get(parent)?.get(key).and_then(serde_json::Value::as_str).map(str::to_owned)
}

fn reindex_build_payload(args: &ReindexArgs290) -> Result<ReindexPayload290, String> {
    let root = std::fs::canonicalize(&args.root).map_err(|error| format!("reindex-gpu: root not readable: {error}"))?;
    if !root.is_dir() { return Err("reindex-gpu: root must be a directory".to_owned()); }
    let mut documents = Vec::new();
    reindex_collect_documents(&root, &root, &mut documents)?;
    documents.sort_by(|a, b| a.path.cmp(&b.path));
    if documents.is_empty() { return Err("reindex-gpu: no indexable documents found".to_owned()); }
    Ok(ReindexPayload290 { client: "maw-rs", mode: "remote-gpu-embed", root: root.to_string_lossy().into_owned(), documents })
}

fn reindex_collect_documents(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<ReindexDocument290>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|error| format!("reindex-gpu: read {} failed: {error}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "dist") { continue; }
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_dir() {
            reindex_collect_documents(root, &path, out)?;
            continue;
        }
        if !file_type.is_file() || !reindex_is_indexable(&path) { continue; }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() > REINDEX_MAX_BYTES_PER_FILE { continue; }
        let Ok(text) = std::fs::read_to_string(&path) else { continue; };
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        out.push(ReindexDocument290 { path: rel, bytes: meta.len(), text });
    }
    Ok(())
}

fn reindex_is_indexable(path: &std::path::Path) -> bool {
    path.extension().and_then(std::ffi::OsStr::to_str).is_some_and(|ext| REINDEX_ALLOWED_EXTENSIONS.iter().any(|allowed| ext.eq_ignore_ascii_case(allowed)))
}

async fn reindex_post_with_fallback<G: ReindexGateway290 + Send>(args: &ReindexArgs290, payload: &ReindexPayload290, gateway: &mut G) -> Result<ReindexPostOutcome290, String> {
    let primary = gateway.post(&args.endpoint, payload, args.timeout_ms, false).await;
    match primary {
        Ok(outcome) => Ok(outcome),
        Err(primary_error) => {
            let Some(alt) = &args.alt_endpoint else { return Err(primary_error); };
            gateway.post(alt, payload, args.timeout_ms, true).await.map_err(|alt_error| format!("{primary_error}; alternate endpoint failed: {alt_error}"))
        }
    }
}

async fn reindex_reqwest_post_once(endpoint: &str, payload: &ReindexPayload290, timeout_ms: u64, used_alt: bool) -> Result<ReindexPostOutcome290, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|error| format!("reindex-gpu: HTTP client build failed: {error}"))?;
    let response = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(payload)
        .send()
        .await
        .map_err(|error| format!("reindex-gpu: remote gpu-embed gateway unreachable: {error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() { return Err(format!("reindex-gpu: remote gpu-embed gateway returned HTTP {}", status.as_u16())); }
    Ok(ReindexPostOutcome290 { endpoint: endpoint.to_owned(), status: status.as_u16(), body: reindex_redact_body(&body), used_alt })
}

fn reindex_build_index(payload: &ReindexPayload290, outcome: &ReindexPostOutcome290) -> ReindexIndex290 {
    ReindexIndex290 {
        command: "reindex-gpu",
        endpoint: outcome.endpoint.clone(),
        used_alt_endpoint: outcome.used_alt,
        status: outcome.status,
        root: payload.root.clone(),
        documents: payload.documents.iter().map(|doc| ReindexIndexDocument290 { path: doc.path.clone(), bytes: doc.bytes }).collect(),
        gateway_body: outcome.body.clone(),
    }
}

fn reindex_write_index_atomic(path: &std::path::Path, index: &ReindexIndex290) -> Result<(), String> {
    use std::io::Write as _;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt as _;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| format!("reindex-gpu: create index dir {} failed: {error}", parent.display()))?;
    let body = serde_json::to_string_pretty(index).map_err(|error| format!("reindex-gpu: encode index failed: {error}"))? + "\n";
    let tmp = path.with_extension("json.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).truncate(true).write(true);
    #[cfg(unix)]
    opts.mode(0o600);
    let mut file = opts.open(&tmp).map_err(|error| format!("reindex-gpu: open temp index {} failed: {error}", tmp.display()))?;
    file.write_all(body.as_bytes()).map_err(|error| format!("reindex-gpu: write temp index {} failed: {error}", tmp.display()))?;
    file.sync_all().map_err(|error| format!("reindex-gpu: sync temp index {} failed: {error}", tmp.display()))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|error| format!("reindex-gpu: rename index {} -> {} failed: {error}", tmp.display(), path.display()))
}

fn reindex_validate_endpoint(value: &str, label: &str) -> Result<(), String> {
    reindex_validate_token(value, label)?;
    let parsed = reqwest::Url::parse(value).map_err(|error| format!("reindex-gpu: invalid {label}: {error}"))?;
    if parsed.username() != "" || parsed.password().is_some() { return Err(format!("reindex-gpu: invalid {label}: credentials in URL rejected")); }
    if parsed.path() != "/api/embed" { return Err(format!("reindex-gpu: invalid {label}: path must be /api/embed")); }
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if reindex_url_host_is_loopback(&parsed) => Ok(()),
        "http" => Err(format!("reindex-gpu: invalid {label}: http endpoint must be loopback; use https for remote gateway")),
        _ => Err(format!("reindex-gpu: invalid {label}: only https or http-loopback allowed")),
    }
}

fn reindex_url_host_is_loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else { return false; };
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn reindex_validate_path_arg(value: &str) -> Result<(), String> {
    reindex_validate_token(value, "path")?;
    let path = std::path::Path::new(value);
    if path.components().any(|component| matches!(component, std::path::Component::ParentDir)) { return Err("reindex-gpu: path traversal rejected".to_owned()); }
    Ok(())
}

fn reindex_validate_token(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() { return Err(format!("reindex-gpu: {label} is empty")); }
    if value.starts_with('-') { return Err(format!("reindex-gpu: {label} leading dash rejected")); }
    if value.chars().any(|ch| ch == '\0' || ch.is_control()) { return Err(format!("reindex-gpu: {label} control character rejected")); }
    Ok(())
}

fn reindex_redact_body(body: &str) -> String {
    body.split_whitespace()
        .map(|token| if token.starts_with("ghp_") || token.starts_with("github_pat_") || token.starts_with("sk-") || token.len() > 80 { "[redacted]" } else { token })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod reindex_tests290 {
    use super::*;

    #[derive(Default)]
    struct MockGateway290 {
        calls: Vec<(String, bool, usize)>,
        fail_first: bool,
    }

    impl ReindexGateway290 for MockGateway290 {
        fn post<'a>(
            &'a mut self,
            endpoint: &'a str,
            payload: &'a ReindexPayload290,
            _timeout_ms: u64,
            used_alt: bool,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ReindexPostOutcome290, String>> + Send + 'a>> {
            Box::pin(async move {
                self.calls.push((endpoint.to_owned(), used_alt, payload.documents.len()));
                if self.fail_first && !used_alt { return Err("reindex-gpu: remote gpu-embed gateway unreachable: mock".to_owned()); }
                Ok(ReindexPostOutcome290 { endpoint: endpoint.to_owned(), status: 200, body: "{\"ok\":true}".to_owned(), used_alt })
            })
        }
    }

    fn reindex_test_block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(future)
    }

    #[test]
    fn reindex_dispatch_declares_part290() {
        assert_eq!(DISPATCH_290.len(), 1);
        assert_eq!(DISPATCH_290[0].command, "reindex-gpu");
    }

    #[test]
    fn reindex_parse_rejects_endpoint_and_path_injection() {
        let _guard = env_test_lock().lock().expect("env lock");
        let _endpoint = EnvVarRestore::capture("MAW_REINDEX_GPU_ENDPOINT");
        std::env::set_var("MAW_REINDEX_GPU_ENDPOINT", "http://user:pw@example.test/api/embed");
        assert!(reindex_parse_args(&[]).expect_err("creds").contains("credentials"));
        std::env::set_var("MAW_REINDEX_GPU_ENDPOINT", "http://example.test/api/embed");
        assert!(reindex_parse_args(&[]).expect_err("ssrf").contains("http endpoint must be loopback"));
        std::env::set_var("MAW_REINDEX_GPU_ENDPOINT", "https://example.test/not-embed");
        assert!(reindex_parse_args(&[]).expect_err("path").contains("/api/embed"));
        assert!(reindex_parse_args(&["../brain".to_owned()]).expect_err("traversal").contains("traversal"));
        assert!(reindex_parse_args(&["-bad".to_owned()]).expect_err("dash").contains("leading dash"));
    }

    #[test]
    fn reindex_mock_gateway_writes_atomic_index_and_matches_golden() {
        let _guard = env_test_lock().lock().expect("env lock");
        let restores = [
            EnvVarRestore::capture("MAW_REINDEX_GPU_ENDPOINT"),
            EnvVarRestore::capture("MAW_REINDEX_GPU_ALT_ENDPOINT"),
            EnvVarRestore::capture("MAW_REINDEX_GPU_INDEX_PATH"),
        ];
        let root = reindex_temp_dir("mock");
        let docs = root.join("brain");
        std::fs::create_dir_all(&docs).expect("docs");
        std::fs::write(docs.join("README.md"), "# Alpha\nremote embed me\n").expect("doc1");
        std::fs::write(docs.join("note.txt"), "Beta note\n").expect("doc2");
        let index_path = root.join("index.json");
        std::env::set_var("MAW_REINDEX_GPU_ENDPOINT", "https://gpu.example.test/api/embed");
        std::env::set_var("MAW_REINDEX_GPU_INDEX_PATH", &index_path);
        let mut gateway = MockGateway290::default();
        let output = reindex_test_block_on(reindex_run_async_with(&[docs.to_string_lossy().into_owned()], &mut gateway));
        assert_eq!(output.code, 0, "{}", output.stderr);
        let normalized = output.stdout.replace(&root.display().to_string(), "<ROOT>");
        assert_eq!(normalized, include_str!("../../tests/fixtures/native-reindex-gpu/reindex-mock.stdout"));
        assert_eq!(gateway.calls, vec![("https://gpu.example.test/api/embed".to_owned(), false, 2)]);
        let index = std::fs::read_to_string(&index_path).expect("index");
        assert!(index.contains("remote-gpu-embed") || index.contains("reindex-gpu"));
        assert!(index.contains("README.md"));
        assert!(!root.join("index.json.tmp").exists());
        drop(restores);
    }

    #[test]
    fn reindex_unreachable_does_not_write_partial_index_and_alt_is_remote_only() {
        let _guard = env_test_lock().lock().expect("env lock");
        let restores = [
            EnvVarRestore::capture("MAW_REINDEX_GPU_ENDPOINT"),
            EnvVarRestore::capture("MAW_REINDEX_GPU_ALT_ENDPOINT"),
            EnvVarRestore::capture("MAW_REINDEX_GPU_INDEX_PATH"),
        ];
        let root = reindex_temp_dir("alt");
        let docs = root.join("brain");
        std::fs::create_dir_all(&docs).expect("docs");
        std::fs::write(docs.join("README.md"), "# Alpha\n").expect("doc");
        let index_path = root.join("index.json");
        std::env::set_var("MAW_REINDEX_GPU_ENDPOINT", "https://primary.example.test/api/embed");
        std::env::set_var("MAW_REINDEX_GPU_ALT_ENDPOINT", "https://alt.example.test/api/embed");
        std::env::set_var("MAW_REINDEX_GPU_INDEX_PATH", &index_path);
        let mut gateway = MockGateway290 { fail_first: true, ..MockGateway290::default() };
        let output = reindex_test_block_on(reindex_run_async_with(&[docs.to_string_lossy().into_owned()], &mut gateway));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(gateway.calls, vec![
            ("https://primary.example.test/api/embed".to_owned(), false, 1),
            ("https://alt.example.test/api/embed".to_owned(), true, 1),
        ]);
        assert!(std::fs::read_to_string(&index_path).expect("index").contains("alt.example.test"));

        std::fs::remove_file(&index_path).expect("reset index");
        std::env::remove_var("MAW_REINDEX_GPU_ALT_ENDPOINT");
        let mut failing = MockGateway290 { fail_first: true, ..MockGateway290::default() };
        let failed = reindex_test_block_on(reindex_run_async_with(&[docs.to_string_lossy().into_owned()], &mut failing));
        assert_ne!(failed.code, 0);
        assert!(failed.stderr.contains("unreachable"));
        assert!(!index_path.exists(), "unreachable gateway must not write partial index");
        drop(restores);
    }

    #[test]
    fn reindex_redacts_secret_shaped_gateway_body() {
        assert_eq!(reindex_redact_body("ok ghp_secret github_pat_secret sk-secret short"), "ok [redacted] [redacted] [redacted] short");
    }

    fn reindex_temp_dir(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("maw-rs-reindex-gpu-{label}-{}", random_hex(6)));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }
}
