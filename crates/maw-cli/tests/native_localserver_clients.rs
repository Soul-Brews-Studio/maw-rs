#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::OnceLock;

use maw_cli::run_cli_async;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    body: String,
}

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    localserver_url: Option<OsString>,
    engine_url: Option<OsString>,
    port: Option<OsString>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            localserver_url: std::env::var_os("MAW_LOCALSERVER_URL"),
            engine_url: std::env::var_os("MAW_ENGINE_URL"),
            port: std::env::var_os("MAW_PORT"),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env("MAW_LOCALSERVER_URL", self.localserver_url.take());
        restore_env("MAW_ENGINE_URL", self.engine_url.take());
        restore_env("MAW_PORT", self.port.take());
    }
}

fn restore_env(key: &str, value: Option<OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

#[tokio::test]
async fn native_messages_gets_localserver_message_ledger() {
    let _guard = env_lock().lock().await;
    let _restore = EnvRestore::capture();
    let (base, rx) = spawn_one_response(r#"{"ok":true,"messages":[{"id":"m1"}],"total":1}"#).await;
    std::env::set_var("MAW_LOCALSERVER_URL", &base);
    std::env::remove_var("MAW_ENGINE_URL");
    std::env::remove_var("MAW_PORT");

    let output = run_cli_async(&args(&[
        "messages",
        "--limit",
        "5",
        "--from",
        "nova-codex-1",
        "--q",
        "hello world",
        "--json",
    ]))
    .await;

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        "{\"ok\":true,\"messages\":[{\"id\":\"m1\"}],\"total\":1}\n"
    );
    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "GET");
    assert_eq!(
        captured.path,
        "/api/message-ledger?limit=5&from=nova-codex-1&q=hello%20world&json=1"
    );
    assert_eq!(captured.body, "");
}

#[tokio::test]
async fn native_reply_posts_to_localserver_reply_endpoint() {
    let _guard = env_lock().lock().await;
    let _restore = EnvRestore::capture();
    let (base, rx) = spawn_one_response(r#"{"ok":true,"correlationId":"req-7"}"#).await;
    std::env::set_var("MAW_LOCALSERVER_URL", &base);
    std::env::remove_var("MAW_ENGINE_URL");
    std::env::remove_var("MAW_PORT");

    let output = run_cli_async(&args(&["reply", "req-7", "ack", "from", "rust"])).await;

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(output.stdout, "\u{1b}[32mreplied\u{1b}[0m → req-7\n");
    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/api/reply/req-7");
    assert_eq!(captured.body, r#"{"reply":"ack from rust"}"#);
}

#[tokio::test]
async fn native_reply_list_formats_seeded_localserver_requests() {
    let _guard = env_lock().lock().await;
    let _restore = EnvRestore::capture();
    let body = r#"{"requests":[{"correlationId":"req-9","from":"alice","message":"need input"}],"total":1}"#;
    let (base, rx) = spawn_one_response(body).await;
    std::env::set_var("MAW_LOCALSERVER_URL", &base);
    std::env::remove_var("MAW_ENGINE_URL");
    std::env::remove_var("MAW_PORT");

    let output = run_cli_async(&args(&["rp", "--list", "nova"])).await;

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        "  \u{1b}[36mreq-9\u{1b}[0m from \u{1b}[33malice\u{1b}[0m → need input\n\n1 pending request(s)\n"
    );
    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "GET");
    assert_eq!(captured.path, "/api/requests?status=delivered&oracle=nova");
    assert_eq!(captured.body, "");
}

#[tokio::test]
async fn native_health_probes_pinned_localserver() {
    let _guard = env_lock().lock().await;
    let _restore = EnvRestore::capture();
    let (base, rx) = spawn_one_response(r#"{"ok":true}"#).await;
    std::env::set_var("MAW_LOCALSERVER_URL", &base);
    std::env::remove_var("MAW_ENGINE_URL");
    std::env::remove_var("MAW_PORT");

    let output = run_cli_async(&args(&["health"])).await;

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stdout.contains("maw health"), "{}", output.stdout);
    assert!(output.stdout.contains("maw server"), "{}", output.stdout);
    assert!(output.stdout.contains("probe ok"), "{}", output.stdout);
    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/api/probe");
    assert_eq!(captured.body, "{}");
}

async fn spawn_one_response(
    body: &'static str,
) -> (String, tokio::sync::oneshot::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let captured = read_one_http_request(&mut socket).await;
        write_json_response(&mut socket, body).await;
        tx.send(captured).expect("send capture");
    });
    (format!("http://{addr}"), rx)
}

async fn write_json_response(socket: &mut tokio::net::TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    socket
        .write_all(response.as_bytes())
        .await
        .expect("response");
}

async fn read_one_http_request(socket: &mut tokio::net::TcpStream) -> CapturedRequest {
    let mut buffer = Vec::new();
    let mut temp = [0_u8; 1024];
    let header_end = loop {
        let read = socket.read(&mut temp).await.expect("read");
        assert_ne!(read, 0, "client closed before headers");
        buffer.extend_from_slice(&temp[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
    };
    let header_text = String::from_utf8(buffer[..header_end].to_vec()).expect("utf8 headers");
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().expect("request line");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().expect("method").to_owned();
    let path = request_parts.next().expect("path").to_owned();
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = socket.read(&mut temp).await.expect("read body");
        assert_ne!(read, 0, "client closed before body");
        buffer.extend_from_slice(&temp[..read]);
    }
    let body = String::from_utf8(buffer[body_start..body_start + content_length].to_vec())
        .expect("utf8 body");
    CapturedRequest { method, path, body }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
